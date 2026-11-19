//! Email retry schedule. Ported from `app/mail/domain/retry.ts`.

use crate::DomainError;

/// Minutes to wait after the Nth failed attempt (index 0 = first failure).
const BACKOFF_MINUTES: [u32; 4] = [5, 30, 180, 720];

pub const MAX_ATTEMPTS: u32 = BACKOFF_MINUTES.len() as u32 + 1;

/// How long a claimed job is leased before another run may pick it up (crash recovery).
pub const CLAIM_LEASE_MINUTES: u32 = 10;

/// Given the number of attempts made so far (including the one that just failed), returns
/// the delay before the next attempt, or `None` when the job should be given up.
pub fn retry_delay_minutes(attempts: u32) -> Result<Option<u32>, DomainError> {
    if attempts < 1 {
        return Err(DomainError::new("attempts must be a positive integer"));
    }
    Ok(if attempts >= MAX_ATTEMPTS {
        None
    } else {
        Some(BACKOFF_MINUTES[attempts as usize - 1])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backs_off_after_each_failure_and_gives_up_after_the_last_attempt() {
        let delays: Vec<_> = (1..=4).map(|n| retry_delay_minutes(n).unwrap()).collect();
        assert_eq!(delays, [Some(5), Some(30), Some(180), Some(720)]);
        assert_eq!(MAX_ATTEMPTS, 5);
        assert_eq!(retry_delay_minutes(MAX_ATTEMPTS).unwrap(), None);
        assert_eq!(retry_delay_minutes(MAX_ATTEMPTS + 3).unwrap(), None);
    }

    #[test]
    fn rejects_zero_attempts() {
        assert!(retry_delay_minutes(0).is_err());
    }
}
