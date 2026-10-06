//! Push-silence watchdog for Intercom dings.
//!
//! Ring records Intercom dings in the device event history too; a history
//! ding without a matching push ding means the push channel silently dropped
//! it. History dings are never published as live call events (a late
//! "someone is ringing" would mislead); a miss is reported with its time, so
//! Home Assistant can say "someone rang at HH:MM".

use std::collections::{HashMap, VecDeque};

/// A push ding this close to a history ding is the same ring.
pub const MATCH_WINDOW: i64 = 90;
/// Push delivery and history indexing both lag; judge a ding only after this
/// while push has proven itself since the last miss.
pub const GRACE: i64 = 120;
/// Grace without such proof: after a miss or before any push ding.
///
/// Waiting two minutes for a silent push only delays the report. It counts
/// from when the ding was first read, so a late push still matches first.
pub const UNPROVEN_GRACE: i64 = 15;
/// A miss keeps the device degraded at most this long without a newer push ding.
pub const DEGRADED_WINDOW: i64 = 24 * 60 * 60;
/// Older history dings are ignored: a quiet history page can show the same
/// ding for days, long after its ID left the bounded memory below.
const OBSERVE_MAX_AGE: i64 = 60 * 60;
const RETENTION: i64 = 2 * OBSERVE_MAX_AGE;
const MEMORY_LIMIT: usize = 256;

/// One ding read from the Ring event history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryDing {
    pub event_id: String,
    pub occurred_at: i64,
}

/// Whether a history item is an Intercom ding.
///
/// The exact `event_type` Ring uses for Intercom dings in this feed is
/// unconfirmed (unlocks are `Door.Unlock`), so `ding` alone or as the last
/// dotted/underscored segment (`Intercom.Ding`, `intercom_ding`) is
/// accepted, as is `kind=ding`.
#[must_use]
pub fn is_ding_event(event_type: Option<&str>, kind: Option<&str>) -> bool {
    let named_ding = |label: &str| {
        label.eq_ignore_ascii_case("ding")
            || label
                .rsplit_once(['.', '_', ':', '-'])
                .is_some_and(|(prefix, last)| {
                    !prefix.is_empty() && last.eq_ignore_ascii_case("ding")
                })
    };
    event_type.is_some_and(named_ding) || kind.is_some_and(|kind| kind.eq_ignore_ascii_case("ding"))
}

/// Push-health summary served on the device event cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DingHealth {
    pub push_degraded: bool,
    pub last_missed_ding_at: Option<i64>,
}

/// Per-device memory of push dings and history dings awaiting judgement.
#[derive(Debug, Default)]
pub struct DingWatchdog {
    push_dings: VecDeque<i64>,
    newest_push: Option<i64>,
    /// History ding ID to its time, when it was first read, and whether it
    /// was judged.
    history: HashMap<String, (i64, i64, bool)>,
    last_missed: Option<i64>,
}

impl DingWatchdog {
    pub fn record_push(&mut self, occurred_at: i64) {
        self.push_dings.push_back(occurred_at);
        while self.push_dings.len() > MEMORY_LIMIT {
            self.push_dings.pop_front();
        }
        self.newest_push = self.newest_push.max(Some(occurred_at));
    }

    /// Remembers new history dings, then judges those past the grace period.
    /// Returns how many were missed by push.
    pub fn observe<'a>(
        &mut self,
        dings: impl IntoIterator<Item = &'a HistoryDing>,
        now: i64,
    ) -> u64 {
        for ding in dings {
            if ding.occurred_at >= now - OBSERVE_MAX_AGE && self.history.len() < MEMORY_LIMIT {
                self.history
                    .entry(ding.event_id.clone())
                    .or_insert((ding.occurred_at, now, false));
            }
        }
        let proven = self.push_proven();
        let mut missed = 0;
        for (at, seen, judged) in self.history.values_mut() {
            let waited = if proven {
                now - *at >= GRACE
            } else {
                now - *seen >= UNPROVEN_GRACE
            };
            if *judged || !waited {
                continue;
            }
            *judged = true;
            let delivered = self
                .push_dings
                .iter()
                .any(|push| (push - *at).abs() <= MATCH_WINDOW);
            if !delivered {
                missed += 1;
                self.last_missed = self.last_missed.max(Some(*at));
            }
        }
        self.history.retain(|_, (at, _, _)| *at >= now - RETENTION);
        self.push_dings.retain(|at| *at >= now - RETENTION);
        missed
    }

    /// A push ding was seen, and none was missed after it. Push and history
    /// times of one ring can be equal, so equality counts as delivered.
    fn push_proven(&self) -> bool {
        self.newest_push
            .is_some_and(|push| self.last_missed.is_none_or(|missed| push >= missed))
    }

    /// Degraded while a miss from the last day has no newer push ding.
    #[must_use]
    pub fn health(&self, now: i64) -> DingHealth {
        let push_degraded = self.last_missed.is_some_and(|missed| {
            now - missed <= DEGRADED_WINDOW && self.newest_push.is_none_or(|push| push < missed)
        });
        DingHealth {
            push_degraded,
            last_missed_ding_at: self.last_missed,
        }
    }
}

#[cfg(test)]
#[path = "ring_ding_watchdog_tests.rs"]
mod tests;
