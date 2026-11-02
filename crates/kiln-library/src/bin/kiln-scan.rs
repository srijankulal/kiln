//! CLI Scanner entrypoint: scans system and reports all detected games without running any launcher.

use kiln_library::scanners::deduplicate_games;
use kiln_library::scanners::epic::EpicScanner;
use kiln_library::scanners::standalone::StandaloneScanner;
use kiln_library::scanners::steam::SteamScanner;
use kiln_library::scanners::GameScanner;
use std::path::PathBuf;

fn main() {
    println!("=== Kiln Game Library Scanner ===");

    let mut all_games = Vec::new();

    // 1. Steam Scanner
    let steam_roots = vec![
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps"),
        PathBuf::from(r"D:\SteamLibrary\steamapps"),
        PathBuf::from(r"E:\SteamLibrary\steamapps"),
        // Test fixture path
        PathBuf::from("crates/kiln-library/fixtures/steam/steamapps"),
    ];
    let steam_scanner = SteamScanner::new(steam_roots);
    if let Ok(mut games) = steam_scanner.scan() {
        println!("  [Steam] Discovered {} title(s)", games.len());
        all_games.append(&mut games);
    }

    // 2. Epic Scanner
    let epic_roots = vec![
        PathBuf::from(r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests"),
        PathBuf::from(r"C:\ProgramData\Epic\UnrealEngineLauncher\Data\Manifests"),
        // Test fixture path
        PathBuf::from("crates/kiln-library/fixtures/epic/Manifests"),
    ];
    let epic_scanner = EpicScanner::new(epic_roots);
    if let Ok(mut games) = epic_scanner.scan() {
        println!("  [Epic] Discovered {} title(s)", games.len());
        all_games.append(&mut games);
    }

    // 3. Standalone Scanner
    let standalone_roots = vec![
        PathBuf::from(r"C:\Games"),
        PathBuf::from(r"D:\Games"),
        PathBuf::from(r"E:\Games"),
        PathBuf::from(r"C:\Program Files\Games"),
        // Test fixture path
        PathBuf::from("crates/kiln-library/fixtures/standalone"),
    ];
    let standalone_scanner = StandaloneScanner::new(standalone_roots);
    if let Ok(mut games) = standalone_scanner.scan() {
        println!(
            "  [Standalone] Discovered {} candidate title(s)",
            games.len()
        );
        all_games.append(&mut games);
    }

    // Deduplicate
    let deduped = deduplicate_games(all_games);
    println!("\nTotal Unique Games Found: {}", deduped.len());
    println!("{:-<75}", "");
    println!(
        "{:<24} | {:<12} | {:<10} | {:<20}",
        "Title", "Source", "Type", "Target"
    );
    println!("{:-<75}", "");

    for game in &deduped {
        let src = format!("{:?}", game.source);
        let kind = format!("{:?}", game.launch_kind);
        let target = if game.launch_target.len() > 25 {
            format!("{}...", &game.launch_target[..22])
        } else {
            game.launch_target.clone()
        };
        println!(
            "{:<24} | {:<12} | {:<10} | {:<20}",
            game.title, src, kind, target
        );
    }
    println!("{:-<75}", "");
}
