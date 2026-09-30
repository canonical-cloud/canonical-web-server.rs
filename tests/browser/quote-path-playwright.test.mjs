import assert from "node:assert/strict";
import { test } from "node:test";
import { chromium } from "playwright";
import { chromeExecutablePath, startServer } from "./app-browser-harness.mjs";

const QUOTE_ID = "11111111-1111-4111-8111-111111111111";

async function withBrowser(t) {
  const server = await startServer();
  t.after(() => server.stop());
  const browser = await chromium.launch({
    executablePath: chromeExecutablePath(),
    headless: true,
    args: ["--no-sandbox", "--disable-setuid-sandbox"],
  });
  t.after(() => browser.close());
  return { server, request: await browser.newContext().then((context) => {
    t.after(() => context.close());
    return context.request;
  }) };
}

function assertCanonicalSignIn(location, serverUrl) {
  const target = new URL(location, serverUrl);
  assert.equal(target.origin, serverUrl);
  assert.equal(target.pathname, "/shared-auth/auth/browser/sign-in");
  assert.equal(target.searchParams.get("client_id"), "canonical-web");
  assert.equal(target.searchParams.get("return"), "/quote");
  assert.equal(target.searchParams.size, 2);
}

for (const path of ["/quote", "/u/quote", `/quote/${QUOTE_ID}`, `/u/quote/${QUOTE_ID}`]) {
  test(`playwright: ${path} preserves auth denial and canonicalizes the return target`, async (t) => {
    const { server, request } = await withBrowser(t);

    const browserResponse = await request.get(`${server.url}${path}`, { maxRedirects: 0 });
    assert.equal(browserResponse.status(), 303);
    assert.equal(browserResponse.headers()["cache-control"], "no-store");
    assertCanonicalSignIn(browserResponse.headers().location, server.url);

    const htmxResponse = await request.get(`${server.url}${path}`, {
      headers: { "hx-request": "true" },
      maxRedirects: 0,
    });
    assert.equal(htmxResponse.status(), 401);
    assert.equal(htmxResponse.headers()["cache-control"], "no-store");
    assert.equal(htmxResponse.headers()["hx-reswap"], "none");
    assert.ok(!("location" in htmxResponse.headers()));
    assertCanonicalSignIn(htmxResponse.headers()["hx-redirect"], server.url);
  });
}
