//! Narrow environment-variable compatibility for staged configuration renames.
//!
//! The reviewed variable always wins. The legacy name is consulted only when
//! the primary variable is absent, and token material is never logged.

const INTERNAL_AUTH_TOKEN_ENV: &str = "CANONICAL_INTERNAL_AUTH_TOKEN";
const LEGACY_WEB_SERVICE_TOKEN_ENV: &str = "CANONICAL_WEB_SERVICE_TOKEN";

pub fn install_internal_auth_token_alias() {
    if std::env::var_os(INTERNAL_AUTH_TOKEN_ENV).is_none() {
        if let Some(token) = std::env::var_os(LEGACY_WEB_SERVICE_TOKEN_ENV) {
            std::env::set_var(INTERNAL_AUTH_TOKEN_ENV, token);
        }
    }
}
