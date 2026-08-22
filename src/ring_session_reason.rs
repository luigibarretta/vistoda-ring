use std::sync::atomic::{AtomicUsize, Ordering};

use crate::ring_audio::SessionEndReason;

pub fn requested_reason(value: &AtomicUsize) -> Option<SessionEndReason> {
    let raw = value.load(Ordering::Acquire);
    raw.checked_sub(1)
        .and_then(|index| SessionEndReason::ALL.get(index).copied())
        .filter(|reason| reason.is_client())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::requested_reason;
    use crate::ring_audio::SessionEndReason;

    #[test]
    fn only_client_reasons_are_restored() {
        let value = AtomicUsize::new(0);
        assert_eq!(requested_reason(&value), None);
        value.store(SessionEndReason::UserStop as usize + 1, Ordering::Release);
        assert_eq!(requested_reason(&value), Some(SessionEndReason::UserStop));
        value.store(
            SessionEndReason::LifetimeExpired as usize + 1,
            Ordering::Release,
        );
        assert_eq!(requested_reason(&value), None);
    }
}
