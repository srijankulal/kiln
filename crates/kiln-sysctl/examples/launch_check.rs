//! Prints what Kiln's game-launch helpers see on this PC: launcher install paths, whether
//! they're running, what's in front, and how many process paths are readable.
//! Optional argument: a game install folder, to list the processes Kiln would count as it.
//!
//!   cargo run -p kiln-sysctl --example launch_check -- "C:\...\steamapps\common\Stardew Valley"

use kiln_sysctl::focus;
use kiln_sysctl::launchers::Launcher;

fn main() {
    for l in [Launcher::Steam, Launcher::Epic] {
        println!(
            "{:<11} exe: {:?}  running: {}  pids: {:?}",
            l.name(),
            l.exe_path(),
            l.is_running(),
            l.pids()
        );
    }
    println!("Clock: {}", kiln_sysctl::session::local_clock());
    println!("In front: {:?}", focus::foreground_exe());
    let procs = focus::process_image_paths();
    println!("Readable process paths: {}", procs.len());
    if let Some(dir) = std::env::args().nth(1) {
        let dir_l = dir.to_ascii_lowercase();
        let hits: Vec<_> = procs
            .iter()
            .filter(|(_, p)| p.to_ascii_lowercase().starts_with(&dir_l))
            .collect();
        println!("Processes inside {dir}: {hits:#?}");
    }
}
