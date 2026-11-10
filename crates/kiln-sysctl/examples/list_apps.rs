//! Read-only: prints the apps Kiln's startup screen would offer to close.
//!   cargo run -p kiln-sysctl --example list_apps
use kiln_sysctl::optimizer::GameModeOptimizer;

fn main() {
    for a in GameModeOptimizer::new().scan_running_apps() {
        println!(
            "{:<32} {:<26} {:<10} {} process(es), {}",
            a.name,
            a.exe,
            a.category.label(),
            a.process_count,
            if a.category.closes_by_default() {
                "Close"
            } else {
                "Keep"
            }
        );
    }
}
