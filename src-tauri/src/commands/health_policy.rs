use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const RECOVERY_WINDOW: Duration = Duration::from_secs(600);
const MAX_ATTEMPTS: usize = 3;

#[derive(Default)]
pub(super) struct ThresholdGate {
    since: Option<Instant>,
    last_alert: Option<Instant>,
}

impl ThresholdGate {
    pub fn observe(
        &mut self,
        exceeded: bool,
        now: Instant,
        sustain: Duration,
        cooldown: Duration,
    ) -> bool {
        if !exceeded {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        if now.duration_since(since) < sustain
            || self
                .last_alert
                .is_some_and(|last| now.duration_since(last) < cooldown)
        {
            return false;
        }
        self.last_alert = Some(now);
        true
    }
}

#[derive(Default)]
pub(super) struct RecoveryPolicy {
    generation: Option<u64>,
    attempts: VecDeque<Instant>,
    next_attempt: Option<Instant>,
    pub blocked: bool,
    crashed: bool,
}

impl RecoveryPolicy {
    pub fn observe(
        &mut self,
        generation: u64,
        expected: bool,
        running: bool,
        now: Instant,
        backoff: Duration,
    ) -> bool {
        if self.generation != Some(generation) || !expected {
            *self = Self {
                generation: Some(generation),
                ..Self::default()
            };
        }
        if running || !expected {
            self.crashed = false;
            self.next_attempt = None;
            return false;
        }
        if self.crashed {
            return false;
        }
        self.crashed = true;
        if !self.blocked {
            self.next_attempt = Some(now + backoff);
        }
        true
    }

    pub fn due(&self, now: Instant) -> bool {
        !self.blocked && self.next_attempt.is_some_and(|next| now >= next)
    }

    pub fn attempt_count(&self, now: Instant) -> usize {
        self.attempts
            .iter()
            .filter(|attempt| now.duration_since(**attempt) < RECOVERY_WINDOW)
            .count()
    }

    pub fn next_in_seconds(&self, now: Instant) -> Option<u64> {
        self.next_attempt
            .map(|next| next.saturating_duration_since(now).as_secs())
    }

    pub fn record_attempt(&mut self, now: Instant, backoff: Duration, succeeded: bool) -> bool {
        while self
            .attempts
            .front()
            .is_some_and(|attempt| now.duration_since(*attempt) >= RECOVERY_WINDOW)
        {
            self.attempts.pop_front();
        }
        self.attempts.push_back(now);
        self.next_attempt = if succeeded { None } else { Some(now + backoff) };
        self.crashed = !succeeded;
        if self.attempts.len() >= MAX_ATTEMPTS {
            self.blocked = true;
            self.next_attempt = None;
            return true;
        }
        false
    }

    pub fn disable(&mut self) {
        self.next_attempt = None;
    }

    pub fn resume(&mut self) {
        self.crashed = false;
    }

    pub fn matches_generation(&self, generation: u64) -> bool {
        self.generation == Some(generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_sustained_threshold_and_cooldown() {
        let start = Instant::now();
        let sustain = Duration::from_secs(15);
        let cooldown = Duration::from_secs(60);
        let mut gate = ThresholdGate::default();
        assert!(!gate.observe(true, start, sustain, cooldown));
        assert!(!gate.observe(true, start + Duration::from_secs(10), sustain, cooldown));
        assert!(gate.observe(true, start + sustain, sustain, cooldown));
        assert!(!gate.observe(true, start + Duration::from_secs(30), sustain, cooldown));
        assert!(gate.observe(true, start + Duration::from_secs(75), sustain, cooldown));
    }

    #[test]
    fn interrupted_threshold_must_sustain_again() {
        let now = Instant::now();
        let period = Duration::from_secs(10);
        let mut gate = ThresholdGate::default();
        assert!(!gate.observe(true, now, period, period));
        assert!(!gate.observe(false, now + period, period, period));
        assert!(!gate.observe(true, now + period * 2, period, period));
        assert!(gate.observe(true, now + period * 3, period, period));
    }

    #[test]
    fn three_attempts_are_latched_until_manual_launch() {
        let mut policy = RecoveryPolicy::default();
        let mut now = Instant::now();
        let backoff = Duration::from_secs(30);
        assert!(policy.observe(1, true, false, now, backoff));
        assert!(!policy.due(now));
        for attempt in 0..3 {
            now += backoff;
            assert!(policy.due(now));
            assert_eq!(policy.record_attempt(now, backoff, false), attempt == 2);
        }
        assert!(policy.blocked);
        assert!(!policy.due(now + RECOVERY_WINDOW * 2));
        policy.disable();
        policy.observe(1, true, false, now, backoff);
        assert!(
            policy.blocked,
            "Toggling recovery must not reset the retry limit"
        );
        policy.observe(2, true, true, now, backoff);
        assert!(!policy.blocked);
        assert_eq!(policy.attempt_count(now), 0);
    }

    #[test]
    fn successful_restarts_do_not_reset_retry_budget() {
        let mut policy = RecoveryPolicy::default();
        let now = Instant::now();
        let backoff = Duration::from_secs(10);
        policy.observe(1, true, false, now, backoff);
        policy.record_attempt(now + backoff, backoff, true);
        policy.observe(1, true, true, now + backoff * 2, backoff);
        policy.observe(1, true, false, now + backoff * 3, backoff);
        assert_eq!(policy.attempt_count(now + backoff * 3), 1);
        assert!(policy.due(now + backoff * 4));
    }

    #[test]
    fn manual_stop_disarms_pending_recovery() {
        let mut policy = RecoveryPolicy::default();
        let now = Instant::now();
        let backoff = Duration::from_secs(30);
        policy.observe(1, true, false, now, backoff);
        policy.observe(2, false, false, now, backoff);
        assert!(!policy.due(now + backoff));
        assert_eq!(policy.next_in_seconds(now), None);
    }

    #[test]
    fn attempts_outside_window_expire_before_budget_is_exhausted() {
        let mut policy = RecoveryPolicy::default();
        let now = Instant::now();
        let backoff = Duration::from_secs(30);
        policy.record_attempt(now, backoff, true);
        policy.record_attempt(now + RECOVERY_WINDOW, backoff, true);
        assert_eq!(policy.attempt_count(now + RECOVERY_WINDOW), 1);
        assert!(!policy.blocked);
    }
}
