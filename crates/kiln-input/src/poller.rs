//! Gamepad input polling and navigation event generation.

use crate::chord::{ChordDetector, ChordEvent};
use crate::deadzone::{AnalogConfig, AnalogProcessor};
#[cfg(not(windows))]
use crate::mouse::FakeMouseDriver;
#[cfg(windows)]
use crate::mouse::Win32MouseDriver;
use crate::mouse::{MouseController, MouseDriver};
use crate::NavAction;
use std::time::Instant;

pub trait InputPoller {
    fn poll_actions(&mut self, now: Instant) -> Vec<NavAction>;
}

pub struct GilrsPoller {
    gilrs: Option<gilrs::Gilrs>,
    chord: ChordDetector,
    analog: AnalogProcessor,
    stick_x: f32,
    stick_y: f32,
    mouse: MouseController<Box<dyn MouseDriver>>,
    last_poll: Instant,
}

impl GilrsPoller {
    pub fn new() -> Self {
        let gilrs = gilrs::Gilrs::new().ok();
        #[cfg(windows)]
        let driver: Box<dyn MouseDriver> = Box::new(Win32MouseDriver::new());
        #[cfg(not(windows))]
        let driver: Box<dyn MouseDriver> = Box::new(FakeMouseDriver::new());

        let now = Instant::now();
        Self {
            gilrs,
            chord: ChordDetector::default(),
            analog: AnalogProcessor::new(AnalogConfig::default()),
            stick_x: 0.0,
            stick_y: 0.0,
            mouse: MouseController::new(driver),
            last_poll: now,
        }
    }

    pub fn with_mouse_driver(driver: Box<dyn MouseDriver>) -> Self {
        let gilrs = gilrs::Gilrs::new().ok();
        let now = Instant::now();
        Self {
            gilrs,
            chord: ChordDetector::default(),
            analog: AnalogProcessor::new(AnalogConfig::default()),
            stick_x: 0.0,
            stick_y: 0.0,
            mouse: MouseController::new(driver),
            last_poll: now,
        }
    }

    pub fn is_mouse_mode(&self) -> bool {
        self.mouse.is_active()
    }

    pub fn toggle_mouse_mode(&mut self) -> bool {
        self.mouse.toggle_active()
    }

    pub fn set_mouse_mode(&mut self, active: bool) {
        self.mouse.set_active(active);
    }

    /// Call instead of `poll_actions` while Kiln isn't the window in front (a game is
    /// running): drains controller events without acting on them and releases everything
    /// held, so nothing leaks into the game and nothing fires when Kiln comes back.
    pub fn suspend(&mut self, now: Instant) {
        if let Some(ref mut gilrs) = self.gilrs {
            while gilrs.next_event().is_some() {}
        }
        self.chord.set_back_held(false, now);
        self.chord.set_start_held(false, now);
        self.stick_x = 0.0;
        self.stick_y = 0.0;
        if self.mouse.is_active() {
            self.mouse.set_left_click(false);
            self.mouse.set_right_click(false);
            self.mouse.set_active(false);
        }
        self.last_poll = now;
    }
}

impl Default for GilrsPoller {
    fn default() -> Self {
        Self::new()
    }
}

impl InputPoller for GilrsPoller {
    fn poll_actions(&mut self, now: Instant) -> Vec<NavAction> {
        let dt = now.duration_since(self.last_poll).as_secs_f32();
        self.last_poll = now;

        let mut actions = Vec::new();

        // 1. Check emergency chord timer first
        if let ChordEvent::EmergencyAbort = self.chord.check_elapsed(now) {
            actions.push(NavAction::EmergencyAbort);
        }

        // 2. Poll Gilrs hardware events
        if let Some(ref mut gilrs) = self.gilrs {
            while let Some(event) = gilrs.next_event() {
                use gilrs::{Axis, Button, EventType};

                match event.event {
                    EventType::ButtonPressed(button, _) => match button {
                        Button::Mode => {
                            let _ = self.mouse.toggle_active();
                            actions.push(NavAction::ToggleMouseMode);
                        }
                        Button::South => {
                            if self.mouse.is_active() {
                                self.mouse.set_left_click(true);
                            } else {
                                actions.push(NavAction::Select);
                            }
                        }
                        Button::East => {
                            if self.mouse.is_active() {
                                self.mouse.set_right_click(true);
                            } else {
                                actions.push(NavAction::Back);
                            }
                        }
                        Button::West => {
                            if !self.mouse.is_active() {
                                actions.push(NavAction::ContextMenu);
                            }
                        }
                        Button::North => {
                            if !self.mouse.is_active() {
                                actions.push(NavAction::Options);
                            }
                        }
                        Button::RightTrigger => {
                            if self.mouse.is_active() {
                                self.mouse.scroll(-120);
                            } else {
                                actions.push(NavAction::NextTab);
                            }
                        }
                        Button::LeftTrigger => {
                            if self.mouse.is_active() {
                                self.mouse.scroll(120);
                            } else {
                                actions.push(NavAction::PrevTab);
                            }
                        }
                        Button::DPadUp => {
                            if !self.mouse.is_active() {
                                actions.push(NavAction::Up);
                            }
                        }
                        Button::DPadDown => {
                            if !self.mouse.is_active() {
                                actions.push(NavAction::Down);
                            }
                        }
                        Button::DPadLeft => {
                            if !self.mouse.is_active() {
                                actions.push(NavAction::Left);
                            }
                        }
                        Button::DPadRight => {
                            if !self.mouse.is_active() {
                                actions.push(NavAction::Right);
                            }
                        }
                        Button::Select => {
                            if let ChordEvent::EmergencyAbort = self.chord.set_back_held(true, now)
                            {
                                actions.push(NavAction::EmergencyAbort);
                            }
                        }
                        Button::Start => {
                            if let ChordEvent::EmergencyAbort = self.chord.set_start_held(true, now)
                            {
                                actions.push(NavAction::EmergencyAbort);
                            }
                        }
                        _ => {}
                    },
                    EventType::ButtonReleased(button, _) => match button {
                        Button::South => {
                            if self.mouse.is_active() {
                                self.mouse.set_left_click(false);
                            }
                        }
                        Button::East => {
                            if self.mouse.is_active() {
                                self.mouse.set_right_click(false);
                            }
                        }
                        Button::Select => {
                            self.chord.set_back_held(false, now);
                        }
                        Button::Start => {
                            self.chord.set_start_held(false, now);
                        }
                        _ => {}
                    },
                    EventType::AxisChanged(axis, val, _) => match axis {
                        Axis::LeftStickX => self.stick_x = val,
                        Axis::LeftStickY => self.stick_y = val,
                        _ => {}
                    },
                    _ => {}
                }
            }
        }

        // 3. Process analog stick
        if self.mouse.is_active() {
            self.mouse.update_stick(self.stick_x, self.stick_y, dt);
        } else if let Some(stick_dir) = self.analog.process_stick(self.stick_x, self.stick_y, now) {
            actions.push(stick_dir);
        }

        actions
    }
}

/// Headless fake input poller for unit testing and property-based test suites.
#[derive(Default)]
pub struct FakeInputPoller {
    queued_actions: Vec<NavAction>,
    chord: ChordDetector,
    analog: AnalogProcessor,
    stick_x: f32,
    stick_y: f32,
}

impl FakeInputPoller {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_action(&mut self, action: NavAction) {
        self.queued_actions.push(action);
    }

    pub fn set_buttons(&mut self, back: bool, start: bool, now: Instant) -> Option<NavAction> {
        let e1 = self.chord.set_back_held(back, now);
        let e2 = self.chord.set_start_held(start, now);
        if e1 == ChordEvent::EmergencyAbort || e2 == ChordEvent::EmergencyAbort {
            Some(NavAction::EmergencyAbort)
        } else {
            None
        }
    }

    pub fn set_stick(&mut self, x: f32, y: f32) {
        self.stick_x = x;
        self.stick_y = y;
    }
}

impl InputPoller for FakeInputPoller {
    fn poll_actions(&mut self, now: Instant) -> Vec<NavAction> {
        let mut actions = std::mem::take(&mut self.queued_actions);

        if let ChordEvent::EmergencyAbort = self.chord.check_elapsed(now) {
            actions.push(NavAction::EmergencyAbort);
        }

        if let Some(act) = self.analog.process_stick(self.stick_x, self.stick_y, now) {
            actions.push(act);
        }

        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_fake_input_poller_chords_and_actions() {
        let mut poller = FakeInputPoller::new();
        let t0 = Instant::now();

        poller.push_action(NavAction::Select);
        let res = poller.poll_actions(t0);
        assert_eq!(res, vec![NavAction::Select]);

        // Start emergency chord
        poller.set_buttons(true, true, t0);
        assert_eq!(poller.poll_actions(t0), vec![]);

        // 5 seconds later
        let t5 = t0 + Duration::from_secs(5);
        let res5 = poller.poll_actions(t5);
        assert_eq!(res5, vec![NavAction::EmergencyAbort]);
    }

    #[test]
    fn test_gilrs_poller_mouse_mode_toggle() {
        let mut poller = GilrsPoller::new();
        assert!(!poller.is_mouse_mode());
        poller.toggle_mouse_mode();
        assert!(poller.is_mouse_mode());
        poller.set_mouse_mode(false);
        assert!(!poller.is_mouse_mode());
    }
}
