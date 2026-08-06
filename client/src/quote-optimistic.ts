import {
  createOptoSyncClient,
  SYNC_STATUS,
  type JsonRecord,
  type LocalMutation,
  type OptoSyncClient,
} from "@opto-sync/client/browser";
import type htmx from "htmx.org";

const QUOTE_TABLE = "canonical_quote_intent";
const RETRY_INTERVAL_MS = 15_000;
const MAX_FIELD_COUNT = 64;
const MAX_FIELD_BYTES = 8_192;
const OMITTED_FIELDS = new Set(["csrf", "client_request_id"]);

type HtmxApi = typeof htmx;
export type QuoteFields = Record<string, string>;

interface HtmxConfirmDetail {
  issueRequest: (skipConfirmation?: boolean) => void;
}

interface HtmxAfterRequestDetail {
  successful?: boolean;
  xhr: XMLHttpRequest;
}

interface ActiveQuoteRequest {
  recordId: string;
  rowId: number;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function quoteFieldsFromForm(form: HTMLFormElement): QuoteFields {
  const fields: QuoteFields = {};
  for (const [name, value] of new FormData(form)) {
    if (OMITTED_FIELDS.has(name) || typeof value !== "string") {
      continue;
    }
    fields[name] = value;
  }
  return validateQuoteFields(fields);
}

export function quotePayload(fields: QuoteFields): JsonRecord {
  return {
    fields: validateQuoteFields(fields),
    state: "pending",
  };
}

export function quoteFieldsFromPayload(payload: unknown): QuoteFields {
  if (!isRecord(payload) || !isRecord(payload.fields)) {
    throw new Error("queued quote payload is invalid");
  }
  const fields: QuoteFields = {};
  for (const [name, value] of Object.entries(payload.fields)) {
    if (typeof value !== "string" || OMITTED_FIELDS.has(name)) {
      throw new Error("queued quote fields are invalid");
    }
    fields[name] = value;
  }
  return validateQuoteFields(fields);
}

function validateQuoteFields(fields: QuoteFields): QuoteFields {
  const entries = Object.entries(fields);
  if (entries.length === 0 || entries.length > MAX_FIELD_COUNT) {
    throw new Error("quote form contains an invalid number of fields");
  }
  for (const [name, value] of entries) {
    if (
      name.length === 0 ||
      name.length > 80 ||
      !/^[a-z0-9_]+$/u.test(name) ||
      new TextEncoder().encode(value).length > MAX_FIELD_BYTES
    ) {
      throw new Error("quote form contains an invalid field");
    }
  }
  return { ...fields };
}

export class QuoteWriteQueue {
  private constructor(readonly client: OptoSyncClient) {}

  static async open(accountKey: string): Promise<QuoteWriteQueue> {
    if (!/^[0-9a-f-]{36}$/iu.test(accountKey)) {
      throw new Error("quote account key is invalid");
    }
    const client = await createOptoSyncClient({
      databaseName: `canonical-quotes-${accountKey}`,
    });
    return new QuoteWriteQueue(client);
  }

  static fromClient(client: OptoSyncClient): QuoteWriteQueue {
    return new QuoteWriteQueue(client);
  }

  async enqueue(recordId: string, fields: QuoteFields): Promise<number> {
    return this.client.queueMutation(QUOTE_TABLE, recordId, quotePayload(fields));
  }

  async pending(): Promise<LocalMutation[]> {
    return this.client.pendingMutations(QUOTE_TABLE);
  }

  async localFields(recordId: string): Promise<QuoteFields> {
    const view = await this.client.localView(QUOTE_TABLE, recordId, {});
    return quoteFieldsFromPayload(view);
  }

  async markSynced(rowId: number): Promise<void> {
    await this.client.markMutation(rowId, SYNC_STATUS.SYNCED);
  }

  async markFailed(rowId: number): Promise<void> {
    await this.client.markMutation(rowId, SYNC_STATUS.FAILED);
  }

  async recordFailure(rowId: number, message: string): Promise<void> {
    await this.client.recordPushFailure(rowId, message);
  }

  async clearLocalData(): Promise<void> {
    await this.client.db.delete();
  }
}

export async function wireQuoteOptimisticWrites(api: HtmxApi): Promise<QuoteWriteQueue | undefined> {
  const form = document.querySelector<HTMLFormElement>('form[data-opto-quote="true"]');
  const results = document.querySelector<HTMLElement>("#quote-results");
  const status = document.querySelector<HTMLElement>("#quote-sync-status");
  const accountKey = document.querySelector<HTMLMetaElement>(
    'meta[name="canonical-quote-account"]',
  )?.content;
  if (form === null || results === null || accountKey === undefined || accountKey.length === 0) {
    return undefined;
  }

  const submit = form.querySelector<HTMLButtonElement>('button[type="submit"]');
  if (submit !== null) {
    submit.disabled = true;
  }
  setStatus(status, "Preparing durable optimistic writes…", "pending");

  const queue = await QuoteWriteQueue.open(accountKey);
  let active: ActiveQuoteRequest | undefined;
  let flushing = false;

  const renderRecord = (recordId: string, fields: QuoteFields, message: string, state: string): void => {
    let article = document.querySelector<HTMLElement>(`#quote-${recordId}`);
    if (article === null) {
      article = document.createElement("article");
      article.id = `quote-${recordId}`;
      article.className = "card";
      results.prepend(article);
    }
    article.dataset.optoState = state;
    const heading = document.createElement("h2");
    heading.textContent = fields.company_name ?? "Compliance quote";
    const body = document.createElement("p");
    body.textContent = message;
    const metadata = document.createElement("p");
    metadata.className = state === "failed" ? "error" : "muted";
    metadata.textContent =
      state === "failed"
        ? "This local write was not accepted. Review the form and submit a new request."
        : "Stored by opto-sync; safe to close this tab while delivery is pending.";
    article.replaceChildren(heading, body, metadata);
  };

  const renderPending = async (): Promise<void> => {
    for (const mutation of await queue.pending()) {
      renderRecord(
        mutation.recordId,
        await queue.localFields(mutation.recordId),
        "Your quote is saved locally and waiting for the server.",
        "pending",
      );
    }
  };

  const csrf = (): string =>
    form.querySelector<HTMLInputElement>('input[name="csrf"]')?.value ?? "";

  const deliver = async (mutation: LocalMutation): Promise<void> => {
    if (active?.recordId === mutation.recordId || mutation.id === undefined) {
      return;
    }
    const fields = await queue.localFields(mutation.recordId);
    const parameters = new URLSearchParams(fields);
    parameters.set("csrf", csrf());
    parameters.set("client_request_id", mutation.recordId);

    let response: Response;
    try {
      response = await fetch(form.action, {
        method: "POST",
        credentials: "same-origin",
        headers: {
          accept: "text/html",
          "content-type": "application/x-www-form-urlencoded;charset=UTF-8",
          "hx-request": "true",
        },
        body: parameters,
      });
    } catch (error) {
      await queue.recordFailure(
        mutation.id,
        error instanceof Error ? error.message : "network request failed",
      );
      return;
    }

    if (response.status === 401 || response.redirected) {
      const destination = response.headers.get("hx-redirect") ?? "/login";
      window.location.assign(destination);
      return;
    }
    if (response.ok) {
      const html = await response.text();
      const target = document.querySelector<HTMLElement>(`#quote-${mutation.recordId}`) ?? results;
      api.swap(target, html, {
        swapStyle: target === results ? "afterbegin" : "outerHTML",
        swapDelay: 0,
        settleDelay: 20,
      });
      await queue.markSynced(mutation.id);
      return;
    }
    if (response.status >= 400 && response.status < 500 && ![408, 429].includes(response.status)) {
      await queue.markFailed(mutation.id);
      renderRecord(
        mutation.recordId,
        fields,
        "The server rejected this saved quote.",
        "failed",
      );
      return;
    }
    await queue.recordFailure(mutation.id, `server returned ${response.status}`);
  };

  const flush = async (): Promise<void> => {
    if (flushing || !navigator.onLine) {
      return;
    }
    flushing = true;
    try {
      for (const mutation of await queue.pending()) {
        await deliver(mutation);
      }
    } finally {
      flushing = false;
    }
  };

  form.addEventListener("htmx:confirm", (event) => {
    const confirmation = event as CustomEvent<HtmxConfirmDetail>;
    event.preventDefault();
    if (!form.reportValidity() || active !== undefined) {
      return;
    }
    if (submit !== null) {
      submit.disabled = true;
    }

    void (async () => {
      const hidden = form.querySelector<HTMLInputElement>('input[name="client_request_id"]');
      const recordId = hidden?.value.match(/^[0-9a-f-]{36}$/iu)?.[0] ?? crypto.randomUUID();
      const fields = quoteFieldsFromForm(form);
      const rowId = await queue.enqueue(recordId, fields);
      active = { recordId, rowId };
      if (hidden !== null) {
        hidden.value = recordId;
      }
      renderRecord(
        recordId,
        fields,
        navigator.onLine
          ? "Your quote is saved locally and is being delivered."
          : "You are offline. Your quote will be delivered when the connection returns.",
        "pending",
      );
      form.setAttribute("hx-target", `#quote-${recordId}`);
      form.setAttribute("hx-swap", "outerHTML");
      setStatus(status, "Saved locally; delivery in progress…", "pending");
      confirmation.detail.issueRequest(true);
    })().catch((error: unknown) => {
      if (submit !== null) {
        submit.disabled = false;
      }
      setStatus(
        status,
        error instanceof Error ? error.message : "The quote could not be saved locally.",
        "failed",
      );
    });
  });

  form.addEventListener("htmx:afterRequest", (event) => {
    if (active === undefined) {
      return;
    }
    const completed = active;
    const detail = (event as CustomEvent<HtmxAfterRequestDetail>).detail;
    active = undefined;
    form.setAttribute("hx-target", "#quote-results");
    form.setAttribute("hx-swap", "afterbegin");
    if (submit !== null) {
      submit.disabled = false;
    }

    if (detail.successful === true && detail.xhr.status >= 200 && detail.xhr.status < 300) {
      void queue.markSynced(completed.rowId);
      form.reset();
      const hidden = form.querySelector<HTMLInputElement>('input[name="client_request_id"]');
      if (hidden !== null) {
        hidden.value = crypto.randomUUID();
      }
      setStatus(status, "Delivered. The server is analyzing your quote.", "synced");
      return;
    }
    if (
      detail.xhr.status >= 400 &&
      detail.xhr.status < 500 &&
      ![408, 429].includes(detail.xhr.status)
    ) {
      void queue.markFailed(completed.rowId);
      setStatus(status, "The server rejected this write; review the highlighted error.", "failed");
      return;
    }
    void queue.recordFailure(completed.rowId, `request ended with ${detail.xhr.status}`);
    setStatus(status, "Delivery was interrupted; the local write remains queued.", "pending");
  });

  window.addEventListener("online", () => void flush());
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") {
      void flush();
    }
  });
  window.setInterval(() => void flush(), RETRY_INTERVAL_MS);

  await renderPending();
  if (submit !== null) {
    submit.disabled = false;
  }
  setStatus(status, "Optimistic writes are ready.", "synced");
  void flush();
  return queue;
}

function setStatus(element: HTMLElement | null, message: string, state: string): void {
  if (element === null) {
    return;
  }
  element.textContent = message;
  element.dataset.state = state;
}
