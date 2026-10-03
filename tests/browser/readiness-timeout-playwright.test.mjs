import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { chromium } from "playwright";
import { chromeExecutablePath, startServer } from "./app-browser-harness.mjs";

const context = JSON.parse(await readFile(new URL("../../vendor/canonical-auditor-readiness/readiness/context.example.json", import.meta.url), "utf8"));

test("readiness: a stalled file read releases controls and cannot overwrite a newer draft", async (t) => {
  const server = await startServer();
  t.after(() => server.stop());
  const browser = await chromium.launch({ executablePath: chromeExecutablePath(), headless: true, args: ["--no-sandbox", "--disable-setuid-sandbox"] });
  t.after(() => browser.close());
  const browserContext = await browser.newContext();
  const page = await browserContext.newPage();
  page.setDefaultTimeout(15000);
  const errors = [], dialogs = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("dialog", async (dialog) => { dialogs.push(dialog.type()); await dialog.dismiss(); });
  await page.goto(`${server.url}/login`, { waitUntil: "networkidle" });
  await page.locator('input[name="email"]').fill("browser-e2e@canonical.invalid");
  await page.locator('input[name="password"]').fill("browser-e2e-only");
  await page.locator('form[action="/auth/login"] button[type="submit"]').click();
  await page.waitForURL(`${server.url}/app`);
  await page.goto(`${server.url}/app/readiness/soc2`, { waitUntil: "networkidle" });
  await page.locator("#context").waitFor({ state: "visible" });
  for (const [name, value] of Object.entries(context)) await page.locator(`#context [name="${name}"]`).fill(value);
  await page.locator("#start").click();
  const owner = page.locator('[id="soc2.q01-owner"]');
  await owner.fill("Keep existing work");

  await page.evaluate(() => {
    const original = File.prototype.arrayBuffer;
    window.restoreTimedOutFileReader = () => { File.prototype.arrayBuffer = original; };
    File.prototype.arrayBuffer = function () {
      return new Promise((resolve) => { window.finishTimedOutFileRead = resolve; });
    };
  });
  await page.locator("#import").setInputFiles({ name: "stalled.json", mimeType: "application/json", buffer: Buffer.from("{}") });
  await page.waitForFunction(() => document.getElementById("workspace").getAttribute("aria-busy") === "true");
  assert.equal(await owner.isDisabled(), true);
  await page.waitForFunction(() => document.getElementById("error").textContent.includes("read timed out"));
  assert.equal(await owner.inputValue(), "Keep existing work");
  assert.equal(await owner.isDisabled(), false);
  assert.equal(await page.locator("#export-json").isDisabled(), false);

  const downloading = page.waitForEvent("download");
  await page.locator("#export-json").click();
  const download = await downloading;
  const saved = JSON.parse(await readFile(await download.path(), "utf8"));
  assert.equal(saved.answers[0].owner, "Keep existing work");
  assert.equal(saved.answers.length, 10);
  await page.evaluate(() => window.restoreTimedOutFileReader());
  const newer = structuredClone(saved);
  newer.answers[0].owner = "Newer successful draft";
  await page.locator("#import").setInputFiles({ name: "newer.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(newer)) });
  await page.waitForFunction(() => document.getElementById("soc2.q01-owner").value === "Newer successful draft");
  const stale = structuredClone(saved);
  stale.answers[0].owner = "Stale read must not win";
  await page.evaluate((raw) => window.finishTimedOutFileRead(new TextEncoder().encode(raw).buffer), JSON.stringify(stale));
  await page.evaluate(() => Promise.resolve());
  assert.equal(await owner.inputValue(), "Newer successful draft");
  assert.equal(await page.locator("#error").textContent(), "");
  assert.equal(await page.locator("#workspace").getAttribute("aria-busy"), "false");
  assert.deepEqual(dialogs, []);
  assert.deepEqual(errors, []);
});
