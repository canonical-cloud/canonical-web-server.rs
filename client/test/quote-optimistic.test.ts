import { createOptoSyncClient, OptoSyncClient } from "@opto-sync/client/browser";
import { describe, expect, it } from "vitest";
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
