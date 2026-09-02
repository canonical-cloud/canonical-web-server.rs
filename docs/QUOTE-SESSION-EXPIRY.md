# Quote session expiry: full-page sign-in without POST replay

Status: source candidate; retain full Rust, client, and browser release gates.

All three authenticated quote routes share one expiry response: the form page,
quote-detail polling, and form submission. Authentication remains mandatory.

A single exact `HX-Request: true` selects a 401 response with `HX-Redirect` and
`HX-Reswap: none`. The pinned HTMX 2 client must navigate the top-level page to
the existing Shared Auth sign-in ceremony, not insert login HTML into a quote
status fragment. Header presence, false values, and duplicate values do not
select this representation.

Other requests receive 303 See Other. This intentionally changes an expired
POST submission into a GET of the sign-in endpoint instead of replaying customer
answers, CSRF tokens, or the idempotency key through a 307 redirect.

The destination uses only the validated application base URL, fixed sign-in
path, fixed client ID, and relative `/u/quote` return. Caller Host, forwarded,
HTMX-current-URL, target, and referrer headers never choose the destination.
The existing private-route no-store layer remains intact; the expiry helper
also carries no-store, emits an empty body, and does not set cookies.

## Verification

Run the existing locked Rust tests, strict Clippy/formatting, client tests, and
browser E2E. New Rust unit tests cover 303 semantics, HTMX denial/redirect, exact
header parsing, caller-origin rejection, clean return construction, and empty
responses. The original golden request and owner-context tests remain unchanged.

Before deployment, exercise real session expiry during both quote polling and
submission with the pinned browser client. Confirm full-page reauthentication,
no login-markup fragment swap, no POST to sign-in, no duplicate quote on replay,
and continued Origin/CSRF and account-isolation rejection. This change does not
promise automatic restoration of a form after reauthentication.

References: [HTMX HX-Redirect](https://htmx.org/headers/hx-redirect/) and
[HTTP 303](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.4.4).
