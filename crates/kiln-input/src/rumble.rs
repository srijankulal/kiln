//! Controller vibration: short patterns (strong enough to feel: Xbox motors barely move
//! below ~25-40% power or ~60 ms) for a few meaningful moments (play, error,
//! end of a list, favorite, confirm, startup, back to Kiln). Never for plain movement.
//!
//! Any thread can request a pattern (`play`); the controller thread plays it through gilrs'
//! force feedback (`start_effect`), all supporting pads at once. A new pattern replaces the
//! one playing.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

/// 0 off, 1 light, 2 strong.
static LEVEL: AtomicU8 = AtomicU8::new(1);

pub fn set_level(level: i32) {
    LEVEL.store(level.clamp(0, 2) as u8, Ordering::Relaxed);
}

pub fn level() -> i32 {
    LEVEL.load(Ordering::Relaxed) as i32
}

/// One step: heavy (low-frequency) motor, light (high-frequency) motor, both 0..=1, for `ms`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub heavy: f32,
    pub light: f32,
    pub ms: u64,
}

const fn s(heavy: f32, light: f32, ms: u64) -> Step {
    Step { heavy, light, ms }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    /// Play pressed: a soft thump that fades.
    Launch,
    /// Something failed: two quick buzzes.
    Error,
    /// Can't move further: a tiny tick.
    Bump,
    Favorite,
    Unfavorite,
    /// A choice confirmed (power menu, setting sample).
    Confirm,
    /// Startup chime: a gentle swell.
    Boot,
    /// L3 + R3 brought Kiln back: a double tap.
    Return,
}

impl Pattern {
    /// The pattern for a UI sound cue, if that cue gets one.
    pub fn for_cue(cue: &str) -> Option<Pattern> {
        Some(match cue {
            "launch" => Pattern::Launch,
            "error" => Pattern::Error,
            "bump" => Pattern::Bump,
            "favorite" => Pattern::Favorite,
            "unfavorite" => Pattern::Unfavorite,
            "confirm" => Pattern::Confirm,
            "boot" => Pattern::Boot,
            _ => return None,
        })
    }

    pub fn steps(self) -> &'static [Step] {
        match self {
            Pattern::Launch => {
                const P: &[Step] = &[s(0.75, 0.40, 110), s(0.45, 0.15, 150), s(0.22, 0.0, 120)];
                P
            }
            Pattern::Error => {
                const P: &[Step] = &[s(0.30, 0.80, 80), s(0.0, 0.0, 80), s(0.30, 0.80, 80)];
                P
            }
            Pattern::Bump => {
                const P: &[Step] = &[s(0.25, 0.70, 60)];
                P
            }
            Pattern::Favorite => {
                const P: &[Step] = &[s(0.45, 0.75, 80)];
                P
            }
            Pattern::Unfavorite => {
                const P: &[Step] = &[s(0.25, 0.55, 60)];
                P
            }
            Pattern::Confirm => {
                const P: &[Step] = &[s(0.65, 0.35, 100)];
                P
            }
            Pattern::Boot => {
                const P: &[Step] = &[s(0.30, 0.10, 140), s(0.60, 0.25, 180), s(0.30, 0.0, 160)];
                P
            }
            Pattern::Return => {
                const P: &[Step] = &[s(0.70, 0.45, 90), s(0.0, 0.0, 70), s(0.55, 0.35, 80)];
                P
            }
        }
    }
}

/// Motor speeds (0..=65535) for a step at a vibration level.
pub fn motor_speeds(step: Step, level: i32) -> (u16, u16) {
    let scale = match level {
        2 => 1.0,
        1 => 0.75,
        _ => 0.0,
    };
    let to = |v: f32| ((v * scale).clamp(0.0, 1.0) * 65535.0).round() as u16;
    (to(step.heavy), to(step.light))
}

/// The latest requested pattern, waiting for the controller thread to play it.
static PENDING: Mutex<Option<Pattern>> = Mutex::new(None);

/// Requests a pattern at the current level (no-op when off). Returns immediately; the
/// controller thread plays it on its next poll (see `GilrsPoller`). A newer request
/// replaces one that hasn't started yet.
pub fn play(pattern: Pattern) {
    if level() == 0 {
        return;
    }
    if let Ok(mut p) = PENDING.lock() {
        *p = Some(pattern);
    }
}

/// Takes the waiting pattern, if any (called by the controller thread).
pub fn take_pending() -> Option<Pattern> {
    PENDING.lock().ok().and_then(|mut p| p.take())
}

/// Builds and starts a gilrs force-feedback effect for `pattern` on every gamepad that
/// supports it. Vibration must go through gilrs: its force-feedback thread sets each
/// pad's motors every tick, which would immediately cancel motors set any other way.
/// Keep the returned effect alive until it finishes (dropping it stops it).
pub fn start_effect(gilrs: &mut gilrs::Gilrs, pattern: Pattern) -> Option<gilrs::ff::Effect> {
    use gilrs::ff::{BaseEffect, BaseEffectType, EffectBuilder, Repeat, Replay, Ticks};
    let ids: Vec<gilrs::GamepadId> = gilrs
        .gamepads()
        .filter(|(_, g)| g.is_ff_supported())
        .map(|(id, _)| id)
        .collect();
    if ids.is_empty() {
        return None;
    }
    let level = level();
    let total: u64 = pattern.steps().iter().map(|s| s.ms).sum();
    let mut builder = EffectBuilder::new();
    let mut at = 0u64;
    for &step in pattern.steps() {
        let (heavy, light) = motor_speeds(step, level);
        let scheduling = Replay {
            after: Ticks::from_ms(at as u32),
            play_for: Ticks::from_ms(step.ms as u32),
            // Never repeats within the effect's lifetime.
            with_delay: Ticks::from_ms((total + 1000) as u32),
        };
        for (magnitude, kind) in [
            (heavy, BaseEffectType::Strong { magnitude: heavy }),
            (light, BaseEffectType::Weak { magnitude: light }),
        ] {
            if magnitude > 0 {
                builder.add_effect(BaseEffect {
                    kind,
                    scheduling,
                    envelope: Default::default(),
                });
            }
        }
        at += step.ms;
    }
    let effect = builder
        .gamepads(&ids)
        .repeat(Repeat::For(Ticks::from_ms(total as u32)))
        .finish(gilrs)
        .ok()?;
    effect.play().ok()?;
    Some(effect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_scale_motor_speeds() {
        let step = s(1.0, 0.5, 10);
        assert_eq!(motor_speeds(step, 0), (0, 0));
        assert_eq!(motor_speeds(step, 2), (65535, 32768));
        let (h, l) = motor_speeds(step, 1);
        assert!(h > 45000 && h < 52000 && l < h);
    }

    #[test]
    fn only_meaningful_cues_vibrate() {
        assert_eq!(Pattern::for_cue("launch"), Some(Pattern::Launch));
        assert_eq!(Pattern::for_cue("bump"), Some(Pattern::Bump));
        assert_eq!(Pattern::for_cue("move"), None);
        assert_eq!(Pattern::for_cue("tab"), None);
    }

    #[test]
    fn patterns_stay_short() {
        for p in [
            Pattern::Launch,
            Pattern::Error,
            Pattern::Bump,
            Pattern::Favorite,
            Pattern::Unfavorite,
            Pattern::Confirm,
            Pattern::Boot,
            Pattern::Return,
        ] {
            let total: u64 = p.steps().iter().map(|s| s.ms).sum();
            assert!(total <= 480, "{p:?} lasts {total} ms");
            assert!(p.steps().iter().all(|s| s.heavy <= 0.8 && s.light <= 0.8));
        }
    }
}
