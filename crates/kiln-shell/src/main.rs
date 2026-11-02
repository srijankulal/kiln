//! Kiln shell: Fullscreen 10-foot TV UI binary.

pub mod app;
pub mod image_loader;

use app::{MainWindow, ShellApp};
use clap::Parser;
use kiln_library::db::LibraryDatabase;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

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

    // Fullscreen behavior: if not in safe mode, request fullscreen
    if !opts.safe {
        ui.window().set_fullscreen(true);
    }

    ui.run()?;
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
}
