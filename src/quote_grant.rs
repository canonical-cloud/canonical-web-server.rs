use std::fmt;

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm,
};
use axum::http::HeaderMap;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;

const AAD: &[u8] = b"canonical-plus/quote-grant/v1";
const MAX_ENVELOPE_BYTES: usize = 4_096;
const SECURE_COOKIE_NAME: &str = "__Host-canonical-quote-grant";
const LOOPBACK_COOKIE_NAME: &str = "canonical-quote-grant";

#[derive(Clone)]
pub(crate) struct QuoteGrantCodec {
    cipher: Aes256Gcm,
    cookie_name: &'static str,
    secure: bool,
}

impl QuoteGrantCodec {
    pub(crate) fn new(key: &[u8], secure: bool) -> Result<Self, AppError> {
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| AppError::Crypto)?;
        Ok(Self {
            cipher,
            cookie_name: if secure {
                SECURE_COOKIE_NAME
            } else {
                LOOPBACK_COOKIE_NAME
            },
            secure,
        })
    }

    pub(crate) fn issue(
        &self,
        owner_subject: Uuid,
        quote_id: Uuid,
        link_expires_at: &str,
    ) -> Result<(Cookie<'static>, QuoteGrant), AppError> {
        let link_expires_at = DateTime::parse_from_rfc3339(link_expires_at)
            .map_err(|_| AppError::ServiceUpstream)?
            .with_timezone(&Utc);
        let expires_at = std::cmp::min(link_expires_at, Utc::now() + Duration::hours(1));
        if expires_at <= Utc::now() {
            return Err(AppError::NotFound);
        }
        let grant = QuoteGrant {
            csrf_token: URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>()),
            expires_at,
            owner_subject,
            quote_id,
        };
        let encoded = self.seal(&grant)?;
        let cookie = Cookie::build((self.cookie_name, encoded))
            .path("/")
            .http_only(true)
            .secure(self.secure)
            .same_site(SameSite::Lax)
            .max_age(time::Duration::hours(1))
            .build();
        Ok((cookie, grant))
    }

    pub(crate) fn authenticate(
        &self,
        headers: &HeaderMap,
        expected_quote_id: Uuid,
    ) -> Result<QuoteGrant, AppError> {
        let jar = CookieJar::from_headers(headers);
        let encoded = jar
            .get(self.cookie_name)
            .map(|cookie| cookie.value())
            .ok_or(AppError::Unauthorized)?;
        let grant = self.open(encoded)?;
        if grant.quote_id != expected_quote_id || grant.expires_at <= Utc::now() {
            return Err(AppError::Unauthorized);
        }
        Ok(grant)
    }

    fn seal(&self, grant: &QuoteGrant) -> Result<String, AppError> {
        let plaintext = serde_json::to_vec(grant).map_err(|_| AppError::Crypto)?;
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = nonce_bytes
            .as_slice()
            .try_into()
            .map_err(|_| AppError::Crypto)?;
        let ciphertext = self
            .cipher
            .encrypt(
                nonce,
                Payload {
                    msg: &plaintext,
                    aad: AAD,
                },
            )
            .map_err(|_| AppError::Crypto)?;
        let mut envelope = nonce_bytes.to_vec();
        envelope.extend(ciphertext);
        Ok(URL_SAFE_NO_PAD.encode(envelope))
    }

    fn open(&self, encoded: &str) -> Result<QuoteGrant, AppError> {
        if encoded.len() > MAX_ENVELOPE_BYTES {
            return Err(AppError::Unauthorized);
        }
        let envelope = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| AppError::Unauthorized)?;
        let (nonce_bytes, ciphertext) = envelope
            .split_at_checked(12)
            .ok_or(AppError::Unauthorized)?;
        let nonce = nonce_bytes.try_into().map_err(|_| AppError::Unauthorized)?;
        let plaintext = self
            .cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: AAD,
                },
            )
            .map_err(|_| AppError::Unauthorized)?;
        let grant: QuoteGrant =
            serde_json::from_slice(&plaintext).map_err(|_| AppError::Unauthorized)?;
        if grant.owner_subject.is_nil()
            || grant.quote_id.is_nil()
            || grant.csrf_token.len() != 43
            || grant.expires_at > Utc::now() + Duration::hours(2)
        {
            return Err(AppError::Unauthorized);
        }
        Ok(grant)
    }
}

impl fmt::Debug for QuoteGrantCodec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QuoteGrantCodec")
            .field("cipher", &"[redacted]")
            .field("cookie_name", &self.cookie_name)
            .field("secure", &self.secure)
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct QuoteGrant {
    pub(crate) csrf_token: String,
    pub(crate) expires_at: DateTime<Utc>,
    pub(crate) owner_subject: Uuid,
    pub(crate) quote_id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_is_encrypted_scoped_and_short_lived() {
        let codec = QuoteGrantCodec::new(&[9; 32], false).unwrap();
        let quote_id = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let expires = (Utc::now() + Duration::days(25)).to_rfc3339();
        let (cookie, issued) = codec.issue(owner, quote_id, &expires).unwrap();
        assert!(!cookie.value().contains(&owner.to_string()));
        let headers = HeaderMap::from_iter([(
            axum::http::header::COOKIE,
            format!("{}={}", cookie.name(), cookie.value())
                .parse()
                .unwrap(),
        )]);
        let opened = codec.authenticate(&headers, quote_id).unwrap();
        assert_eq!(opened.owner_subject, owner);
        assert_eq!(opened.csrf_token, issued.csrf_token);
        assert!(codec.authenticate(&headers, Uuid::new_v4()).is_err());
    }
}
