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
