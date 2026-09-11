use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

/// Bounded, per-process protection for the password endpoint. The edge still
/// enforces the authoritative per-IP limit: application processes cannot
/// safely infer a browser IP behind an untrusted proxy header.
#[derive(Clone)]
pub struct LoginRateLimiter {
    state: Arc<Mutex<State>>,
    policy: RateLimitPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct State {
    attempts: HashMap<String, AttemptWindow>,
    global: AttemptWindow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AttemptWindow {
    opened_at: Instant,
    attempts: u32,
}

#[derive(Clone, Copy)]
struct RateLimitPolicy {
    max_attempts: u32,
    global_max_attempts: u32,
    window: Duration,
    max_keys: usize,
}

struct RateLimitAttempt {
    account_key: String,
    observed_at: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RateLimitRejection {
    AccountBudget,
    GlobalBudget,
    KeyCapacity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RateLimitOutcome {
    Allowed,
    Rejected {
        reason: RateLimitRejection,
        retry_after_seconds: u64,
    },
}

struct RateLimitTransition {
    state: State,
    outcome: RateLimitOutcome,
}

impl State {
    fn empty(opened_at: Instant) -> Self {
        Self {
            attempts: HashMap::new(),
            global: AttemptWindow {
                opened_at,
                attempts: 0,
            },
        }
    }
}

impl RateLimitOutcome {
    fn into_result(self) -> Result<(), u64> {
        match self {
            Self::Allowed => Ok(()),
            Self::Rejected {
                retry_after_seconds,
                ..
            } => Err(retry_after_seconds),
        }
    }
}

impl LoginRateLimiter {
    pub fn new(
        max_attempts: u32,
        global_max_attempts: u32,
        window: Duration,
        max_keys: usize,
    ) -> Self {
        let now = Instant::now();
        Self {
            state: Arc::new(Mutex::new(State::empty(now))),
            policy: RateLimitPolicy {
                max_attempts,
                global_max_attempts,
                window,
                max_keys,
            },
        }
    }

    /// Records an attempt before contacting Supabase. The key is a SHA-256
    /// digest of a normalized email so raw identifiers never enter metrics or
    /// logs through this component.
    pub async fn check(&self, email: &str) -> Result<(), u64> {
        let now = Instant::now();
        let mut state = self.state.lock().await;
        // Locking and wall-clock acquisition stay at the effect boundary. The
        // security decision itself is one explicit, deterministic transition.
        let previous = std::mem::replace(&mut *state, State::empty(now));
        let transition = reduce_rate_limit(
            previous,
            RateLimitAttempt {
                account_key: account_key(email),
                observed_at: now,
            },
            self.policy,
        );
        *state = transition.state;
        transition.outcome.into_result()
    }
}

/// Pure state transition for one login attempt. All inputs are explicit and
/// the complete next state and typed outcome are returned together.
fn reduce_rate_limit(
    mut state: State,
    attempt: RateLimitAttempt,
    policy: RateLimitPolicy,
) -> RateLimitTransition {
    let now = attempt.observed_at;
    if now.saturating_duration_since(state.global.opened_at) >= policy.window {
        state.global = AttemptWindow {
            opened_at: now,
            attempts: 0,
        };
    }
    state.global.attempts = state.global.attempts.saturating_add(1);
    if state.global.attempts > policy.global_max_attempts {
        let opened_at = state.global.opened_at;
        return rejected_transition(
            state,
            RateLimitRejection::GlobalBudget,
            now,
            opened_at,
            policy.window,
        );
    }

    state
        .attempts
        .retain(|_, entry| now.saturating_duration_since(entry.opened_at) < policy.window);

    if !state.attempts.contains_key(&attempt.account_key) && state.attempts.len() >= policy.max_keys
    {
        let oldest = state
            .attempts
            .values()
            .map(|entry| entry.opened_at)
            .min()
            .unwrap_or(now);
        // Never evict a live account window: key churn must not erase a
        // targeted account's block. Capacity exhaustion fails closed until
        // the oldest window expires.
        return RateLimitTransition {
            state,
            outcome: RateLimitOutcome::Rejected {
                reason: RateLimitRejection::KeyCapacity,
                retry_after_seconds: retry_after(now, oldest, policy.window),
            },
        };
    }

    let entry = state
        .attempts
        .entry(attempt.account_key)
        .or_insert(AttemptWindow {
            opened_at: now,
            attempts: 0,
        });
    entry.attempts = entry.attempts.saturating_add(1);
    if entry.attempts > policy.max_attempts {
        let opened_at = entry.opened_at;
        return rejected_transition(
            state,
            RateLimitRejection::AccountBudget,
            now,
            opened_at,
            policy.window,
        );
    }
    RateLimitTransition {
        state,
        outcome: RateLimitOutcome::Allowed,
    }
}

fn rejected_transition(
    state: State,
    reason: RateLimitRejection,
    now: Instant,
    opened_at: Instant,
    window: Duration,
) -> RateLimitTransition {
    RateLimitTransition {
        state,
        outcome: RateLimitOutcome::Rejected {
            reason,
            retry_after_seconds: retry_after(now, opened_at, window),
        },
    }
}

fn retry_after(now: Instant, opened_at: Instant, window: Duration) -> u64 {
    let remaining = window.saturating_sub(now.saturating_duration_since(opened_at));
    remaining
        .as_secs()
        .saturating_add(u64::from(remaining.subsec_nanos() > 0))
        .max(1)
}

fn account_key(email: &str) -> String {
    let normalized = email.trim().to_ascii_lowercase();
    URL_SAFE_NO_PAD.encode(Sha256::digest(normalized.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{
        account_key, reduce_rate_limit, LoginRateLimiter, RateLimitAttempt, RateLimitOutcome,
        RateLimitPolicy, RateLimitRejection, State,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn pure_transition_returns_typed_outcomes_without_mutating_its_input() {
        let now = Instant::now();
        let policy = RateLimitPolicy {
            max_attempts: 1,
            global_max_attempts: 100,
            window: Duration::from_secs(60),
            max_keys: 4,
        };
        let original = State::empty(now);
        let unchanged = original.clone();
        let first = reduce_rate_limit(
            original.clone(),
            RateLimitAttempt {
                account_key: account_key("User@example.com"),
                observed_at: now,
            },
            policy,
        );
        assert_eq!(original, unchanged);
        assert_eq!(first.outcome, RateLimitOutcome::Allowed);

        let second = reduce_rate_limit(
            first.state,
            RateLimitAttempt {
                account_key: account_key(" user@EXAMPLE.com "),
                observed_at: now,
            },
            policy,
        );
        assert_eq!(
            second.outcome,
            RateLimitOutcome::Rejected {
                reason: RateLimitRejection::AccountBudget,
                retry_after_seconds: 60,
            }
        );
    }

    #[tokio::test]
    async fn normalizes_accounts_and_returns_a_retry_after() {
        let limiter = LoginRateLimiter::new(2, 100, Duration::from_secs(60), 4);
        assert!(limiter.check("User@Example.com").await.is_ok());
        assert!(limiter.check(" user@example.com ").await.is_ok());
        assert_eq!(limiter.check("USER@example.com").await, Err(60));
    }

    #[tokio::test]
    async fn key_churn_cannot_evict_a_blocked_account() {
        let limiter = LoginRateLimiter::new(1, 100, Duration::from_secs(60), 2);
        assert!(limiter.check("a@example.com").await.is_ok());
        assert_eq!(limiter.check("a@example.com").await, Err(60));
        assert!(limiter.check("b@example.com").await.is_ok());
        assert_eq!(limiter.check("c@example.com").await, Err(60));
        assert_eq!(limiter.check("a@example.com").await, Err(60));
    }

    #[tokio::test]
    async fn global_budget_limits_account_spraying() {
        let limiter = LoginRateLimiter::new(10, 2, Duration::from_secs(60), 10);
        assert!(limiter.check("a@example.com").await.is_ok());
        assert!(limiter.check("b@example.com").await.is_ok());
        assert_eq!(limiter.check("c@example.com").await, Err(60));
    }
}
