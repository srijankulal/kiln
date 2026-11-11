//! Gamepad-as-mouse controller with acceleration curves and mouse driver abstraction.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Move { dx: i32, dy: i32 },
    LeftDown,
    LeftUp,
    RightDown,
    RightUp,
    Scroll { delta: i32 },
}

pub trait MouseDriver: Send + Sync {
    fn execute(&mut self, action: MouseAction);
}

impl<D: MouseDriver + ?Sized> MouseDriver for Box<D> {
    fn execute(&mut self, action: MouseAction) {
        (**self).execute(action);
    }
}

#[derive(Default)]
pub struct FakeMouseDriver {
    pub actions: Vec<MouseAction>,
    pub cursor_x: i32,
    pub cursor_y: i32,
    pub left_down: bool,
    pub right_down: bool,
}

impl FakeMouseDriver {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MouseDriver for FakeMouseDriver {
    fn execute(&mut self, action: MouseAction) {
        match action {
            MouseAction::Move { dx, dy } => {
                self.cursor_x += dx;
                self.cursor_y += dy;
            }
            MouseAction::LeftDown => self.left_down = true,
            MouseAction::LeftUp => self.left_down = false,
            MouseAction::RightDown => self.right_down = true,
            MouseAction::RightUp => self.right_down = false,
            MouseAction::Scroll { .. } => {}
        }
        self.actions.push(action);
    }
}

#[cfg(windows)]
pub struct Win32MouseDriver;

#[cfg(windows)]
impl Win32MouseDriver {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(windows)]
impl Default for Win32MouseDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
impl MouseDriver for Win32MouseDriver {
    fn execute(&mut self, action: MouseAction) {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
            MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
            MOUSEINPUT,
        };

        let mut mouse_input = MOUSEINPUT::default();

        match action {
            MouseAction::Move { dx, dy } => {
                mouse_input.dx = dx;
                mouse_input.dy = dy;
                mouse_input.dwFlags = MOUSEEVENTF_MOVE;
            }
            MouseAction::LeftDown => {
                mouse_input.dwFlags = MOUSEEVENTF_LEFTDOWN;
            }
            MouseAction::LeftUp => {
                mouse_input.dwFlags = MOUSEEVENTF_LEFTUP;
            }
            MouseAction::RightDown => {
                mouse_input.dwFlags = MOUSEEVENTF_RIGHTDOWN;
            }
            MouseAction::RightUp => {
                mouse_input.dwFlags = MOUSEEVENTF_RIGHTUP;
            }
            MouseAction::Scroll { delta } => {
                mouse_input.mouseData = delta as u32;
                mouse_input.dwFlags = MOUSEEVENTF_WHEEL;
            }
        }

        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: mouse_input },
        };

        unsafe {
            let _ = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }
}

pub struct MouseController<D: MouseDriver> {
    driver: D,
    is_active: bool,
    max_speed_px_per_sec: f32,
    remainder_x: f32,
    remainder_y: f32,
    left_button_down: bool,
    right_button_down: bool,
}

impl<D: MouseDriver> MouseController<D> {
    pub fn new(driver: D) -> Self {
        Self {
            driver,
            is_active: false,
            max_speed_px_per_sec: 1400.0,
            remainder_x: 0.0,
            remainder_y: 0.0,
            left_button_down: false,
            right_button_down: false,
        }
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    pub fn set_active(&mut self, active: bool) {
        if !active {
            // Release any pressed buttons on deactivate
            if self.left_button_down {
                self.driver.execute(MouseAction::LeftUp);
                self.left_button_down = false;
            }
            if self.right_button_down {
                self.driver.execute(MouseAction::RightUp);
                self.right_button_down = false;
            }
        }
        self.is_active = active;
    }

    pub fn toggle_active(&mut self) -> bool {
        self.set_active(!self.is_active);
        self.is_active
    }

    /// Moves the cursor based on analog stick input with non-linear acceleration.
    pub fn update_stick(&mut self, sx: f32, sy: f32, dt_seconds: f32) {
        if !self.is_active {
            return;
        }

        let magnitude = (sx * sx + sy * sy).sqrt();
        if magnitude < 0.001 {
            return;
        }

        // Non-linear acceleration: magnitude^1.5 creates precision for small tilts and rapid travel for full tilt
        let accel = magnitude.powf(1.5);
        let speed = accel * self.max_speed_px_per_sec;

        let total_dx = (sx / magnitude) * speed * dt_seconds + self.remainder_x;
        // Invert Y: gamepads report Y+ up, screen coordinates have Y+ down
        let total_dy = (-sy / magnitude) * speed * dt_seconds + self.remainder_y;

        let int_dx = total_dx.round() as i32;
        let int_dy = total_dy.round() as i32;

        self.remainder_x = total_dx - int_dx as f32;
        self.remainder_y = total_dy - int_dy as f32;

        if int_dx != 0 || int_dy != 0 {
            self.driver.execute(MouseAction::Move {
                dx: int_dx,
                dy: int_dy,
            });
        }
    }

    pub fn set_left_click(&mut self, pressed: bool) {
        if !self.is_active {
            return;
        }
        if pressed && !self.left_button_down {
            self.left_button_down = true;
            self.driver.execute(MouseAction::LeftDown);
        } else if !pressed && self.left_button_down {
            self.left_button_down = false;
            self.driver.execute(MouseAction::LeftUp);
        }
    }

    pub fn set_right_click(&mut self, pressed: bool) {
        if !self.is_active {
            return;
        }
        if pressed && !self.right_button_down {
            self.right_button_down = true;
            self.driver.execute(MouseAction::RightDown);
        } else if !pressed && self.right_button_down {
            self.right_button_down = false;
            self.driver.execute(MouseAction::RightUp);
        }
    }

    pub fn scroll(&mut self, delta: i32) {
        if self.is_active {
            self.driver.execute(MouseAction::Scroll { delta });
        }
    }

    pub fn driver(&self) -> &D {
        &self.driver
    }

    pub fn driver_mut(&mut self) -> &mut D {
        &mut self.driver
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mouse_controller_lifecycle_and_movement() {
        let driver = FakeMouseDriver::new();
        let mut controller = MouseController::new(driver);

        assert!(!controller.is_active());

        // Inputs ignored when not active
        controller.update_stick(1.0, 0.0, 0.016);
        assert_eq!(controller.driver().actions.len(), 0);

        // Activate
        controller.set_active(true);
        assert!(controller.is_active());

        // Full stick right for 16ms (1 frame @ 60fps)
        controller.update_stick(1.0, 0.0, 0.016);
        assert!(controller.driver().cursor_x > 0);
        assert_eq!(controller.driver().cursor_y, 0);

        // Left Click
        controller.set_left_click(true);
        assert!(controller.driver().left_down);
        controller.set_left_click(false);
        assert!(!controller.driver().left_down);

        // Right Click
        controller.set_right_click(true);
        assert!(controller.driver().right_down);

        // Deactivating automatically releases pressed buttons
        controller.set_active(false);
        assert!(!controller.driver().right_down);
    }
}
