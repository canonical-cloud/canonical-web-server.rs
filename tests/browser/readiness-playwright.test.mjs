import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { chromium } from "playwright";
import { chromeExecutablePath, startServer } from "./app-browser-harness.mjs";

const source = new URL("../../vendor/canonical-auditor-readiness/readiness/", import.meta.url);
const context = JSON.parse(await readFile(new URL("context.example.json", source), "utf8"));
const catalog = JSON.parse(await readFile(new URL("catalog.json", source), "utf8"));

async function withPage(t) {
  const server = await startServer();
  t.after(() => server.stop());
  const browser = await chromium.launch({
    executablePath: chromeExecutablePath(),
    headless: true,
    args: ["--no-sandbox", "--disable-setuid-sandbox"],
  });
  t.after(() => browser.close());
  const browserContext = await browser.newContext();
  const page = await browserContext.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  return { server, page, errors };
}

async function signIn(page, server) {
  await page.goto(`${server.url}/login`, { waitUntil: "networkidle" });
  await page.locator('input[name="email"]').fill("browser-e2e@canonical.invalid");
  await page.locator('input[name="password"]').fill("browser-e2e-only");
  await page.locator('form[action="/auth/login"] button[type="submit"]').click();
  await page.waitForURL(`${server.url}/app`);
}

async function start(page) {
  await page.locator("#context").waitFor({ state: "visible" });
  for (const [name, value] of Object.entries(context)) {
    await page.locator(`#context [name="${name}"]`).fill(value);
  }
  await page.locator("#start").click();
  await page.locator("#workspace").waitFor({ state: "visible" });
  assert.equal(await page.locator("#questions fieldset").count(), 10);
}

async function downloadText(page, button) {
  const pending = page.waitForEvent("download");
  await page.locator(button).click();
  const download = await pending;
  return readFile(await download.path(), "utf8");
}

async function importResponse(page, response) {
  await page.locator("#import").setInputFiles({
    name: "customer-draft.json",
    mimeType: "application/json",
    buffer: Buffer.from(JSON.stringify(response)),
  });
}

test("readiness: pages and embedded assets require a real session", async (t) => {
  const { server, page } = await withPage(t);
  for (const path of ["/app/readiness", "/app/readiness/soc2", "/app/readiness/assets/catalog.json", "/app/readiness/assets/browser.mjs"]) {
    const response = await page.request.get(`${server.url}${path}`, { maxRedirects: 0 });
    assert.equal(response.status(), 303);
    assert.equal(response.headers().location, "/login");
    assert.equal(response.headers()["cache-control"], "no-store");
  }
});

test("readiness: authenticated worksheets round-trip independently without uploading answers", async (t) => {
  const { server, page, errors } = await withPage(t);
  await signIn(page, server);
  assert.equal(await page.locator('nav a[href="/app/readiness"]').count(), 1);
  const writes = [];
  page.on("request", (request) => {
    if (!["GET", "HEAD"].includes(request.method())) writes.push(request.url());
  });
  const response = await page.goto(`${server.url}/app/readiness`, { waitUntil: "networkidle" });
  assert.equal(response.status(), 200);
  assert.equal(response.headers()["cache-control"], "no-store");
  assert.match(response.headers()["content-security-policy"], /default-src 'none'/);
  assert.doesNotMatch(response.headers()["content-security-policy"], /unsafe-inline|unsafe-eval/);
  assert.equal(await page.locator("#frameworks a").count(), 15);
  for (const framework of catalog.frameworks) {
    const result = await page.request.get(`${server.url}/app/readiness/${framework.id}`);
    assert.equal(result.status(), 200);
  }
  for (const path of ["/app/readiness/unknown", "/app/readiness/assets/unknown", "/app/readiness/assets/runtime-probe.mjs"]) {
    assert.equal((await page.request.get(`${server.url}${path}`)).status(), 404);
  }
  const script = await page.request.get(`${server.url}/app/readiness/assets/browser.mjs`);
  assert.match(script.headers()["content-type"], /^text\/javascript/);
  assert.equal(script.headers()["x-content-type-options"], "nosniff");

  await page.locator('a[href="/app/readiness/soc2"]').click();
  await start(page);
  await page.locator('[id="soc2.q01-status"]').selectOption("implemented");
  await page.locator('[id="soc2.q01-owner"]').fill("Security owner");
  await page.locator('[id="soc2.q01-notes"]').fill('<img src=x onerror="alert(1)"> evidence description');
  await page.locator('[id="soc2.q01-evidenceRef"]').fill("vault:e-001");
  await page.locator('[id="soc2.q01-evidenceDate"]').fill("2026-08-31");
  assert.equal(await page.locator("#questions img").count(), 0);
  assert.match(await page.locator("#summary").textContent(), /Answered 1\/10/);
  const packet = JSON.parse(await downloadText(page, "#export-json"));
  assert.equal(packet.frameworkId, "soc2");
  assert.deepEqual(packet.context, context);
  assert.equal(packet.answers[0].status, "implemented");
  assert.equal(packet.answers[0].evidenceRef, "vault:e-001");

  const bad = structuredClone(packet);
  bad.frameworkId = "gdpr";
  await importResponse(page, bad);
  await page.waitForFunction(() => document.getElementById("error").textContent.includes("Framework or version mismatch"));
  assert.equal(await page.locator('[id="soc2.q01-status"]').inputValue(), "implemented");
  bad.frameworkId = "soc2";
  bad.context.customerId = "other-customer";
  await importResponse(page, bad);
  await page.waitForFunction(() => document.getElementById("error").textContent.includes("Customer, assessment, scope"));

  Object.assign(packet.answers[1], { status: "partial", owner: "Operations", notes: "Restore drill outstanding", dueDate: "2026-09-01" });
  await importResponse(page, packet);
  await page.waitForFunction(() => document.getElementById("summary").textContent.includes("Declared gaps 1"));
  assert.match(await page.locator("#summary").textContent(), /Overdue 1/);
  const md = await downloadText(page, "#export-md");
  assert.match(md, /SOC 2 readiness checklist/);
  assert.match(md, /&lt;img/);
  assert.doesNotMatch(md, /<img/);
  assert.match(md, /not certification/);

  const independent = await page.context().newPage();
  await independent.goto(`${server.url}/app/readiness/gdpr`, { waitUntil: "networkidle" });
  await start(independent);
  assert.equal(await independent.locator('[id="gdpr.q01-status"]').inputValue(), "unanswered");
  assert.equal(await independent.locator('[id="gdpr.q01-owner"]').inputValue(), "");
  await independent.close();

  await page.setViewportSize({ width: 375, height: 900 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
  await page.emulateMedia({ media: "print" });
  assert.equal(await page.locator('[id="soc2.q01-notes"]').isVisible(), false);
  assert.equal(await page.locator("#questions .print-value").first().isVisible(), true);
  assert.deepEqual(errors, []);
  assert.deepEqual(writes, []);
});

async function openWorksheet(t) {
  const result = await withPage(t);
  result.page.setDefaultTimeout(10000);
  await signIn(result.page, result.server);
  await result.page.goto(`${result.server.url}/app/readiness/soc2`, { waitUntil: "networkidle" });
  await start(result.page);
  return result;
}

async function importWithDecision(page, packet, accept) {
  const pendingDialog = page.waitForEvent("dialog");
  const upload = importResponse(page, packet);
  const dialog = await pendingDialog;
  assert.equal(dialog.type(), "confirm");
  assert.match(dialog.message(), /Replace this tab/);
  if (accept) await dialog.accept();
  else await dialog.dismiss();
  await upload;
  await page.waitForFunction(() => document.getElementById("workspace").getAttribute("aria-busy") === "false");
}

test("readiness: every field-only edit survives a cancelled replacement", async (t) => {
  const { page, errors } = await openWorksheet(t);
  const blank = JSON.parse(await downloadText(page, "#export-json"));
  const fields = { status: "partial", owner: "Owner only", notes: "Explanation only", evidenceRef: "vault:e-001", evidenceDate: "2026-08-31", reviewer: "Reviewer only", dueDate: "2026-10-01" };
  for (const [field, value] of Object.entries(fields)) {
    const input = page.locator(`[id="soc2.q01-${field}"]`);
    if (field === "status") await input.selectOption(value);
    else await input.fill(value);
    await importWithDecision(page, blank, false);
    assert.equal(await input.inputValue(), value);
    assert.equal(await input.isDisabled(), false);
    if (field === "status") await input.selectOption("unanswered");
    else await input.fill("");
  }
  await page.locator('[id="soc2.q01-owner"]').fill("Explicitly replace me");
  await importWithDecision(page, blank, true);
  assert.equal(await page.locator('[id="soc2.q01-owner"]').inputValue(), "");
  assert.equal(await page.locator("#import").isDisabled(), false);
  assert.deepEqual(errors, []);
});

test("readiness: overlapping file reads cannot replace the active import", async (t) => {
  const { page, errors } = await openWorksheet(t);
  const packet = JSON.parse(await downloadText(page, "#export-json"));
  packet.answers[0].owner = "First import";
  await page.evaluate(() => {
    const original = File.prototype.arrayBuffer;
    window.restoreReadinessFileReader = () => { File.prototype.arrayBuffer = original; };
    window.readinessReadCount = 0;
    File.prototype.arrayBuffer = function () {
      window.readinessReadCount++;
      if (this.name === "delayed.json") return new Promise((resolve) => {
        window.finishReadinessRead = async () => resolve(await original.call(this));
      });
      if (this.name === "failure.json") return Promise.reject(new Error("PRIVATE_FILE_CANARY"));
      return original.call(this);
    };
  });
  await page.locator("#import").setInputFiles({ name: "delayed.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(packet)) });
  await page.waitForFunction(() => document.getElementById("workspace").getAttribute("aria-busy") === "true");
  assert.equal(await page.locator('[id="soc2.q01-owner"]').isDisabled(), true);
  assert.equal(await page.locator("#export-json").isDisabled(), true);
  const second = structuredClone(packet);
  second.answers[0].owner = "Second import must not win";
  // Deliberately bypass disabled UI controls to test the operation gate itself.
  await page.evaluate((raw) => {
    const input = document.getElementById("import"), transfer = new DataTransfer();
    transfer.items.add(new File([raw], "second.json", { type: "application/json" }));
    input.files = transfer.files;
    input.dispatchEvent(new Event("change", { bubbles: true }));
  }, JSON.stringify(second));
  assert.equal(await page.evaluate(() => window.readinessReadCount), 1);
  await page.evaluate(() => window.finishReadinessRead());
  await page.waitForFunction(() => document.getElementById("workspace").getAttribute("aria-busy") === "false");
  assert.equal(await page.locator('[id="soc2.q01-owner"]').inputValue(), "First import");
  await page.locator("#import").setInputFiles({ name: "failure.json", mimeType: "application/json", buffer: Buffer.from("{}") });
  await page.waitForFunction(() => document.getElementById("error").textContent.includes("Unable to read draft file"));
  assert.doesNotMatch(await page.locator("#error").textContent(), /PRIVATE_FILE_CANARY/);
  assert.equal(await page.locator('[id="soc2.q01-owner"]').inputValue(), "First import");
  assert.equal(await page.locator("#import").isDisabled(), false);
  await page.evaluate(() => window.restoreReadinessFileReader());
  await importWithDecision(page, second, true);
  assert.equal(await page.locator('[id="soc2.q01-owner"]').inputValue(), "Second import must not win");
  assert.deepEqual(errors, []);
});

test("readiness: malformed, oversized and ambiguous drafts preserve existing answers", async (t) => {
  const { page, errors } = await openWorksheet(t);
  const packet = JSON.parse(await downloadText(page, "#export-json"));
  await page.locator('[id="soc2.q01-owner"]').fill("Keep existing owner");
  const duplicate = JSON.stringify(packet).replace('"schemaVersion":', '"schemaVersion":"duplicate","schemaVersion":');
  const unknown = structuredClone(packet);
  unknown.answers[0].approved = true;
  const inputs = [Buffer.from("x".repeat(1048577)), Buffer.from([0xff]), Buffer.from(duplicate), Buffer.from(JSON.stringify(unknown)), Buffer.from("{broken")];
  for (const buffer of inputs) {
    await page.locator("#import").setInputFiles({ name: "invalid.json", mimeType: "application/json", buffer });
    await page.waitForFunction(() => {
      const input = document.getElementById("import");
      return !input.disabled && input.value === "" && document.getElementById("error").textContent.length > 0;
    });
    assert.equal(await page.locator('[id="soc2.q01-owner"]').inputValue(), "Keep existing owner");
  }
  await importWithDecision(page, packet, true);
  assert.equal(await page.locator('[id="soc2.q01-owner"]').inputValue(), "");
  assert.deepEqual(errors, []);
});

test("readiness: filtered views never truncate exports, workpapers or print coverage", async (t) => {
  const { page, errors } = await openWorksheet(t);
  const packet = JSON.parse(await downloadText(page, "#export-json"));
  Object.assign(packet.answers[0], { status: "implemented", owner: "Owner", notes: "No evidence yet" });
  Object.assign(packet.answers[1], { status: "partial", owner: "Owner", notes: "Gap", dueDate: "2026-09-01" });
  Object.assign(packet.answers[2], { status: "missing", owner: "Owner", notes: "Gap", dueDate: "2026-10-01", reviewer: "UNVERIFIED_REVIEWER_CANARY" });
  await importResponse(page, packet);
  await page.waitForFunction(() => document.getElementById("summary").textContent.includes("Answered 3/10"));
  for (const [view, count] of Object.entries({ all: 10, unanswered: 7, gaps: 2, evidence: 1, review: 2, overdue: 1 })) {
    await page.locator("#question-view").selectOption(view);
    assert.equal(await page.locator("#questions fieldset:visible").count(), count);
    assert.match(await page.locator("#summary").textContent(), /Answered 3\/10/);
  }
  const exported = JSON.parse(await downloadText(page, "#export-json"));
  assert.equal(exported.answers.length, 10);
  assert.deepEqual(exported, packet);
  const md = await downloadText(page, "#export-md");
  assert.match(md, /soc2.q10/);
  const workpaper = await downloadText(page, "#export-workpaper");
  assert.equal((workpaper.match(/Assessor outcome: not assessed/g) ?? []).length, 10);
  assert.equal((workpaper.match(/Independent review: pending/g) ?? []).length, 10);
  assert.doesNotMatch(workpaper, /UNVERIFIED_REVIEWER_CANARY|Assessor outcome: pass/);
  assert.match(workpaper, /completeness reconciliation/);
  assert.match(workpaper, /Retest evidence/);
  await page.emulateMedia({ media: "print" });
  assert.equal(await page.locator("#questions fieldset:visible").count(), 10);
  await page.emulateMedia({ media: "screen" });
  assert.equal(await page.locator("#questions fieldset:visible").count(), 1);
  assert.deepEqual(errors, []);
});

test("readiness: invalid native fields block export without stale summaries or focus loss", async (t) => {
  const { page, errors } = await openWorksheet(t);
  const downloads = [];
  page.on("download", (download) => downloads.push(download.suggestedFilename()));
  await page.locator('[id="soc2.q01-status"]').selectOption("implemented");
  const date = page.locator('[id="soc2.q01-evidenceDate"]');
  await date.fill("2026-09-08");
  assert.equal(await date.evaluate((input) => input.validity.rangeOverflow), true);
  assert.equal(await page.evaluate(() => document.activeElement.id), "soc2.q01-evidenceDate");
  assert.match(await page.locator("#summary").textContent(), /no current summary/);
  await page.locator("#export-json").click();
  assert.equal(await page.evaluate(() => document.activeElement.id), "error");
  assert.deepEqual(downloads, []);
  await date.fill("2026-08-31");
  await page.locator("#question-view").selectOption("evidence");
  const reference = page.locator('[id="soc2.q01-evidenceRef"]');
  await reference.fill("https://example.invalid/private");
  assert.equal(await reference.evaluate((input) => input.validity.patternMismatch), true);
  assert.equal(await reference.getAttribute("aria-invalid"), "true");
  assert.equal(await page.evaluate(() => document.activeElement.id), "soc2.q01-evidenceRef");
  assert.equal(await page.locator("#questions fieldset:visible").count(), 10);
  await page.locator("#export-workpaper").click();
  assert.deepEqual(downloads, []);
  await reference.fill("vault:e-001");
  assert.equal(await page.locator("#error").textContent(), "");
  assert.match(await page.locator("#summary").textContent(), /Answered 1\/10/);
  const output = JSON.parse(await downloadText(page, "#export-json"));
  assert.equal(output.answers[0].evidenceRef, "vault:e-001");
  assert.deepEqual(errors, []);
});
