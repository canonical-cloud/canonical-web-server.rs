import { createOptoSyncClient, OptoSyncClient } from "@opto-sync/client/browser";
import { Subject } from "rxjs";
import { describe, expect, it, vi } from "vitest";
import {
  INITIAL_QUOTE_DELIVERY_SCHEDULE,
  QuoteDeliveryScheduler,
  reduceQuoteDeliverySchedule,
} from "../src/quote-delivery-scheduler";
import {
  QuoteWriteQueue,
  quoteFieldsFromPayload,
  quotePayload,
  type QuoteFields,
} from "../src/quote-optimistic";

const fields: QuoteFields = {
  company_name: "Example Incorporated",
  contact_name: "Casey Example",
  employee_count: "42",
  infrastructure: "AWS, Cloudflare",
  soc2_type_2: "on",
};

describe("opto-backed quote writes", () => {
  it("keeps credentials and request ids outside the durable payload", () => {
    const payload = quotePayload(fields);
    expect(quoteFieldsFromPayload(payload)).toEqual(fields);
    expect(() =>
      quoteFieldsFromPayload({ fields: { ...fields, csrf: "must-not-persist" } }),
    ).toThrow(/invalid/u);
  });

  it("survives a client restart and settles only after acknowledgement", async () => {
    const databaseName = `canonical-quote-test-${crypto.randomUUID()}`;
    const firstClient = await createOptoSyncClient({ databaseName });
    const first = QuoteWriteQueue.fromClient(firstClient);
    const recordId = crypto.randomUUID();
    const rowId = await first.enqueue(recordId, fields);

    const queued = await first.pending();
    expect(queued).toHaveLength(1);
    expect(queued[0]?.recordId).toBe(recordId);
    expect(queued[0]?.clientId).toMatch(/^[0-9a-f-]+$/iu);
    expect(queued[0]?.mutationId).toBe("1");
    expect(await first.localFields(recordId)).toEqual(fields);
    firstClient.db.close();

    const reopenedClient = new OptoSyncClient({ databaseName });
    const reopened = QuoteWriteQueue.fromClient(reopenedClient);
    expect(await reopened.pending()).toHaveLength(1);
    expect(await reopened.localFields(recordId)).toEqual(fields);

    await reopened.markSynced(rowId);
    expect(await reopened.pending()).toHaveLength(0);
    await reopened.clearLocalData();
  });
});

describe("quote delivery scheduler", () => {
  it("models an explicit immutable trailing-flush transition", () => {
    const firstRun = reduceQuoteDeliverySchedule(
      INITIAL_QUOTE_DELIVERY_SCHEDULE,
      { type: "trigger" },
    );
    const pendingWake = reduceQuoteDeliverySchedule(firstRun, { type: "trigger" });
    const trailingRun = reduceQuoteDeliverySchedule(pendingWake, { type: "settled" });
    const settled = reduceQuoteDeliverySchedule(trailingRun, { type: "settled" });

    expect(firstRun).toEqual({ phase: "flushing", flushRequested: false, run: 1 });
    expect(pendingWake).toEqual({ phase: "flushing", flushRequested: true, run: 1 });
    expect(trailingRun).toEqual({ phase: "flushing", flushRequested: false, run: 2 });
    expect(settled).toEqual({ phase: "idle", flushRequested: false, run: 2 });
    expect(INITIAL_QUOTE_DELIVERY_SCHEDULE).toEqual({
      phase: "idle",
      flushRequested: false,
      run: 0,
    });
  });

  it("coalesces wakes into one serial trailing flush", async () => {
    const wakeSignals = new Subject<void>();
    let resolveFirstFlush: (() => void) | undefined;
    const firstFlush = new Promise<void>((resolve) => {
      resolveFirstFlush = resolve;
    });
    let attempts = 0;
    const flush = vi.fn(() => {
      attempts += 1;
      return attempts === 1 ? firstFlush : Promise.resolve();
    });
    const scheduler = new QuoteDeliveryScheduler({
      flush,
      onFlushError: vi.fn(),
    });
    scheduler.start(wakeSignals);

    wakeSignals.next();
    await vi.waitFor(() => expect(flush).toHaveBeenCalledTimes(1));
    wakeSignals.next();
    wakeSignals.next();
    expect(flush).toHaveBeenCalledTimes(1);

    resolveFirstFlush?.();
    await vi.waitFor(() => expect(flush).toHaveBeenCalledTimes(2));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(flush).toHaveBeenCalledTimes(2);
    scheduler.stop();
  });

  it("contains flush errors at the effect boundary", async () => {
    const wakeSignals = new Subject<void>();
    const failure = new Error("offline");
    const onFlushError = vi.fn();
    const scheduler = new QuoteDeliveryScheduler({
      flush: async () => Promise.reject(failure),
      onFlushError,
    });
    scheduler.start(wakeSignals);

    wakeSignals.next();
    await vi.waitFor(() => expect(onFlushError).toHaveBeenCalledWith(failure));
    scheduler.stop();
  });
});
