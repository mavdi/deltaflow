//! Retry policy configuration.

use std::time::Duration;

/// Policy for retrying failed steps.
#[derive(Debug, Clone)]
pub enum RetryPolicy {
    /// No retries - fail immediately.
    None,

    /// Fixed delay between retries.
    Fixed {
        /// Maximum number of retry attempts.
        max_attempts: u32,
        /// Delay between attempts.
        delay: Duration,
    },

    /// Exponential backoff between retries.
    Exponential {
        /// Maximum number of retry attempts.
        max_attempts: u32,
        /// Initial delay (doubles each attempt).
        initial_delay: Duration,
        /// Maximum delay cap.
        max_delay: Duration,
    },
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::None
    }
}

impl RetryPolicy {
    /// Create an exponential backoff policy with sensible defaults.
    ///
    /// - Initial delay: 1 second
    /// - Max delay: 5 minutes
    pub fn exponential(max_attempts: u32) -> Self {
        Self::Exponential {
            max_attempts,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(300),
        }
    }

    /// Create a fixed delay policy.
    pub fn fixed(max_attempts: u32, delay: Duration) -> Self {
        Self::Fixed { max_attempts, delay }
    }

    /// Calculate the delay for a given attempt number (1-indexed).
    ///
    /// Returns `None` if max attempts exceeded.
    pub fn delay_for_attempt(&self, attempt: u32) -> Option<Duration> {
        match self {
            Self::None => None,
            Self::Fixed { max_attempts, delay } => {
                if attempt <= *max_attempts {
                    Some(*delay)
                } else {
                    None
                }
            }
            Self::Exponential {
                max_attempts,
                initial_delay,
                max_delay,
            } => {
                if attempt <= *max_attempts {
                    // 2^(attempt-1) * initial_delay, capped at max_delay
                    let multiplier = 2u64.saturating_pow(attempt.saturating_sub(1));
                    let delay_ms = initial_delay.as_millis() as u64 * multiplier;
                    let delay = Duration::from_millis(delay_ms.min(max_delay.as_millis() as u64));
                    Some(delay)
                } else {
                    None
                }
            }
        }
    }

    /// Returns the maximum number of attempts allowed.
    pub fn max_attempts(&self) -> u32 {
        match self {
            Self::None => 0,
            Self::Fixed { max_attempts, .. } => *max_attempts,
            Self::Exponential { max_attempts, .. } => *max_attempts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_none_policy() {
        let policy = RetryPolicy::None;
        assert_eq!(policy.delay_for_attempt(1), None);
        assert_eq!(policy.max_attempts(), 0);
    }

    #[test]
    fn test_fixed_policy() {
        let policy = RetryPolicy::fixed(3, Duration::from_secs(5));
        assert_eq!(policy.delay_for_attempt(1), Some(Duration::from_secs(5)));
        assert_eq!(policy.delay_for_attempt(3), Some(Duration::from_secs(5)));
        assert_eq!(policy.delay_for_attempt(4), None);
    }

    #[test]
    fn test_exponential_policy() {
        let policy = RetryPolicy::Exponential {
            max_attempts: 5,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
        };

        assert_eq!(policy.delay_for_attempt(1), Some(Duration::from_secs(1)));
        assert_eq!(policy.delay_for_attempt(2), Some(Duration::from_secs(2)));
        assert_eq!(policy.delay_for_attempt(3), Some(Duration::from_secs(4)));
        assert_eq!(policy.delay_for_attempt(4), Some(Duration::from_secs(8)));
        assert_eq!(policy.delay_for_attempt(5), Some(Duration::from_secs(16)));
        assert_eq!(policy.delay_for_attempt(6), None);
    }

    #[test]
    fn test_exponential_caps_at_max() {
        let policy = RetryPolicy::Exponential {
            max_attempts: 10,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(10),
        };

        // 2^6 = 64 seconds, but capped at 10
        assert_eq!(policy.delay_for_attempt(7), Some(Duration::from_secs(10)));
    }
}
