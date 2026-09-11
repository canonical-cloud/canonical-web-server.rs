import {
  EMPTY,
  Subject,
  Subscription,
  catchError,
  concatMap,
  defer,
  distinctUntilChanged,
  filter,
  finalize,
  fromEvent,
  map,
  merge,
  of,
  scan,
  timer,
  type Observable,
} from "rxjs";

export type QuoteDeliveryScheduleAction =
  | Readonly<{ type: "trigger" }>
  | Readonly<{ type: "settled" }>;

export interface QuoteDeliveryScheduleState {
  readonly phase: "idle" | "flushing";
  // A second wake while a request is in flight must result in one trailing
  // pass, but never in concurrent writes or an unbounded timer backlog.
  readonly flushRequested: boolean;
  readonly run: number;
}

export const INITIAL_QUOTE_DELIVERY_SCHEDULE: QuoteDeliveryScheduleState = {
  phase: "idle",
  flushRequested: false,
  run: 0,
};

const TRIGGER: QuoteDeliveryScheduleAction = { type: "trigger" };
const SETTLED: QuoteDeliveryScheduleAction = { type: "settled" };

/**
 * The scheduler's complete state machine. It has no browser or IndexedDB
 * effects, so delivery coalescing is independently testable from DOM wiring.
 */
export function reduceQuoteDeliverySchedule(
  state: QuoteDeliveryScheduleState,
  action: QuoteDeliveryScheduleAction,
): QuoteDeliveryScheduleState {
  switch (action.type) {
    case "trigger":
      if (state.phase === "idle") {
        return { phase: "flushing", flushRequested: false, run: state.run + 1 };
      }
      return state.flushRequested ? state : { ...state, flushRequested: true };
    case "settled":
      if (state.phase === "idle") {
        return state;
      }
      return state.flushRequested
        ? { phase: "flushing", flushRequested: false, run: state.run + 1 }
        : { ...state, phase: "idle" };
  }
}

export interface QuoteDeliverySchedulerOptions {
  readonly flush: () => Promise<void>;
  readonly onFlushError: (error: unknown) => void;
}

/**
 * Serializes optimistic-delivery effects. RxJS is intentionally limited to
 * the effect boundary; reducer state remains immutable and pure.
 */
export class QuoteDeliveryScheduler {
  private readonly actions = new Subject<QuoteDeliveryScheduleAction>();
  private subscription: Subscription | null = null;

  constructor(private readonly options: QuoteDeliverySchedulerOptions) {}

  start(wakeSignals: Observable<unknown>): void {
    if (this.subscription !== null) {
      return;
    }
    const subscription = new Subscription();
    this.subscription = subscription;

    subscription.add(
      this.actions
        .pipe(
          scan(reduceQuoteDeliverySchedule, INITIAL_QUOTE_DELIVERY_SCHEDULE),
          // A wake during a run leaves its run number unchanged. The next run
          // is emitted only after the reducer observes the current one settled.
          distinctUntilChanged((previous, next) => previous.run === next.run),
          filter((state) => state.phase === "flushing"),
          concatMap(() =>
            defer(this.options.flush).pipe(
              catchError((error: unknown) => {
                this.options.onFlushError(error);
                return EMPTY;
              }),
              // Feeding completion back through the reducer keeps every
              // transition decision in one explicit state machine.
              finalize(() => this.actions.next(SETTLED)),
            ),
          ),
        )
        .subscribe(),
    );
    subscription.add(
      wakeSignals.subscribe({
        next: () => this.actions.next(TRIGGER),
        error: (error: unknown) => this.options.onFlushError(error),
      }),
    );
  }

  stop(): void {
    const subscription = this.subscription;
    this.subscription = null;
    subscription?.unsubscribe();
  }
}

export interface QuoteDeliveryWakeSources {
  readonly onlineTarget: EventTarget;
  readonly visibilityTarget: EventTarget;
  readonly isVisible: () => boolean;
  readonly retryIntervalMs: number;
}

/**
 * Converts browser wake sources into a single stream. The scheduler owns
 * deduplication and serialization, while this adapter remains declarative.
 */
export function quoteDeliveryWakeSignals(
  sources: QuoteDeliveryWakeSources,
): Observable<void> {
  return merge(
    of(undefined),
    fromEvent(sources.onlineTarget, "online").pipe(map(() => undefined)),
    fromEvent(sources.visibilityTarget, "visibilitychange").pipe(
      filter(() => sources.isVisible()),
      map(() => undefined),
    ),
    timer(sources.retryIntervalMs, sources.retryIntervalMs).pipe(map(() => undefined)),
  );
}
