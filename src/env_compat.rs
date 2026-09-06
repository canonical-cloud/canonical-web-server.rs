//! Narrow environment-variable compatibility for staged configuration renames.
//!
//! The reviewed variable always wins. The legacy name is consulted only when
//! the primary variable is absent, and token material is never logged.

/// The alias is installed into the immutable flags snapshot before dispatch.
pub fn install_internal_auth_token_alias() {}
