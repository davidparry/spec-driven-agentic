//! The system clock behind the [`Clock`] port: the one place in the
//! crate that asks what time it is.
//!
//! Every timestamp the harness writes comes through here, so
//! `.spec/state.json` and `.spec/memory.json` cannot end up holding two
//! formats - and `lastActiveAt` stays sortable as plain text, which is
//! how the startup list orders sessions.

use crate::ports::Clock;

#[derive(Debug, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> String {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_clock_reads_as_a_sortable_utc_instant() {
        let now = SystemClock.now();
        assert!(now.ends_with('Z'), "{now}");
        assert_eq!(now.len(), 20, "to the second, no fraction: {now}");
        let parsed = chrono::DateTime::parse_from_rfc3339(&now).expect("RFC 3339");
        assert_eq!(parsed.timezone().local_minus_utc(), 0, "{now}");
        // Text order is time order, which is what the session list and
        // `lastActiveAt` comparisons rely on.
        let later = SystemClock.now();
        assert!(later >= now, "{now} then {later}");
    }
}
