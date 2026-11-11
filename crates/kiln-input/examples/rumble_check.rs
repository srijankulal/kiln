//! Lists connected XInput pads and buzzes each one (strong, then light), then plays Kiln's
//! "launch" pattern through gilrs force feedback (the path Kiln itself uses).
//!
//!   cargo run -p kiln-input --example rumble_check

fn main() {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::XboxController::{
            XInputGetState, XInputSetState, XINPUT_STATE, XINPUT_VIBRATION,
        };
        let mut found = false;
        for pad in 0..4u32 {
            let mut state = XINPUT_STATE::default();
            // SAFETY: valid out-param.
            let rc = unsafe { XInputGetState(pad, &mut state) };
            println!("pad {pad}: {}", if rc == 0 { "connected" } else { "empty" });
            if rc != 0 {
                continue;
            }
            found = true;
            for (label, speed) in [("strong", 50000u16), ("light", 22000u16)] {
                let vib = XINPUT_VIBRATION {
                    wLeftMotorSpeed: speed,
                    wRightMotorSpeed: speed,
                };
                // SAFETY: initialised struct, outlives the call.
                let set = unsafe { XInputSetState(pad, &vib) };
                println!("  {label} buzz: XInputSetState -> {set}");
                std::thread::sleep(std::time::Duration::from_millis(500));
                let off = XINPUT_VIBRATION::default();
                // SAFETY: as above.
                unsafe { XInputSetState(pad, &off) };
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
        }
        if !found {
            println!("No XInput controller found.");
        }
    }

    // Kiln's own path: gilrs force feedback.
    let Ok(mut gilrs) = gilrs::Gilrs::new() else {
        println!("gilrs failed to start");
        return;
    };
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_millis(500) {
        while gilrs.next_event().is_some() {}
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    for (id, g) in gilrs.gamepads() {
        println!(
            "gilrs pad {id}: {} (vibration: {})",
            g.name(),
            g.is_ff_supported()
        );
    }
    kiln_input::rumble::set_level(2);
    for pattern in [
        kiln_input::rumble::Pattern::Launch,
        kiln_input::rumble::Pattern::Return,
    ] {
        let effect = kiln_input::rumble::start_effect(&mut gilrs, pattern);
        println!(
            "{pattern:?} via gilrs: {}",
            if effect.is_some() {
                "playing"
            } else {
                "failed"
            }
        );
        let t = std::time::Instant::now();
        while t.elapsed() < std::time::Duration::from_millis(900) {
            while gilrs.next_event().is_some() {}
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        drop(effect);
    }
}
