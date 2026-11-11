//! Headless UI screenshots with Slint's software renderer (no display, no GPU).
//!
//! Point `KILN_SNAPSHOT_DIR` at a Kiln data folder (the one holding `kiln.db` and
//! `artwork/`), then run:
//!   cargo test -p kiln-shell --test snapshot -- --nocapture
//! PNGs land in `target/screenshots/`. Without the env var the test is a no-op, so CI
//! and plain `cargo test` never depend on a local library.

use kiln_library::db::LibraryDatabase;
use kiln_shell::app::{LaunchApp, LaunchDisplay, MainWindow, Prompts, ShellApp, Theme};
use kiln_shell::sync_selected_game;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, ModelRc, PhysicalSize, VecModel};
use std::path::{Path, PathBuf};
use std::rc::Rc;

struct Headless(Rc<MinimalSoftwareWindow>);

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

/// Minimal PNG writer (stored zlib blocks) so the test needs no extra dependency.
fn save_png(path: &Path, buf: &slint::SharedPixelBuffer<slint::Rgba8Pixel>) {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    }
    let (w, h) = (buf.width(), buf.height());
    let mut raw = Vec::with_capacity((w * 4 + 1) as usize * h as usize);
    for row in buf.as_bytes().chunks((w * 4) as usize) {
        raw.push(0);
        raw.extend(row);
    }
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        z.extend((block.len() as u16).to_le_bytes());
        z.extend((!(block.len() as u16)).to_le_bytes());
        z.extend(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png).expect("write png");
}

#[test]
fn screenshots() {
    let Ok(data_dir) = std::env::var("KILN_SNAPSHOT_DIR") else {
        println!("KILN_SNAPSHOT_DIR not set; skipping UI snapshots");
        return;
    };
    let data_dir = PathBuf::from(data_dir);
    let prefix = std::env::var("KILN_SNAPSHOT_PREFIX").unwrap_or_else(|_| "ui".into());

    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone()))).expect("platform");

    let db = Rc::new(LibraryDatabase::open(&data_dir.join("kiln.db")).expect("open db"));
    let app = ShellApp::new(db, true, true, data_dir.join("artwork"));
    let ui = MainWindow::new().expect("build ui");
    ui.show().expect("show");

    let load = |tab: i32| {
        let (games, recent) = app.load_games(tab).expect("load games");
        ui.set_games(ModelRc::new(VecModel::from(games)));
        ui.set_recent_games(ModelRc::new(VecModel::from(recent)));
        ui.set_selected_tab(tab);
        sync_selected_game(&ui);
    };

    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/screenshots");
    std::fs::create_dir_all(&out).expect("dir");
    let shoot = |name: &str, w: u32, h: u32| {
        window.set_size(PhysicalSize::new(w, h));
        // Let animations settle before capturing.
        for _ in 0..4 {
            slint::platform::update_timers_and_animations();
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
        slint::platform::update_timers_and_animations();
        let img = ui.window().take_snapshot().expect("snapshot");
        let path = out.join(format!("{prefix}-{name}.png"));
        save_png(&path, &img);
        println!("wrote {}", path.display());
    };

    ui.set_clock_time("12:42 PM".into());
    let theme = ui.global::<Theme>();

    // ---------------------------------------------------------------- startup screen
    let app_row = |exe: &str, name: &str, category: &str, processes: i32, close: bool| LaunchApp {
        exe: exe.into(),
        name: name.into(),
        category: category.into(),
        processes,
        close,
    };
    ui.set_launch_apps(ModelRc::new(VecModel::from(vec![
        app_row("discord.exe", "Discord", "Chat", 6, true),
        app_row("chrome.exe", "Google Chrome", "Browser", 24, true),
        app_row("onedrive.exe", "OneDrive", "Cloud sync", 1, false),
        app_row("spotify.exe", "Spotify", "Media", 5, true),
        app_row("winword.exe", "Microsoft Word", "Work", 1, false),
    ])));
    ui.set_launch_closing_count(3);
    let display = |title: &str, detail: &str, external: bool, available: bool| LaunchDisplay {
        title: title.into(),
        detail: detail.into(),
        external,
        available,
    };
    ui.set_launch_displays(ModelRc::new(VecModel::from(vec![
        display("LG TV SSCR2", "External display · 3840 × 2160", true, true),
        display(
            "Laptop screen",
            "Built-in display · 1920 × 1200",
            false,
            true,
        ),
    ])));
    ui.set_launch_display_choice(0);
    ui.set_show_launch(true);
    ui.set_launch_focus(9); // Start button (5 apps + checkbox + 2 displays + Quit)
    shoot("launch-1080p", 1920, 1080);
    shoot("launch-720p", 1280, 720);
    ui.set_launch_focus(7);
    shoot("launch-focus-display", 1920, 1080);
    ui.set_launch_focus(1);
    shoot("launch-focus-app", 1920, 1080);

    ui.set_launch_displays(ModelRc::new(VecModel::from(vec![
        display(
            "TV or external monitor",
            "Connected, but turned off in Windows",
            true,
            false,
        ),
        display(
            "Laptop screen",
            "Built-in display · 1920 × 1200",
            false,
            true,
        ),
    ])));
    ui.set_launch_inactive_external(1);
    ui.set_launch_display_choice(1);
    ui.set_launch_apps(ModelRc::new(VecModel::from(Vec::<LaunchApp>::new())));
    ui.set_launch_closing_count(0);
    ui.set_launch_focus(3);
    shoot("launch-no-tv-no-apps", 1920, 1080);

    ui.set_show_launch(false);

    // ---------------------------------------------------------------- loading screen
    ui.set_boot_status("Closing Discord and Google Chrome…".into());
    ui.set_show_boot(true);
    ui.set_boot_playing(true);
    shoot("boot-working-720p", 1280, 720);
    shoot("boot-working", 1920, 1080);
    ui.set_boot_status("Closed Discord, Google Chrome".into());
    ui.set_boot_done(true);
    shoot("boot-done", 1920, 1080);
    ui.set_show_boot(false);
    ui.set_boot_done(false);

    // ---------------------------------------------------------------- library
    load(0);
    ui.set_focused_card_index(1);
    sync_selected_game(&ui);
    shoot("home-1080p", 1920, 1080);
    shoot("home-720p", 1280, 720);

    ui.set_show_game_detail(true);
    shoot("detail", 1920, 1080);
    ui.set_show_game_detail(false);

    // Game launch screen: poster mid-flight, settled, launcher needs the user, failed.
    // Uses a game with artwork when there is one.
    {
        use slint::Model;
        let games = ui.get_games();
        let pick = (0..games.row_count())
            .find(|&i| {
                games
                    .row_data(i)
                    .is_some_and(|g| g.has_poster && g.has_hero)
            })
            .or_else(|| {
                (0..games.row_count()).find(|&i| games.row_data(i).is_some_and(|g| g.has_poster))
            })
            .unwrap_or(0);
        ui.set_focused_card_index(pick as i32);
        sync_selected_game(&ui);
    }
    shoot("launch-before", 1920, 1080);
    ui.set_game_launch(ui.get_selected_game());
    ui.set_game_launch_phase(0);
    ui.set_game_launch_status("Starting through Steam…".into());
    ui.set_show_game_launch(true);
    shoot("launch-flight", 1920, 1080);
    shoot("launch-mid", 1920, 1080);
    shoot("launch-loading", 1920, 1080);
    shoot("launch-loading-settled", 1920, 1080);
    ui.set_game_launch_phase(1);
    ui.set_game_launch_status(
        "Epic Games needs you before the game can start. Finish there: the stick moves the mouse and A clicks."
            .into(),
    );
    shoot("launch-epic", 1920, 1080);
    ui.set_game_launch_phase(3);
    ui.set_game_launch_status(
        "The game didn't start. Check Steam or Epic Games, then try again.".into(),
    );
    shoot("launch-failed", 1920, 1080);
    shoot("launch-failed-720p", 1280, 720);
    ui.set_show_game_launch(false);

    ui.set_show_power_menu(true);
    ui.set_power_focus(0);
    shoot("power", 1920, 1080);
    ui.set_show_power_menu(false);

    load(2);
    shoot("favorites-empty", 1920, 1080);

    // ---------------------------------------------------------------- settings
    load(6);
    ui.set_settings_focus(5);
    shoot("settings", 1920, 1080);
    ui.set_settings_focus(9);
    shoot("settings-sound", 1920, 1080);
    ui.set_settings_focus(12);
    shoot("settings-vibration", 1920, 1080);
    ui.set_settings_focus(16);
    shoot("settings-scrolled", 1920, 1080);
    ui.set_game_folders(ModelRc::new(VecModel::from(vec![
        slint::SharedString::from(r"D:\Games"),
        slint::SharedString::from(r"E:\Emulators\Standalone PC games with a long folder name"),
    ])));
    ui.set_folders_focus(1);
    ui.set_show_folders(true);
    shoot("folders", 1920, 1080);
    shoot("folders-720p", 1280, 720);
    ui.set_show_folders(false);

    // Themes and accents
    theme.set_theme_preset(1);
    theme.set_accent_preset(5);
    ui.set_settings_focus(4);
    shoot("settings-graphite-blue", 1920, 1080);
    load(0);
    ui.set_focused_card_index(1);
    sync_selected_game(&ui);
    shoot("home-graphite-blue", 1920, 1080);

    theme.set_theme_preset(2);
    theme.set_accent_preset(6);
    ui.global::<Prompts>().set_style(2);
    shoot("home-midnight-teal-controller", 1920, 1080);
    ui.set_show_launch(true);
    ui.set_launch_apps(ModelRc::new(VecModel::from(vec![
        app_row("discord.exe", "Discord", "Chat", 6, true),
        app_row("code.exe", "Visual Studio Code", "Work", 12, false),
    ])));
    ui.set_launch_closing_count(1);
    ui.set_launch_displays(ModelRc::new(VecModel::from(vec![
        display("LG TV SSCR2", "External display · 3840 × 2160", true, true),
        display(
            "Laptop screen",
            "Built-in display · 1920 × 1200",
            false,
            true,
        ),
    ])));
    ui.set_launch_inactive_external(0);
    ui.set_launch_display_choice(1);
    ui.set_launch_focus(5);
    shoot("launch-midnight-teal-controller", 1920, 1080);
    ui.set_show_launch(false);

    // Extreme sizes still fit
    theme.set_theme_preset(0);
    theme.set_accent_preset(0);
    ui.global::<Prompts>().set_style(0);
    theme.set_ui_scale(1.4);
    theme.set_text_scale(1.3);
    ui.set_show_launch(true);
    shoot("launch-max-720p", 1280, 720);
    ui.set_show_launch(false);
}
