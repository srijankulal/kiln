//! Emergency abort chord detection (Back + Start held for 5 seconds).

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordEvent {
    None,
    EmergencyAbort,
}

pub struct ChordDetector {
    hold_duration: Duration,
    back_held: bool,
    start_held: bool,
    chord_start: Option<Instant>,
    triggered: bool,
}

impl Default for ChordDetector {
    fn default() -> Self {
        Self::new(Duration::from_secs(5))
    }
}

impl ChordDetector {
    pub fn new(hold_duration: Duration) -> Self {
        Self {
            hold_duration,
            back_held: false,
            start_held: false,
            chord_start: None,
            triggered: false,
        }
    }

    pub fn set_back_held(&mut self, held: bool, now: Instant) -> ChordEvent {
        self.back_held = held;
        self.update_state(now)
    }

    pub fn set_start_held(&mut self, held: bool, now: Instant) -> ChordEvent {
        self.start_held = held;
        self.update_state(now)
    }

    fn update_state(&mut self, now: Instant) -> ChordEvent {
        if self.back_held && self.start_held {
            if self.chord_start.is_none() {
                self.chord_start = Some(now);
                self.triggered = false;
            }
            self.check_elapsed(now)
        } else {
            // Either button released: reset timer and triggered state
            self.chord_start = None;
            self.triggered = false;
            ChordEvent::None
        }
    }

    pub fn check_elapsed(&mut self, now: Instant) -> ChordEvent {
        if let Some(start) = self.chord_start {
            if !self.triggered && now.saturating_duration_since(start) >= self.hold_duration {
                self.triggered = true;
                return ChordEvent::EmergencyAbort;
            }
        }
        ChordEvent::None
    }

    pub fn is_chord_in_progress(&self) -> bool {
        self.back_held && self.start_held && !self.triggered
    }

    pub fn progress_ratio(&self, now: Instant) -> f32 {
        if let Some(start) = self.chord_start {
            let elapsed = now.saturating_duration_since(start).as_secs_f32();
            (elapsed / self.hold_duration.as_secs_f32()).min(1.0)
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chord_emergency_abort_after_5_seconds() {
        let mut detector = ChordDetector::new(Duration::from_secs(5));
        let start = Instant::now();

        // Press Back
        assert_eq!(detector.set_back_held(true, start), ChordEvent::None);
        // Press Start at t=0
        assert_eq!(detector.set_start_held(true, start), ChordEvent::None);
        assert!(detector.is_chord_in_progress());

        // Check at t=4.9s
        let t4_9 = start + Duration::from_millis(4900);
        assert_eq!(detector.check_elapsed(t4_9), ChordEvent::None);
        assert!(detector.progress_ratio(t4_9) > 0.9);

        // Check at t=5.0s -> EmergencyAbort!
        let t5_0 = start + Duration::from_millis(5000);
        assert_eq!(detector.check_elapsed(t5_0), ChordEvent::EmergencyAbort);

        // Check again at t=5.5s -> Should not re-trigger
        let t5_5 = start + Duration::from_millis(5500);
        assert_eq!(detector.check_elapsed(t5_5), ChordEvent::None);

        // Release Back
        assert_eq!(detector.set_back_held(false, t5_5), ChordEvent::None);
        assert!(!detector.is_chord_in_progress());
    }

    #[test]
    fn test_chord_aborted_if_button_released_early() {
        let mut detector = ChordDetector::new(Duration::from_secs(5));
        let start = Instant::now();

        detector.set_back_held(true, start);
        detector.set_start_held(true, start);

        // Release Start at t=3.0s
        let t3 = start + Duration::from_secs(3);
        assert_eq!(detector.set_start_held(false, t3), ChordEvent::None);
        assert!(!detector.is_chord_in_progress());

        // Check at t=6.0s -> Should not trigger because Start was released
        let t6 = start + Duration::from_secs(6);
        assert_eq!(detector.check_elapsed(t6), ChordEvent::None);
    }
}
