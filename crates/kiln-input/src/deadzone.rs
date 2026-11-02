//! Analog stick deadzone filtering and repeat-rate directional navigation.

use crate::NavAction;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct AnalogConfig {
    pub deadzone: f32,
    pub threshold: f32,
    pub initial_repeat_delay: Duration,
    pub repeat_interval: Duration,
}

impl Default for AnalogConfig {
    fn default() -> Self {
        Self {
            deadzone: 0.25,
            threshold: 0.55,
            initial_repeat_delay: Duration::from_millis(350),
            repeat_interval: Duration::from_millis(120),
        }
    }
}

pub struct AnalogProcessor {
    config: AnalogConfig,
    current_direction: Option<NavAction>,
    direction_started_at: Option<Instant>,
    last_repeated_at: Option<Instant>,
}

impl Default for AnalogProcessor {
    fn default() -> Self {
        Self::new(AnalogConfig::default())
    }
}

impl AnalogProcessor {
    pub fn new(config: AnalogConfig) -> Self {
        Self {
            config,
            current_direction: None,
            direction_started_at: None,
            last_repeated_at: None,
        }
    }

    /// Filters raw (x, y) analog stick coordinates using circular deadzone.
    /// Returns filtered (x, y) coordinates with smoothed normalized magnitude.
    pub fn filter_deadzone(&self, raw_x: f32, raw_y: f32) -> (f32, f32) {
        let magnitude = (raw_x * raw_x + raw_y * raw_y).sqrt();
        if magnitude <= self.config.deadzone {
            return (0.0, 0.0);
        }

        let normalized_mag =
            ((magnitude - self.config.deadzone) / (1.0 - self.config.deadzone)).min(1.0);
        let scale = normalized_mag / magnitude;
        (raw_x * scale, raw_y * scale)
    }

    /// Processes filtered stick values and emits NavAction if threshold is exceeded,
    /// respecting initial hold delay and repeat rates.
    pub fn process_stick(&mut self, raw_x: f32, raw_y: f32, now: Instant) -> Option<NavAction> {
        let (x, y) = self.filter_deadzone(raw_x, raw_y);

        let detected = if y > self.config.threshold && y.abs() >= x.abs() {
            Some(NavAction::Up)
        } else if y < -self.config.threshold && y.abs() >= x.abs() {
            Some(NavAction::Down)
        } else if x < -self.config.threshold && x.abs() >= y.abs() {
            Some(NavAction::Left)
        } else if x > self.config.threshold && x.abs() >= y.abs() {
            Some(NavAction::Right)
        } else {
            None
        };

        match detected {
            Some(dir) => {
                if self.current_direction == Some(dir) {
                    // Holding existing direction: check repeat
                    let started = self.direction_started_at.unwrap_or(now);
                    let last_rep = self.last_repeated_at.unwrap_or(started);

                    if now.saturating_duration_since(started) >= self.config.initial_repeat_delay
                        && now.saturating_duration_since(last_rep) >= self.config.repeat_interval
                    {
                        self.last_repeated_at = Some(now);
                        Some(dir)
                    } else {
                        None
                    }
                } else {
                    // New direction tilt
                    self.current_direction = Some(dir);
                    self.direction_started_at = Some(now);
                    self.last_repeated_at = Some(now);
                    Some(dir)
                }
            }
            None => {
                // Returned to neutral / deadzone
                self.current_direction = None;
                self.direction_started_at = None;
                self.last_repeated_at = None;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deadzone_filtering() {
        let processor = AnalogProcessor::new(AnalogConfig::default());

        // Inside deadzone (0.1, 0.1) -> magnitude ~0.14 < 0.25
        let (x, y) = processor.filter_deadzone(0.1, 0.1);
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);

        // Outside deadzone
        let (x, y) = processor.filter_deadzone(0.0, 1.0);
        assert_eq!(x, 0.0);
        assert!((y - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_stick_direction_and_repeat() {
        let mut processor = AnalogProcessor::new(AnalogConfig::default());
        let t0 = Instant::now();

        // 1. Initial push Up
        let act = processor.process_stick(0.0, 0.9, t0);
        assert_eq!(act, Some(NavAction::Up));

        // 2. Immediate next poll at t=100ms should NOT repeat (delay is 350ms)
        let t1 = t0 + Duration::from_millis(100);
        assert_eq!(processor.process_stick(0.0, 0.9, t1), None);

        // 3. Poll at t=360ms (> 350ms initial delay) -> should repeat
        let t2 = t0 + Duration::from_millis(360);
        assert_eq!(processor.process_stick(0.0, 0.9, t2), Some(NavAction::Up));

        // 4. Poll at t=400ms (< 120ms interval from 360ms) -> should NOT repeat
        let t3 = t0 + Duration::from_millis(400);
        assert_eq!(processor.process_stick(0.0, 0.9, t3), None);

        // 5. Poll at t=490ms (> 120ms interval) -> should repeat
        let t4 = t0 + Duration::from_millis(490);
        assert_eq!(processor.process_stick(0.0, 0.9, t4), Some(NavAction::Up));

        // 6. Release to neutral -> reset
        let t5 = t0 + Duration::from_millis(500);
        assert_eq!(processor.process_stick(0.0, 0.0, t5), None);
    }
}
