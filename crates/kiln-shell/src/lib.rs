//! Kiln shell library: UI bindings, app controller, and navigation routing.
//! Split from the binary so headless snapshot tests can build the UI.

pub mod app;
pub mod game_watch;
pub mod image_loader;
pub mod launch;
pub mod settings;
pub mod sound;

use app::{MainWindow, Sfx};
use kiln_input::NavAction;
use launch::{focus_item, LaunchFocus};
use slint::{ComponentHandle, Model, SharedString};

/// Tab index of the Settings screen (see `tabs` in ui/main.slint).
pub const SETTINGS_TAB: i32 = 6;

/// Power menu entries, in display order (must match `power-actions` in ui/main.slint).
pub const POWER_ACTIONS: [&str; 4] = ["exit", "sleep", "restart", "shutdown"];

/// Asks the UI to play an interface sound (see `sound.rs` for the cue names and rules).
pub fn sfx(ui: &MainWindow, cue: &str, variant: i32) {
    ui.global::<Sfx>()
        .invoke_play(SharedString::from(cue), variant);
}

/// Sets an index, or bumps when it can't move (already at the edge).
fn step_focus(ui: &MainWindow, current: i32, next: i32, set: impl FnOnce(&MainWindow, i32)) {
    if next == current {
        sfx(ui, "bump", 0);
    } else {
        set(ui, next);
    }
}

fn any_overlay_open(ui: &MainWindow) -> bool {
    ui.get_show_game_detail()
        || ui.get_show_game_launch()
        || ui.get_show_power_menu()
        || ui.get_show_launch()
        || ui.get_show_folders()
}

/// Keyboard / controller handling for the Game folders dialog. Returns true if handled.
/// Focus order: each folder (Select removes it), "Add a folder", "Done".
fn handle_folders_nav(action: NavAction, ui: &MainWindow) -> bool {
    if !ui.get_show_folders() || action == NavAction::EmergencyAbort {
        return false;
    }
    let folders = ui.get_game_folders().row_count() as i32;
    let last = folders + 1;
    let focus = ui.get_folders_focus().clamp(0, last);
    match action {
        NavAction::Up | NavAction::Down => {
            let delta = if action == NavAction::Up { -1 } else { 1 };
            step_focus(ui, focus, (focus + delta).clamp(0, last), |ui, v| {
                ui.set_folders_focus(v)
            });
        }
        NavAction::Select => {
            if focus < folders {
                ui.invoke_remove_game_folder(focus);
            } else if focus == folders {
                ui.invoke_add_game_folder();
            } else {
                ui.set_show_folders(false);
            }
        }
        NavAction::Back => ui.set_show_folders(false),
        _ => sfx(ui, "bump", 0),
    }
    true
}

/// Keyboard / controller handling while the startup screen is up. Returns true if handled.
fn handle_launch_nav(action: NavAction, ui: &MainWindow) -> bool {
    if !ui.get_show_launch() {
        return false;
    }
    if action == NavAction::EmergencyAbort {
        return false;
    }
    if ui.get_launch_busy() || ui.get_show_boot() {
        return true; // ignore input while apps are closing
    }
    let apps = ui.get_launch_apps().row_count();
    let displays = ui.get_launch_displays().row_count();
    let last = launch::start_index(apps, displays);
    let focus = ui.get_launch_focus().clamp(0, last);
    match action {
        NavAction::Up | NavAction::Down => {
            let delta = if action == NavAction::Up { -1 } else { 1 };
            step_focus(ui, focus, (focus + delta).clamp(0, last), |ui, v| {
                ui.set_launch_focus(v)
            });
        }
        NavAction::Left | NavAction::Right => match focus_item(focus, apps, displays) {
            // Left = Close, Right = Keep (matches the order of the two buttons).
            LaunchFocus::App(i) => {
                ui.invoke_launch_set_app_close(i as i32, action == NavAction::Left)
            }
            LaunchFocus::Quit if action == NavAction::Right => ui.set_launch_focus(last),
            LaunchFocus::Start if action == NavAction::Left => ui.set_launch_focus(last - 1),
            _ => sfx(ui, "bump", 0),
        },
        NavAction::Select => match focus_item(focus, apps, displays) {
            LaunchFocus::App(i) => {
                let close = ui
                    .get_launch_apps()
                    .row_data(i)
                    .map(|r| r.close)
                    .unwrap_or(false);
                ui.invoke_launch_set_app_close(i as i32, !close);
            }
            LaunchFocus::Remember => ui.invoke_launch_toggle_remember(),
            LaunchFocus::Display(i) => ui.invoke_launch_choose_display(i as i32),
            LaunchFocus::Quit => ui.invoke_launch_quit(),
            LaunchFocus::Start => ui.invoke_launch_start(),
        },
        NavAction::Back => {
            if ui.get_launch_dismissable() {
                ui.set_show_launch(false);
            } else {
                sfx(ui, "bump", 0); // the first screen can't be skipped
            }
        }
        NavAction::Options => {
            sfx(ui, "select", 0);
            ui.invoke_launch_refresh();
        }
        _ => {}
    }
    true
}

pub fn sync_selected_game(ui: &MainWindow) {
    let idx = ui.get_focused_card_index() as usize;
    if let Some(game) = ui.get_games().row_data(idx) {
        ui.set_selected_game(game);
    } else if ui.get_games().row_count() > 0 {
        if let Some(first) = ui.get_games().row_data(0) {
            ui.set_selected_game(first);
        }
    }
}

pub fn handle_nav_action(action: NavAction, ui: &MainWindow) {
    if handle_launch_nav(action, ui) || handle_folders_nav(action, ui) {
        return;
    }
    // Game launch screen: B hides it (the game keeps starting); A closes it once it failed.
    if ui.get_show_game_launch() && action != NavAction::EmergencyAbort {
        let failed = ui.get_game_launch_phase() == 3;
        if action == NavAction::Back || (failed && action == NavAction::Select) {
            ui.set_show_game_launch(false);
        }
        return;
    }
    if ui.get_show_boot() && action != NavAction::EmergencyAbort {
        return;
    }
    match action {
        NavAction::EmergencyAbort => {
            eprintln!("EMERGENCY ABORT TRIGGERED VIA GAMEPAD CHORD (BACK+START)");
            std::process::exit(0);
        }
        NavAction::NextTab => {
            let next = (ui.get_selected_tab() + 1) % 7;
            ui.set_selected_tab(next);
            ui.set_focused_card_index(0);
            ui.invoke_change_tab(next);
            sync_selected_game(ui);
        }
        NavAction::PrevTab => {
            let prev = (ui.get_selected_tab() + 6) % 7;
            ui.set_selected_tab(prev);
            ui.set_focused_card_index(0);
            ui.invoke_change_tab(prev);
            sync_selected_game(ui);
        }
        NavAction::Left | NavAction::Right => {
            let delta = if action == NavAction::Left { -1 } else { 1 };
            if any_overlay_open(ui) {
                sfx(ui, "bump", 0);
                return;
            }
            if ui.get_selected_tab() == SETTINGS_TAB {
                ui.invoke_adjust_setting(ui.get_settings_focus(), delta);
                return;
            }
            let idx = ui.get_focused_card_index() + delta;
            if idx >= 0 && idx < ui.get_games().row_count() as i32 {
                ui.set_focused_card_index(idx);
                sync_selected_game(ui);
            } else {
                sfx(ui, "bump", 0);
            }
        }
        NavAction::Up | NavAction::Down => {
            let delta = if action == NavAction::Up { -1 } else { 1 };
            if ui.get_show_power_menu() {
                let cur = ui.get_power_focus();
                let next = (cur + delta).clamp(0, POWER_ACTIONS.len() as i32 - 1);
                step_focus(ui, cur, next, |ui, v| ui.set_power_focus(v));
                return;
            }
            if any_overlay_open(ui) {
                sfx(ui, "bump", 0);
                return;
            }
            if ui.get_selected_tab() == SETTINGS_TAB {
                let cur = ui.get_settings_focus();
                let next = (cur + delta).clamp(0, ui.get_settings_count() - 1);
                step_focus(ui, cur, next, |ui, v| ui.set_settings_focus(v));
                return;
            }
            // Single shelf: Up jumps to the first title, Down to the last.
            let games_len = ui.get_games().row_count() as i32;
            if games_len > 0 {
                let cur = ui.get_focused_card_index();
                let next = if delta < 0 { 0 } else { games_len - 1 };
                step_focus(ui, cur, next, |ui, v| {
                    ui.set_focused_card_index(v);
                    sync_selected_game(ui);
                });
            } else {
                sfx(ui, "bump", 0);
            }
        }
        NavAction::Select => {
            if ui.get_show_game_detail() {
                let sel = ui.get_selected_game();
                ui.set_show_game_detail(false);
                ui.invoke_play_game(sel.id);
            } else if ui.get_show_power_menu() {
                let idx = ui
                    .get_power_focus()
                    .clamp(0, POWER_ACTIONS.len() as i32 - 1) as usize;
                ui.set_show_power_menu(false);
                ui.invoke_power_action(SharedString::from(POWER_ACTIONS[idx]));
            } else if ui.get_selected_tab() == SETTINGS_TAB {
                ui.invoke_activate_setting(ui.get_settings_focus());
            } else {
                let idx = ui.get_focused_card_index() as usize;
                if let Some(game) = ui.get_games().row_data(idx) {
                    ui.set_selected_game(game);
                    ui.set_show_game_detail(true);
                } else {
                    sfx(ui, "bump", 0);
                }
            }
        }
        NavAction::Back => {
            if ui.get_show_game_detail() {
                ui.set_show_game_detail(false);
            } else if ui.get_show_power_menu() {
                ui.set_show_power_menu(false);
            } else {
                ui.set_power_focus(0);
                ui.set_show_power_menu(true);
            }
        }
        NavAction::ContextMenu => {
            let idx = ui.get_focused_card_index() as usize;
            if let Some(game) = ui.get_games().row_data(idx) {
                ui.invoke_toggle_favorite(game.id);
            } else {
                sfx(ui, "bump", 0);
            }
        }
        NavAction::Options => {
            sfx(ui, "select", 0);
            ui.invoke_rescan_library();
        }
        NavAction::ToggleMouseMode => {
            let current = ui.get_status_message();
            if current.starts_with("Mouse Mode") {
                sfx(ui, "toggle-off", 0);
                ui.set_status_message(SharedString::from(""));
            } else {
                sfx(ui, "toggle-on", 0);
                ui.set_status_message(SharedString::from(
                    "Mouse Mode: ON (Stick moves cursor, A clicks)",
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::ComponentHandle;

    #[test]
    fn test_nav_action_navigation() {
        let Ok(ui) = MainWindow::new() else {
            // In headless CI environments without X11 or Wayland, window creation is gracefully skipped
            return;
        };
        ui.set_show_launch(false);
        ui.set_selected_tab(0);
        ui.set_focused_card_index(0);

        handle_nav_action(NavAction::NextTab, &ui);
        assert_eq!(ui.get_selected_tab(), 1);

        handle_nav_action(NavAction::PrevTab, &ui);
        assert_eq!(ui.get_selected_tab(), 0);

        handle_nav_action(NavAction::ToggleMouseMode, &ui);
        assert!(ui.get_status_message().contains("Mouse Mode: ON"));

        handle_nav_action(NavAction::ToggleMouseMode, &ui);
        assert_eq!(ui.get_status_message(), "");

        handle_nav_action(NavAction::Back, &ui);
        assert!(ui.get_show_power_menu());

        handle_nav_action(NavAction::Back, &ui);
        assert!(!ui.get_show_power_menu());
    }

    #[test]
    fn test_settings_and_power_navigation() {
        let Ok(ui) = MainWindow::new() else {
            return;
        };
        let theme = ui.global::<app::Theme>();
        ui.set_show_launch(false);

        // Settings: Down moves focus, Right/Left adjust the focused value.
        ui.set_selected_tab(SETTINGS_TAB);
        ui.set_settings_focus(0);
        handle_nav_action(NavAction::Right, &ui);
        assert!((theme.get_ui_scale() - 1.1).abs() < 1e-4);
        handle_nav_action(NavAction::Left, &ui);
        handle_nav_action(NavAction::Left, &ui);
        handle_nav_action(NavAction::Left, &ui);
        assert!(
            (theme.get_ui_scale() - 0.8).abs() < 1e-4,
            "clamped at minimum"
        );
        handle_nav_action(NavAction::Down, &ui);
        assert_eq!(ui.get_settings_focus(), 1);
        for _ in 0..20 {
            handle_nav_action(NavAction::Down, &ui);
        }
        assert_eq!(ui.get_settings_focus(), ui.get_settings_count() - 1);

        // Font, theme and accent presets wrap around in both directions.
        ui.set_settings_focus(6);
        handle_nav_action(NavAction::Left, &ui);
        assert_eq!(theme.get_font_preset(), 2);
        handle_nav_action(NavAction::Right, &ui);
        assert_eq!(theme.get_font_preset(), 0);
        ui.set_settings_focus(5);
        handle_nav_action(NavAction::Left, &ui);
        assert_eq!(theme.get_accent_preset(), 7);
        ui.set_settings_focus(4);
        handle_nav_action(NavAction::Right, &ui);
        assert_eq!(theme.get_theme_preset(), 1);

        // Power menu: Back opens it on "Exit", Down moves through the items and stops at the end.
        ui.set_selected_tab(0);
        handle_nav_action(NavAction::Back, &ui);
        assert!(ui.get_show_power_menu());
        assert_eq!(ui.get_power_focus(), 0);
        for _ in 0..6 {
            handle_nav_action(NavAction::Down, &ui);
        }
        assert_eq!(ui.get_power_focus(), POWER_ACTIONS.len() as i32 - 1);
        handle_nav_action(NavAction::Back, &ui);
        assert!(!ui.get_show_power_menu());
    }

    #[test]
    fn test_startup_screen_navigation() {
        use app::{LaunchApp, LaunchDisplay};
        use slint::{ModelRc, VecModel};
        let Ok(ui) = MainWindow::new() else {
            return;
        };
        let row = |name: &str, close: bool| LaunchApp {
            exe: name.into(),
            name: name.into(),
            category: "Chat".into(),
            processes: 1,
            close,
        };
        ui.set_launch_apps(ModelRc::new(VecModel::from(vec![
            row("a", true),
            row("b", false),
        ])));
        ui.set_launch_displays(ModelRc::new(VecModel::from(vec![LaunchDisplay {
            title: "TV".into(),
            detail: "".into(),
            external: true,
            available: true,
        }])));
        let toggled = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let t = toggled.clone();
        ui.on_launch_set_app_close(move |i, v| t.borrow_mut().push((i, v)));
        let started = std::rc::Rc::new(std::cell::Cell::new(false));
        let st = started.clone();
        ui.on_launch_start(move || st.set(true));

        ui.set_show_launch(true);
        ui.set_launch_focus(0);
        handle_nav_action(NavAction::Right, &ui); // Keep app 0
        handle_nav_action(NavAction::Select, &ui); // toggle app 0 (model still says close)
        assert_eq!(*toggled.borrow(), [(0, false), (0, false)]);

        // Down past apps, checkbox and display lands on Quit, then Start.
        for _ in 0..10 {
            handle_nav_action(NavAction::Down, &ui);
        }
        assert_eq!(ui.get_launch_focus(), launch::start_index(2, 1));
        handle_nav_action(NavAction::Select, &ui);
        assert!(started.get());

        // Back can't skip the first-run screen, but can close it when reopened.
        handle_nav_action(NavAction::Back, &ui);
        assert!(ui.get_show_launch());
        ui.set_launch_dismissable(true);
        handle_nav_action(NavAction::Back, &ui);
        assert!(!ui.get_show_launch());
    }
}
