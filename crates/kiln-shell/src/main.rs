//! Kiln shell: Fullscreen 10-foot TV UI binary.

pub mod app;
pub mod image_loader;

use app::{MainWindow, ShellApp};
use clap::Parser;
use kiln_input::{GilrsPoller, InputPoller, NavAction};
use kiln_library::db::LibraryDatabase;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

pub fn sync_selected_game(ui: &MainWindow) {
    let idx = ui.get_focused_card_index();
    let item = if idx >= 100 {
        let rel = (idx - 100) as usize;
        ui.get_games().row_data(rel)
    } else {
        ui.get_recent_games().row_data(idx as usize)
    };
    if let Some(game) = item {
        ui.set_selected_game(game);
    } else if ui.get_games().row_count() > 0 {
        if let Some(first) = ui.get_games().row_data(0) {
            ui.set_selected_game(first);
        }
    }
}

pub fn handle_nav_action(action: NavAction, ui: &MainWindow) {
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
        NavAction::Left => {
            if !ui.get_show_game_detail() && !ui.get_show_power_menu() {
                let idx = ui.get_focused_card_index();
                if idx >= 100 {
                    let rel = idx - 100;
                    if rel > 0 {
                        ui.set_focused_card_index(idx - 1);
                    }
                } else if idx > 0 {
                    ui.set_focused_card_index(idx - 1);
                }
                sync_selected_game(ui);
            }
        }
        NavAction::Right => {
            if !ui.get_show_game_detail() && !ui.get_show_power_menu() {
                let idx = ui.get_focused_card_index();
                let games_len = ui.get_games().row_count() as i32;
                let recent_len = ui.get_recent_games().row_count() as i32;
                if idx >= 100 {
                    let rel = idx - 100;
                    if rel + 1 < games_len {
                        ui.set_focused_card_index(idx + 1);
                    }
                } else if idx + 1 < recent_len {
                    ui.set_focused_card_index(idx + 1);
                }
                sync_selected_game(ui);
            }
        }
        NavAction::Up => {
            if !ui.get_show_game_detail() && !ui.get_show_power_menu() {
                let idx = ui.get_focused_card_index();
                if idx >= 100 && ui.get_selected_tab() == 0 && ui.get_recent_games().row_count() > 0
                {
                    let target = std::cmp::min(
                        (idx - 100) as usize,
                        ui.get_recent_games().row_count().saturating_sub(1),
                    );
                    ui.set_focused_card_index(target as i32);
                }
                sync_selected_game(ui);
            }
        }
        NavAction::Down => {
            if !ui.get_show_game_detail() && !ui.get_show_power_menu() {
                let idx = ui.get_focused_card_index();
                if idx < 100 && ui.get_games().row_count() > 0 {
                    let target =
                        std::cmp::min(idx as usize, ui.get_games().row_count().saturating_sub(1));
                    ui.set_focused_card_index(100 + target as i32);
                }
                sync_selected_game(ui);
            }
        }
        NavAction::Select => {
            if ui.get_show_game_detail() {
                let sel = ui.get_selected_game();
                ui.set_show_game_detail(false);
                ui.invoke_play_game(sel.id);
            } else if ui.get_show_power_menu() {
                ui.set_show_power_menu(false);
                let _ = ui.hide();
            } else if ui.get_selected_tab() == 6 {
                let cur = ui.get_trim_tier();
                ui.invoke_set_trim_tier((cur + 1) % 3);
            } else {
                let idx = ui.get_focused_card_index();
                let item = if idx >= 100 {
                    let rel = (idx - 100) as usize;
                    ui.get_games().row_data(rel)
                } else {
                    ui.get_recent_games().row_data(idx as usize)
                };
                if let Some(game) = item {
                    ui.set_selected_game(game);
                    ui.set_show_game_detail(true);
                }
            }
        }
        NavAction::Back => {
            if ui.get_show_game_detail() {
                ui.set_show_game_detail(false);
            } else if ui.get_show_power_menu() {
                ui.set_show_power_menu(false);
            } else {
                ui.set_show_power_menu(true);
            }
        }
        NavAction::ContextMenu => {
            let idx = ui.get_focused_card_index();
            let game_id = if idx >= 100 {
                let rel = (idx - 100) as usize;
                ui.get_games().row_data(rel).map(|g| g.id)
            } else {
                ui.get_recent_games().row_data(idx as usize).map(|g| g.id)
            };
            if let Some(id) = game_id {
                ui.invoke_toggle_favorite(id);
            }
        }
        NavAction::Options => {
            ui.invoke_rescan_library();
        }
        NavAction::ToggleMouseMode => {
            let current = ui.get_status_message();
            if current.starts_with("Mouse Mode") {
                ui.set_status_message(SharedString::from(""));
            } else {
                ui.set_status_message(SharedString::from(
                    "Mouse Mode: ON (Stick moves cursor, A clicks)",
                ));
            }
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "kiln-shell",
    author,
    version,
    about = "Fullscreen 10-foot TV UI for Kiln"
)]
pub struct Opts {
    /// Safe mode: run windowed without display switching or power plan modifications
    #[arg(long)]
    pub safe: bool,

    /// Dry run: simulate all actions and log to console
    #[arg(long)]
    pub dry_run: bool,

    /// Custom path to SQLite library database
    #[arg(long)]
    pub db: Option<PathBuf>,
}

fn get_app_dir() -> PathBuf {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        PathBuf::from(local_app_data).join("Kiln")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/share/Kiln")
    } else {
        PathBuf::from(".")
    }
}

fn get_current_time_str() -> String {
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let hours = (now / 3600) % 24;
    let mins = (now / 60) % 60;
    format!("{:02}:{:02}", hours, mins)
}

fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    let app_dir = get_app_dir();
    let _ = std::fs::create_dir_all(&app_dir);

    let db_path = opts.db.unwrap_or_else(|| app_dir.join("kiln.db"));
    let artwork_dir = app_dir.join("artwork");
    let _ = std::fs::create_dir_all(&artwork_dir);

    println!(
        "Kiln Shell starting (safe={}, dry_run={}, db={})",
        opts.safe,
        opts.dry_run,
        db_path.display()
    );

    let db = Rc::new(LibraryDatabase::open(&db_path)?);
    let app = Rc::new(ShellApp::new(
        db.clone(),
        opts.safe,
        opts.dry_run,
        artwork_dir,
    ));

    // Ensure initial games exist in library
    let existing_games = db.list_games()?;
    if existing_games.is_empty() {
        println!("Initial database is empty, performing initial library scan...");
        let scanned = app.rescan().unwrap_or(0);
        println!("Initial scan completed: {} games added", scanned);

        // If no games found on this system, add demo entries for 10-foot experience
        if scanned == 0 {
            use kiln_core::models::{DiscoveredGame, GameSource, LaunchKind};
            let demo_titles = [
                ("Portal 2", GameSource::Steam, "steam://rungameid/620"),
                ("Hades II", GameSource::Steam, "steam://rungameid/1145350"),
                (
                    "Alan Wake 2",
                    GameSource::Epic,
                    "com.epicgames.launcher://apps/AlanWake2?action=launch",
                ),
                (
                    "Cyberpunk 2077",
                    GameSource::Gog,
                    "C:\\Games\\Cyberpunk 2077\\bin\\x64\\Cyberpunk2077.exe",
                ),
                (
                    "Forza Horizon 5",
                    GameSource::XboxMsStore,
                    "shell:AppsFolder\\Microsoft.624F8B84B80_8wekyb3d8bbwe!ForzaHorizon5",
                ),
                (
                    "Dead Cells",
                    GameSource::Standalone,
                    "C:\\Games\\Dead Cells\\deadcells.exe",
                ),
            ];
            for (title, source, target) in demo_titles {
                let _ = db.upsert_game(&DiscoveredGame {
                    id: format!("demo_{}", title.to_lowercase().replace(' ', "_")),
                    title: title.to_string(),
                    source,
                    install_dir: "C:\\Games".to_string(),
                    exe_path: None,
                    launch_kind: if target.starts_with("C:") {
                        LaunchKind::DirectExe
                    } else {
                        LaunchKind::Uri
                    },
                    launch_target: target.to_string(),
                    launcher: None,
                    artwork_grid: None,
                    artwork_hero: None,
                    last_played: Some(1700000000),
                    playtime_seconds: 3600 * 5,
                    is_hidden: false,
                    is_favorite: title == "Portal 2" || title == "Alan Wake 2",
                    is_manual: false,
                });
            }
        }
    }

    let ui = MainWindow::new()?;

    // Populate Initial View (Tab 0: All)
    let (initial_games, initial_recent) = app.load_games(0)?;
    ui.set_games(ModelRc::new(VecModel::from(initial_games)));
    ui.set_recent_games(ModelRc::new(VecModel::from(initial_recent)));
    sync_selected_game(&ui);
    ui.set_clock_time(SharedString::from(get_current_time_str()));
    ui.set_display_info(SharedString::from("TV: 3840x2160 @ 120Hz [HDR]"));

    // Clock timer (updates time every 10 seconds)
    let clock_timer = slint::Timer::default();
    let ui_weak_clock = ui.as_weak();
    clock_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_secs(10),
        move || {
            if let Some(ui) = ui_weak_clock.upgrade() {
                ui.set_clock_time(SharedString::from(get_current_time_str()));
            }
        },
    );

    // Callbacks
    let app_play = app.clone();
    ui.on_play_game(move |id| {
        if let Err(e) = app_play.play_game(id.as_str()) {
            eprintln!("Failed to play game: {}", e);
        }
    });

    let app_fav = app.clone();
    let ui_weak_fav = ui.as_weak();
    ui.on_toggle_favorite(move |id| {
        let _ = app_fav.toggle_favorite(id.as_str());
        if let Some(ui) = ui_weak_fav.upgrade() {
            let cur_tab = ui.get_selected_tab();
            if let Ok((games, recent)) = app_fav.load_games(cur_tab) {
                ui.set_games(ModelRc::new(VecModel::from(games)));
                ui.set_recent_games(ModelRc::new(VecModel::from(recent)));
            }
        }
    });

    let app_tab = app.clone();
    let ui_weak_tab = ui.as_weak();
    ui.on_change_tab(move |tab| {
        if let Some(ui) = ui_weak_tab.upgrade() {
            if let Ok((games, recent)) = app_tab.load_games(tab) {
                ui.set_games(ModelRc::new(VecModel::from(games)));
                ui.set_recent_games(ModelRc::new(VecModel::from(recent)));
                sync_selected_game(&ui);
            }
        }
    });

    let app_rescan = app.clone();
    let ui_weak_rescan = ui.as_weak();
    ui.on_rescan_library(move || {
        let _ = app_rescan.rescan();
        if let Some(ui) = ui_weak_rescan.upgrade() {
            let cur_tab = ui.get_selected_tab();
            if let Ok((games, recent)) = app_rescan.load_games(cur_tab) {
                ui.set_games(ModelRc::new(VecModel::from(games)));
                ui.set_recent_games(ModelRc::new(VecModel::from(recent)));
            }
        }
    });

    let ui_weak_tier = ui.as_weak();
    ui.on_set_trim_tier(move |tier| {
        if let Some(ui) = ui_weak_tier.upgrade() {
            ui.set_trim_tier(tier);
            println!("Settings: Background trim tier updated to {}", tier);
        }
    });

    let ui_weak_toggle = ui.as_weak();
    ui.on_toggle_close_launcher(move || {
        if let Some(ui) = ui_weak_toggle.upgrade() {
            let current = ui.get_close_launcher_on_exit();
            ui.set_close_launcher_on_exit(!current);
            println!("Settings: Close launcher on exit set to {}", !current);
        }
    });

    let ui_weak_power = ui.as_weak();
    ui.on_power_action(move |action| {
        println!("Power action triggered: {}", action);
        if action == "exit" {
            if let Some(ui) = ui_weak_power.upgrade() {
                let _ = ui.hide();
            }
        }
    });

    // Keyboard & UI Navigation callbacks
    let ui_weak_nav = ui.as_weak();

    ui.on_nav_up({
        let weak = ui_weak_nav.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                handle_nav_action(NavAction::Up, &ui);
            }
        }
    });

    ui.on_nav_down({
        let weak = ui_weak_nav.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                handle_nav_action(NavAction::Down, &ui);
            }
        }
    });

    ui.on_nav_left({
        let weak = ui_weak_nav.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                handle_nav_action(NavAction::Left, &ui);
            }
        }
    });

    ui.on_nav_right({
        let weak = ui_weak_nav.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                handle_nav_action(NavAction::Right, &ui);
            }
        }
    });

    ui.on_nav_select({
        let weak = ui_weak_nav.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                handle_nav_action(NavAction::Select, &ui);
            }
        }
    });

    ui.on_nav_back({
        let weak = ui_weak_nav.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                handle_nav_action(NavAction::Back, &ui);
            }
        }
    });

    ui.on_nav_favorite({
        let weak = ui_weak_nav.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                handle_nav_action(NavAction::ContextMenu, &ui);
            }
        }
    });

    ui.on_nav_next_tab({
        let weak = ui_weak_nav.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                handle_nav_action(NavAction::NextTab, &ui);
            }
        }
    });

    ui.on_nav_prev_tab({
        let weak = ui_weak_nav.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                handle_nav_action(NavAction::PrevTab, &ui);
            }
        }
    });

    // Gamepad background polling thread
    let ui_weak_poll = ui.as_weak();
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    let poller_thread = std::thread::spawn(move || {
        let mut poller = GilrsPoller::new();
        while r.load(Ordering::Relaxed) {
            let now = std::time::Instant::now();
            let actions = poller.poll_actions(now);
            for action in actions {
                if action == NavAction::EmergencyAbort {
                    eprintln!("EMERGENCY ABORT TRIGGERED VIA GAMEPAD CHORD (BACK+START)");
                    std::process::exit(0);
                }
                let weak = ui_weak_poll.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        handle_nav_action(action, &ui);
                    }
                });
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });

    // Fullscreen behavior: if not in safe mode, request fullscreen
    if !opts.safe {
        ui.window().set_fullscreen(true);
    }

    let run_res = ui.run();
    running.store(false, Ordering::Relaxed);
    let _ = poller_thread.join();
    run_res?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opts_parsing() {
        let opts = Opts::parse_from(["kiln-shell", "--safe", "--dry-run"]);
        assert!(opts.safe);
        assert!(opts.dry_run);
        assert_eq!(opts.db, None);
    }

    #[test]
    fn test_clock_format() {
        let time_str = get_current_time_str();
        assert_eq!(time_str.len(), 5);
        assert!(time_str.contains(':'));
    }

    #[test]
    fn test_nav_action_navigation() {
        let Ok(ui) = MainWindow::new() else {
            // In headless CI environments without X11 or Wayland, window creation is gracefully skipped
            return;
        };
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
}
