// Release builds are a GUI app: no console window behind Kiln. Debug builds keep the
// console for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Kiln shell: Fullscreen 10-foot TV UI binary.

use clap::Parser;
use kiln_input::{rumble, GilrsPoller, InputPoller, NavAction};
use kiln_library::db::LibraryDatabase;
use kiln_shell::app::{Haptics, LaunchApp, MainWindow, Prompts, Sfx, ShellApp};
use kiln_shell::game_watch;
use kiln_shell::launch::{self, DisplayTarget};
use kiln_shell::settings::UiSettings;
use kiln_shell::sound::{Cue, SoundEngine};
use kiln_shell::{handle_nav_action, sfx, sync_selected_game};
use kiln_sysctl::launchers::Launcher;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The loading screen's bar fills no earlier than this, in time with the startup chime's
/// final chord (see `render_boot` in sound.rs).
const BOOT_MIN: Duration = Duration::from_millis(1900);
/// How long the filled bar and summary stay up before the loading screen fades out.
const BOOT_HOLD: Duration = Duration::from_millis(900);
/// Loading screen fade-out (matches `fade` in ui/boot.slint).
const BOOT_FADE: Duration = Duration::from_millis(450);
/// Lets a closing sound finish before the window goes away.
const EXIT_SOUND_DELAY: Duration = Duration::from_millis(260);
/// After switching to the chosen screen / full screen, wait for the window to settle before
/// the loading animation and chime start, so the resize never cuts into the animation.
const BOOT_SETTLE: Duration = Duration::from_millis(380);

/// Which kind of window is in front, for the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Front {
    Kiln,
    /// Steam / Epic: the controller can drive the mouse there.
    Launcher,
    /// A game or anything else: the controller is left alone.
    Other,
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
        let p = PathBuf::from(local_app_data).join("Kiln");
        if std::fs::create_dir_all(&p).is_ok() {
            return p;
        }
    }
    if let Ok(snap) = std::env::var("SNAP_USER_DATA") {
        let p = PathBuf::from(snap).join("Kiln");
        if std::fs::create_dir_all(&p).is_ok() {
            return p;
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join(".local/share/Kiln");
        if std::fs::create_dir_all(&p).is_ok() {
            return p;
        }
    }
    let p = PathBuf::from(".kiln");
    let _ = std::fs::create_dir_all(&p);
    p
}

/// Clock text: local time in the user's Windows format (the system clock is UTC).
fn get_current_time_str() -> String {
    kiln_sysctl::session::local_clock()
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
        artwork_dir.clone(),
    ));

    let ui = MainWindow::new()?;

    // Restore interface preferences (size, text, font, theme, ...) before the first frame.
    let settings_path = Rc::new(app_dir.join("settings.json"));
    let settings = Rc::new(RefCell::new(UiSettings::load(&settings_path)));
    settings.borrow().apply(&ui);
    let save_settings = {
        let settings = settings.clone();
        let path = settings_path.clone();
        move || {
            if let Err(e) = settings.borrow().save(&path) {
                eprintln!("Failed to save settings: {}", e);
            }
        }
    };
    let save_settings = Rc::new(save_settings);
    {
        let ui_weak = ui.as_weak();
        let settings = settings.clone();
        let save = save_settings.clone();
        ui.on_settings_changed(move || {
            if let Some(ui) = ui_weak.upgrade() {
                settings.borrow_mut().capture_ui(&ui);
                save();
            }
        });
    }

    // Interface sounds: the UI names a cue, the engine synthesizes and plays it.
    let sound = Rc::new(SoundEngine::start());
    {
        let ui_weak = ui.as_weak();
        let sound = sound.clone();
        ui.global::<Sfx>().on_play(move |name, variant| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let g = ui.global::<Sfx>();
            if !g.get_ready() {
                return;
            }
            // Vibration rides on a few of the same cues, only while the controller is the
            // active input (and independent of the sound setting).
            let prompts = ui.global::<Prompts>();
            if prompts.get_controller_enabled() && prompts.get_last_input_controller() {
                if let Some(p) = rumble::Pattern::for_cue(&name) {
                    rumble::set_level(ui.global::<Haptics>().get_level());
                    rumble::play(p);
                }
            }
            if !g.get_enabled() {
                return;
            }
            if let Some(cue) = Cue::from_name(&name, variant) {
                sound.play(cue, g.get_volume());
            }
        });
    }

    // Vibration setting changed: play a sample at the new level.
    ui.global::<Haptics>().on_preview({
        let ui_weak = ui.as_weak();
        move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            rumble::set_level(ui.global::<Haptics>().get_level());
            rumble::play(rumble::Pattern::Confirm);
        }
    });

    // Library: rescan on every launch so new installs and game-folder changes show up.
    app.set_game_folders(&settings.borrow().game_folders);
    match app.rescan() {
        Ok(n) => println!("Library scan: {} games", n),
        Err(e) => eprintln!("Library scan failed: {e}"),
    }

    // Populate Initial View (Tab 0: All)
    let (initial_games, initial_recent) = app.load_games(0)?;
    ui.set_games(ModelRc::new(VecModel::from(initial_games)));
    ui.set_recent_games(ModelRc::new(VecModel::from(initial_recent)));
    sync_selected_game(&ui);
    ui.set_clock_time(SharedString::from(get_current_time_str()));

    // Artwork for games without any, on a background thread (see app::sync_missing_artwork).
    let art_sync: Rc<dyn Fn()> = {
        let ui_weak = ui.as_weak();
        let settings = settings.clone();
        let db_path = db_path.clone();
        let artwork_dir = artwork_dir.clone();
        Rc::new(move || {
            let ui_weak = ui_weak.clone();
            let key = settings.borrow().sgdb_key();
            let db_path = db_path.clone();
            let artwork_dir = artwork_dir.clone();
            std::thread::spawn(move || {
                let fetched = kiln_shell::app::sync_missing_artwork(&db_path, &artwork_dir, key);
                if fetched > 0 {
                    println!("Artwork: found art for {fetched} game(s)");
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            // Reload the current view from the database (no rescan).
                            ui.invoke_change_tab(ui.get_selected_tab());
                        }
                    });
                }
            });
        })
    };
    art_sync();

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
    let ui_weak_play = ui.as_weak();
    let watching: game_watch::Watching = Default::default();
    let db_path_play = db_path.clone();
    let play_safe = app.is_safe_mode();
    ui.on_play_game(move |id| {
        let Some(ui) = ui_weak_play.upgrade() else {
            return;
        };
        // Launch screen first, so something happens the moment Play is pressed.
        let item = ui.get_selected_game();
        if item.id == id {
            ui.set_game_launch(item.clone());
            ui.set_game_launch_phase(0);
            ui.set_game_launch_status(SharedString::from(game_watch::starting_text(
                item.launcher.as_str(),
            )));
            ui.set_show_game_launch(true);
        }
        match app_play.play_game(id.as_str()) {
            Ok(req) => {
                sfx(&ui, "launch", 0);
                // Track the session: real play time, launcher windows handled, Kiln back
                // in front when the game closes.
                if let Some(req) = req {
                    let weak = ui_weak_play.clone();
                    let raise_weak = ui_weak_play.clone();
                    let raise = move || raise_kiln(raise_weak.clone(), play_safe);
                    let report = move |ev: game_watch::WatchEvent| {
                        let weak = weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                show_watch_event(&ui, &ev);
                            }
                        });
                    };
                    game_watch::spawn(req, db_path_play.clone(), watching.clone(), raise, report);
                } else {
                    // Not tracked (dry run, or no install folder): show the launch screen
                    // briefly, then fade it out.
                    let weak = ui.as_weak();
                    slint::Timer::single_shot(Duration::from_millis(3000), move || {
                        if let Some(ui) = weak.upgrade() {
                            finish_launch_screen(&ui);
                        }
                    });
                }
            }
            Err(e) => {
                eprintln!("Failed to play game: {}", e);
                sfx(&ui, "error", 0);
                ui.set_game_launch_phase(3);
                ui.set_game_launch_status(SharedString::from("Kiln couldn't start this game."));
                ui.set_status_message(SharedString::from("Couldn't start that game"));
            }
        }
    });

    let app_fav = app.clone();
    let ui_weak_fav = ui.as_weak();
    ui.on_toggle_favorite(move |id| {
        let now_favorite = app_fav.toggle_favorite(id.as_str());
        if let Some(ui) = ui_weak_fav.upgrade() {
            match now_favorite {
                Ok(true) => sfx(&ui, "favorite", 0),
                Ok(false) => sfx(&ui, "unfavorite", 0),
                Err(_) => sfx(&ui, "error", 0),
            }
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
    let art_sync_rescan = art_sync.clone();
    ui.on_rescan_library(move || {
        let _ = app_rescan.rescan();
        art_sync_rescan();
        if let Some(ui) = ui_weak_rescan.upgrade() {
            let cur_tab = ui.get_selected_tab();
            if let Ok((games, recent)) = app_rescan.load_games(cur_tab) {
                ui.set_games(ModelRc::new(VecModel::from(games)));
                ui.set_recent_games(ModelRc::new(VecModel::from(recent)));
            }
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
    let power_dry_run = app.is_dry_run();
    ui.on_power_action(move |action| {
        println!("Power action triggered: {}", action);
        let Some(ui) = ui_weak_power.upgrade() else {
            return;
        };
        sfx(&ui, "confirm", 0);
        let weak = ui.as_weak();
        if action == "exit" {
            slint::Timer::single_shot(EXIT_SOUND_DELAY, move || {
                if let Some(ui) = weak.upgrade() {
                    let _ = ui.hide();
                }
            });
            return;
        }
        let Some(power) = kiln_sysctl::session::PowerAction::from_name(&action) else {
            return;
        };
        // Off the UI thread: sleep only returns once the PC wakes up again.
        slint::Timer::single_shot(EXIT_SOUND_DELAY, move || {
            std::thread::spawn(move || {
                if let Err(e) = kiln_sysctl::session::perform(power, power_dry_run) {
                    eprintln!("Power action failed: {e}");
                    let msg = format!("Couldn't {}: {e}", action.replace("shutdown", "shut down"));
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            sfx(&ui, "error", 0);
                            ui.set_status_message(SharedString::from(msg));
                        }
                    });
                }
            });
        });
    });

    // ------------------------------------------------------------------ game folders
    let set_folders = {
        let ui_weak = ui.as_weak();
        let settings = settings.clone();
        let save = save_settings.clone();
        let app = app.clone();
        let art_sync = art_sync.clone();
        Rc::new(move |folders: Vec<String>, status: String| {
            let Some(ui) = ui_weak.upgrade() else { return };
            settings.borrow_mut().game_folders = folders.clone();
            save();
            settings.borrow().apply_game_folders(&ui);
            app.set_game_folders(&folders);
            let _ = app.rescan();
            ui.invoke_change_tab(ui.get_selected_tab());
            art_sync();
            ui.set_status_message(SharedString::from(status));
        })
    };
    {
        let ui_weak = ui.as_weak();
        let settings = settings.clone();
        let set_folders = set_folders.clone();
        ui.on_add_game_folder(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Some(dir) = kiln_sysctl::dialogs::pick_folder("Choose a folder with games") else {
                return; // cancelled
            };
            let dir = dir.to_string_lossy().to_string();
            let mut folders = settings.borrow().game_folders.clone();
            if folders.iter().any(|f| f.eq_ignore_ascii_case(&dir)) {
                sfx(&ui, "bump", 0);
                ui.set_status_message(SharedString::from("That folder is already in the list"));
                return;
            }
            folders.push(dir.clone());
            let found = kiln_library::scanners::standalone::StandaloneScanner::new(vec![
                std::path::PathBuf::from(&dir),
            ]);
            let count = kiln_library::scanners::GameScanner::scan(&found)
                .map(|g| g.len())
                .unwrap_or(0);
            sfx(&ui, "confirm", 0);
            ui.set_folders_focus(folders.len() as i32); // stay on "Add a folder"
            set_folders(
                folders,
                match count {
                    0 => "Added the folder, but found no games in it yet".to_string(),
                    1 => "Added the folder: found 1 game".to_string(),
                    n => format!("Added the folder: found {n} games"),
                },
            );
        });
    }
    {
        let ui_weak = ui.as_weak();
        let settings = settings.clone();
        let set_folders = set_folders.clone();
        ui.on_remove_game_folder(move |i| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let mut folders = settings.borrow().game_folders.clone();
            if (i as usize) >= folders.len() {
                return;
            }
            let removed = folders.remove(i as usize);
            sfx(&ui, "toggle-off", 0);
            ui.set_folders_focus((i).min(folders.len() as i32));
            set_folders(folders, format!("Removed {removed} (nothing was deleted)"));
        });
    }

    // ------------------------------------------------------------------ startup screen
    // Shown first on every launch: pick apps to close and the screen to run on.
    let display_targets: Rc<RefCell<Vec<DisplayTarget>>> = Rc::new(RefCell::new(Vec::new()));
    let launch_rows: Rc<VecModel<LaunchApp>> = Rc::new(VecModel::default());
    ui.set_launch_apps(ModelRc::from(launch_rows.clone()));

    let refresh_launch = {
        let ui_weak = ui.as_weak();
        let settings = settings.clone();
        let display_targets = display_targets.clone();
        let launch_rows = launch_rows.clone();
        move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            // Keep the user's Close/Keep choices across a refresh.
            let previous: HashMap<String, bool> = launch_rows
                .iter()
                .map(|r| (r.exe.to_string(), r.close))
                .collect();
            let apps = kiln_sysctl::optimizer::GameModeOptimizer::new().scan_running_apps();
            let rows = launch::app_rows(&apps, &settings.borrow().always_keep, &previous);
            launch_rows.set_vec(rows);

            let scan = kiln_sysctl::monitors::scan_displays();
            let targets = launch::display_targets(&scan);
            let choice =
                launch::default_display_choice(&targets, &settings.borrow().preferred_display);
            ui.set_launch_displays(ModelRc::new(VecModel::from(
                targets.iter().map(|t| t.to_row()).collect::<Vec<_>>(),
            )));
            ui.set_launch_display_choice(choice as i32);
            ui.set_launch_inactive_external(scan.inactive_external as i32);
            ui.set_launch_closing_count(launch_rows.iter().filter(|r| r.close).count() as i32);
            ui.set_launch_focus(launch::start_index(launch_rows.row_count(), targets.len()));
            *display_targets.borrow_mut() = targets;
        }
    };
    let refresh_launch = Rc::new(refresh_launch);

    {
        let refresh = refresh_launch.clone();
        ui.on_launch_refresh(move || refresh());
    }
    {
        let ui_weak = ui.as_weak();
        let refresh = refresh_launch.clone();
        ui.on_open_launch(move || {
            if let Some(ui) = ui_weak.upgrade() {
                refresh();
                ui.set_launch_dismissable(true);
                ui.set_show_launch(true);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let launch_rows = launch_rows.clone();
        ui.on_launch_set_app_close(move |i, close| {
            let Some(ui) = ui_weak.upgrade() else { return };
            if let Some(mut row) = launch_rows.row_data(i as usize) {
                if row.close == close {
                    sfx(&ui, "bump", 0);
                    return;
                }
                // Keep sounds like "on", Close like "off".
                sfx(&ui, if close { "toggle-off" } else { "toggle-on" }, 0);
                row.close = close;
                launch_rows.set_row_data(i as usize, row);
            }
            ui.set_launch_closing_count(launch_rows.iter().filter(|r| r.close).count() as i32);
        });
    }
    {
        let ui_weak = ui.as_weak();
        let display_targets = display_targets.clone();
        ui.on_launch_choose_display(move |i| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let available = display_targets
                .borrow()
                .get(i as usize)
                .is_some_and(|t| t.monitor.is_some());
            if available {
                sfx(&ui, "select", 0);
                ui.set_launch_display_choice(i);
            } else {
                sfx(&ui, "bump", 0);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_launch_toggle_remember(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let on = !ui.get_launch_remember();
                sfx(&ui, if on { "toggle-on" } else { "toggle-off" }, 0);
                ui.set_launch_remember(on);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        ui.on_launch_quit(move || {
            if let Some(ui) = ui_weak.upgrade() {
                sfx(&ui, "back", 0);
            }
            slint::Timer::single_shot(EXIT_SOUND_DELAY, || {
                let _ = slint::quit_event_loop();
            });
        });
    }
    {
        let ui_weak = ui.as_weak();
        let settings = settings.clone();
        let save = save_settings.clone();
        ui.on_clear_kept_apps(move || {
            settings.borrow_mut().always_keep.clear();
            save();
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_kept_apps_count(0);
                ui.set_status_message(SharedString::from("Always-kept apps cleared"));
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let settings = settings.clone();
        let save = save_settings.clone();
        let display_targets = display_targets.clone();
        let launch_rows = launch_rows.clone();
        let is_safe = app.is_safe_mode();
        let is_dry = app.is_dry_run();
        let app_launch = app.clone();
        ui.on_launch_start(move || {
            let launchers = app_launch.launchers_in_library();
            let Some(ui) = ui_weak.upgrade() else { return };
            let rows: Vec<LaunchApp> = launch_rows.iter().collect();
            let to_close: Vec<String> = rows
                .iter()
                .filter(|r| r.close)
                .map(|r| r.exe.to_string())
                .collect();
            let target = display_targets
                .borrow()
                .get(ui.get_launch_display_choice() as usize)
                .cloned();

            // Remember choices: always-keep list and preferred screen.
            {
                let mut s = settings.borrow_mut();
                s.always_keep =
                    launch::update_always_keep(&s.always_keep, &rows, ui.get_launch_remember());
                if let Some(t) = &target {
                    s.preferred_display = if t.external { "external" } else { "internal" }.into();
                }
                ui.set_kept_apps_count(s.always_keep.len() as i32);
            }
            save();

            // Loading screen + startup chime, on the chosen screen, while apps close.
            let names: Vec<String> = rows
                .iter()
                .filter(|r| r.close)
                .map(|r| r.name.to_string())
                .collect();
            let names_by_exe: HashMap<String, String> = rows
                .iter()
                .map(|r| (r.exe.to_string(), r.name.to_string()))
                .collect();
            ui.set_boot_status(SharedString::from(launch::closing_status(&names)));
            ui.set_boot_done(false);
            ui.set_boot_leaving(false);
            ui.set_boot_playing(false);
            ui.set_show_boot(true);
            ui.set_show_launch(false);
            // Move to the chosen screen behind a plain backdrop first...
            if let Some(m) = target.and_then(|t| t.monitor) {
                place_window(&ui, &m, is_safe);
            }

            // ...then, once the window has settled, start the animation, chime and closing.
            let ui_weak = ui.as_weak();
            slint::Timer::single_shot(BOOT_SETTLE, move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                ui.set_boot_playing(true);
                sfx(&ui, "boot", 0);
                let started = Instant::now();
                let ui_weak = ui.as_weak();
                std::thread::spawn(move || {
                    let report = if to_close.is_empty() {
                        kiln_sysctl::optimizer::CloseReport::default()
                    } else if is_dry {
                        println!(
                            "[DRY-RUN] Would ask these apps to close: {}",
                            to_close.join(", ")
                        );
                        std::thread::sleep(std::time::Duration::from_millis(700));
                        kiln_sysctl::optimizer::CloseReport {
                            closed: to_close
                                .iter()
                                .map(|e| names_by_exe.get(e).cloned().unwrap_or_else(|| e.clone()))
                                .collect(),
                            ..Default::default()
                        }
                    } else {
                        kiln_sysctl::optimizer::GameModeOptimizer::new()
                            .close_apps(&to_close, std::time::Duration::from_secs(10))
                    };
                    let summary = launch::summarize(&report, is_dry);
                    println!("Startup: {}", summary);

                    // Start Steam / Epic quietly now, so they're signed in and ready by the
                    // time a game is picked (no sign-in wait or launcher window on Play).
                    for l in launchers {
                        if is_dry {
                            println!("[DRY-RUN] Would start {} in the background", l.name());
                            continue;
                        }
                        match l.start_silent(None) {
                            Ok(true) => println!("Started {} in the background", l.name()),
                            Ok(false) => {}
                            Err(e) => println!("Didn't start {}: {e}", l.name()),
                        }
                    }

                    let _ = slint::invoke_from_event_loop(move || {
                        let wait = BOOT_MIN.saturating_sub(started.elapsed());
                        slint::Timer::single_shot(wait, move || {
                            let Some(ui) = ui_weak.upgrade() else { return };
                            ui.set_boot_status(SharedString::from(summary.clone()));
                            ui.set_boot_done(true);
                            let weak = ui.as_weak();
                            slint::Timer::single_shot(BOOT_HOLD, move || {
                                let Some(ui) = weak.upgrade() else { return };
                                ui.set_boot_leaving(true);
                                let weak = ui.as_weak();
                                slint::Timer::single_shot(BOOT_FADE, move || {
                                    let Some(ui) = weak.upgrade() else { return };
                                    ui.set_show_boot(false);
                                    ui.set_status_message(SharedString::from(summary));
                                });
                            });
                        });
                    });
                });
            });
        });
    }

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

    let poll_safe = app.is_safe_mode();
    let poller_thread = std::thread::spawn(move || {
        let mut poller = GilrsPoller::new();
        let self_pid = std::process::id();
        let mut front = Front::Kiln;
        // Foreground lookups are cached per PID (the exe of a PID doesn't change).
        let mut fg_cache: Option<(u32, Front)> = None;
        while r.load(Ordering::Relaxed) {
            let now = std::time::Instant::now();
            let now_front = match kiln_sysctl::focus::foreground_pid() {
                Some(pid) if pid == self_pid => Front::Kiln,
                Some(pid) => match fg_cache {
                    Some((p, f)) if p == pid => f,
                    _ => {
                        let f = match kiln_sysctl::focus::foreground_exe() {
                            Some((_, exe)) if Launcher::from_exe(&exe).is_some() => Front::Launcher,
                            _ => Front::Other,
                        };
                        fg_cache = Some((pid, f));
                        f
                    }
                },
                None => Front::Other,
            };
            if now_front != front {
                // Release everything held so nothing carries over between windows.
                poller.suspend(now);
                front = now_front;
            }
            if front != Front::Kiln {
                // A game or launcher is in front: Kiln ignores the controller, except for
                // "hold both sticks in" to come back, and the stick-as-mouse over a launcher
                // (Steam / Epic sign-in, prompts) so no keyboard or mouse is needed.
                if poller.poll_away(now, front == Front::Launcher) {
                    println!("Controller: returning to Kiln");
                    rumble::play(rumble::Pattern::Return);
                    raise_kiln(ui_weak_poll.clone(), poll_safe);
                    poller.suspend(now);
                }
                std::thread::sleep(std::time::Duration::from_millis(16));
                continue;
            }
            let actions = poller.poll_actions(now);
            for action in actions {
                if action == NavAction::EmergencyAbort {
                    eprintln!("EMERGENCY ABORT TRIGGERED VIA GAMEPAD CHORD (BACK+START)");
                    std::process::exit(0);
                }
                let weak = ui_weak_poll.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        let prompts = ui.global::<Prompts>();
                        if !prompts.get_controller_enabled() {
                            return; // Settings > Controller is off (emergency chord still works)
                        }
                        prompts.set_last_input_controller(true);
                        handle_nav_action(action, &ui);
                    }
                });
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    });

    // The startup screen comes first; Kiln goes full screen only after the user presses Start.
    ui.set_show_launch(true);
    ui.set_launch_dismissable(false);
    refresh_launch();
    // Sounds start once the first screen has settled, so initial state changes are silent.
    {
        let ui_weak = ui.as_weak();
        slint::Timer::single_shot(Duration::from_millis(400), move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.global::<Sfx>().set_ready(true);
            }
        });
    }

    let run_res = ui.run();
    running.store(false, Ordering::Relaxed);
    let _ = poller_thread.join();
    run_res?;
    Ok(())
}

/// Updates the launch screen and footer for a game watcher event (UI thread).
fn show_watch_event(ui: &MainWindow, ev: &game_watch::WatchEvent) {
    use game_watch::WatchEvent;
    ui.set_status_message(SharedString::from(game_watch::status_text(ev)));
    let launching = ui.get_show_game_launch();
    match ev {
        WatchEvent::Starting { .. } => {}
        WatchEvent::LauncherNeedsYou { .. } | WatchEvent::NotStarted { .. } => {
            if launching {
                let failed = matches!(ev, WatchEvent::NotStarted { .. });
                ui.set_game_launch_phase(if failed { 3 } else { 1 });
                ui.set_game_launch_status(SharedString::from(game_watch::launch_screen_text(ev)));
                if failed {
                    sfx(ui, "error", 0);
                }
            }
        }
        WatchEvent::Playing { .. } => {
            if launching {
                finish_launch_screen(ui);
            }
        }
        WatchEvent::Ended { .. } => {
            ui.set_show_game_launch(false);
            // Reload the view so the new play time shows.
            ui.invoke_change_tab(ui.get_selected_tab());
        }
    }
}

/// Game is up: the launch screen plays its exit (poster pushes forward, fades), then goes.
fn finish_launch_screen(ui: &MainWindow) {
    if !ui.get_show_game_launch() || ui.get_game_launch_phase() >= 2 {
        return;
    }
    ui.set_game_launch_phase(2);
    ui.set_game_launch_status(SharedString::from("Have fun!"));
    let weak = ui.as_weak();
    slint::Timer::single_shot(Duration::from_millis(760), move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_game_launch_phase() == 2 {
                ui.set_show_game_launch(false);
            }
        }
    });
}

/// Brings Kiln back in front (callable from any thread). If Windows minimized Kiln or
/// dropped it back to its normal window size while a game or launcher was in front, Kiln
/// goes full screen again (not on the startup screen, which is meant to be windowed).
fn raise_kiln(weak: slint::Weak<MainWindow>, is_safe: bool) {
    let pid = std::process::id();
    let needs_fix = !is_safe && kiln_sysctl::focus::process_window_needs_fullscreen(pid);
    kiln_sysctl::focus::bring_process_to_front(pid);
    if needs_fix {
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            if ui.get_show_launch() {
                return;
            }
            println!("Restoring full screen");
            let w = ui.window();
            w.set_minimized(false);
            w.set_fullscreen(false);
            w.set_fullscreen(true);
        });
    }
}

/// Moves Kiln to the chosen screen, then goes full screen there. This only places Kiln's
/// own window; Windows display settings are untouched.
fn place_window(ui: &MainWindow, m: &kiln_sysctl::monitors::MonitorInfo, is_safe: bool) {
    let window = ui.window();
    if window.is_fullscreen() {
        window.set_fullscreen(false);
    }
    window.set_position(slint::PhysicalPosition::new(m.x + 40, m.y + 40));
    if is_safe {
        println!(
            "[SAFE] Staying windowed on {} ({}x{})",
            m.name, m.width, m.height
        );
    } else {
        window.set_fullscreen(true);
    }
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
        // "15:42", "3:42 PM", "03:42 PM"...
        assert!((4..=12).contains(&time_str.chars().count()), "{time_str}");
        assert!(time_str.chars().any(|c| c.is_ascii_digit()));
    }
}
