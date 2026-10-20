//! Reconnect and rotation decisions for the push listener. They are free of
//! I/O so the loop behaviour is unit-tested without sockets or clocks.

use std::time::Duration;

pub const MIN_RETRY: Duration = Duration::from_secs(5);
pub const MAX_RETRY: Duration = Duration::from_mins(5);
/// A connection that stayed up this long proves the stored registration and
/// resets the backoff.
pub const STABLE_UPTIME: Duration = Duration::from_mins(1);
/// Consecutive suspect endings before the registration is rotated.
pub const ROTATE_AFTER: u32 = 3;

/// Why one listener attempt ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// The Intercom subscription set changed; reconnect at once.
    Reload,
    /// Ring rejected the stored session; only re-enrollment recovers.
    Reauth,
    /// The server closed or reset an established connection.
    Dropped,
    /// No inbound traffic, not even a heartbeat, within the idle timeout.
    Idle,
    /// Check-in answered but refused the stored device credentials.
    Rejected,
    /// Any other failure: network, Ring API or local state.
    Failed,
}

#[derive(Clone, Copy, Debug)]
pub struct Attempt {
    pub ending: Ending,
    /// How long the MCS connection was up; `None` if it never connected.
    pub uptime: Option<Duration>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub delay: Duration,
    /// Rotate the registration on the next attempt (make-before-break).
    pub rotate: bool,
}

/// Exponential backoff that resets only after a stable connection, plus a
/// count of quick server-side endings that suggest stale credentials.
pub struct ReconnectPolicy {
    delay: Duration,
    suspect: u32,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            delay: MIN_RETRY,
            suspect: 0,
        }
    }
}

impl ReconnectPolicy {
    /// `jitter` in `[0, 1]` lengthens the delay by up to a quarter, so many
    /// bridges recovering from one outage do not reconnect in lockstep.
    pub fn next(&mut self, attempt: Attempt, jitter: f64) -> Plan {
        let stable = attempt.uptime.is_some_and(|uptime| uptime >= STABLE_UPTIME);
        if stable {
            self.delay = MIN_RETRY;
            self.suspect = 0;
        }
        match attempt.ending {
            Ending::Reload => {
                return Plan {
                    delay: Duration::ZERO,
                    rotate: false,
                };
            }
            Ending::Reauth => {
                self.delay = MAX_RETRY;
                return Plan {
                    delay: MAX_RETRY,
                    rotate: false,
                };
            }
            Ending::Dropped if !stable => self.suspect += 1,
            Ending::Rejected => self.suspect += 1,
            Ending::Dropped | Ending::Idle | Ending::Failed => {}
        }
        let rotate = self.suspect >= ROTATE_AFTER;
        if rotate {
            self.suspect = 0;
        }
        let base = self.delay;
        self.delay = base.saturating_mul(2).min(MAX_RETRY);
        Plan {
            delay: base + base.mul_f64(jitter.clamp(0.0, 1.0) / 4.0),
            rotate,
        }
    }
}

/// A uniform value in `[0, 1)` from the operating system's random source.
pub fn jitter() -> f64 {
    let sample = u16::try_from(uuid::Uuid::new_v4().as_u128() % 10_000).unwrap_or_default();
    f64::from(sample) / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::{Attempt, Ending, MAX_RETRY, MIN_RETRY, Plan, ReconnectPolicy, jitter};
    use std::time::Duration;

    const fn quick(ending: Ending) -> Attempt {
        Attempt {
            ending,
            uptime: Some(Duration::from_secs(3)),
        }
    }

    #[test]
    fn a_drop_after_a_stable_connection_reconnects_quickly_without_rotation() {
        let mut policy = ReconnectPolicy::default();
        for _ in 0..10 {
            let plan = policy.next(
                Attempt {
                    ending: Ending::Dropped,
                    uptime: Some(Duration::from_hours(2)),
                },
                0.0,
            );
            assert_eq!(
                plan,
                Plan {
                    delay: MIN_RETRY,
                    rotate: false
                }
            );
        }
    }

    #[test]
    fn repeated_quick_drops_rotate_once_then_count_again() {
        let mut policy = ReconnectPolicy::default();
        let plans: Vec<bool> = (0..6)
            .map(|_| policy.next(quick(Ending::Dropped), 0.0).rotate)
            .collect();
        assert_eq!(plans, [false, false, true, false, false, true]);
        // Check-in rejections are suspect too; network failures are not.
        let mut policy = ReconnectPolicy::default();
        assert!(!policy.next(quick(Ending::Rejected), 0.0).rotate);
        assert!(!policy.next(quick(Ending::Failed), 0.0).rotate);
        assert!(!policy.next(quick(Ending::Idle), 0.0).rotate);
        assert!(!policy.next(quick(Ending::Rejected), 0.0).rotate);
        assert!(policy.next(quick(Ending::Dropped), 0.0).rotate);
    }

    #[test]
    fn backoff_doubles_to_the_cap_and_resets_only_after_a_stable_connection() {
        let mut policy = ReconnectPolicy::default();
        let delays: Vec<u64> = (0..8)
            .map(|_| policy.next(quick(Ending::Failed), 0.0).delay.as_secs())
            .collect();
        assert_eq!(delays, [5, 10, 20, 40, 80, 160, 300, 300]);
        // Connected, but not for long enough: the backoff keeps growing.
        let short = policy.next(
            Attempt {
                ending: Ending::Failed,
                uptime: Some(Duration::from_secs(59)),
            },
            0.0,
        );
        assert_eq!(short.delay, MAX_RETRY);
        let stable = Attempt {
            ending: Ending::Idle,
            uptime: Some(Duration::from_mins(1)),
        };
        assert_eq!(policy.next(stable, 0.0).delay, MIN_RETRY);
        assert_eq!(policy.next(quick(Ending::Failed), 0.0).delay.as_secs(), 10);
    }

    #[test]
    fn reloads_skip_the_delay_and_reauth_waits_the_maximum() {
        let mut policy = ReconnectPolicy::default();
        assert_eq!(
            policy.next(quick(Ending::Reload), 0.9).delay,
            Duration::ZERO
        );
        let reauth = policy.next(quick(Ending::Reauth), 0.9);
        assert_eq!(
            reauth,
            Plan {
                delay: MAX_RETRY,
                rotate: false
            }
        );
        assert_eq!(policy.next(quick(Ending::Failed), 0.0).delay, MAX_RETRY);
    }

    #[test]
    fn jitter_lengthens_by_at_most_a_quarter() {
        let mut policy = ReconnectPolicy::default();
        assert_eq!(
            policy.next(quick(Ending::Failed), 1.0).delay,
            Duration::from_millis(6_250)
        );
        for _ in 0..100 {
            let value = jitter();
            assert!((0.0..1.0).contains(&value));
        }
    }
}
