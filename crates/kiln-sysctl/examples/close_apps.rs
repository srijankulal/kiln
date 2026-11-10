//! Asks the named apps to close exactly as Kiln's startup screen does (never forced).
//!   cargo run -p kiln-sysctl --example close_apps -- notion.exe whatsapp.root.exe
use kiln_sysctl::optimizer::GameModeOptimizer;
use std::time::{Duration, Instant};

fn main() {
    let exes: Vec<String> = std::env::args().skip(1).map(|a| a.to_lowercase()).collect();
    if exes.is_empty() {
        eprintln!("usage: close_apps <exe> [exe...]");
        return;
    }
    if std::env::var_os("KILN_DEBUG").is_some() {
        tracing_subscriber::fmt()
            .with_env_filter("kiln_sysctl=debug")
            .init();
    }
    let t = Instant::now();
    let report = GameModeOptimizer::new().close_apps(&exes, Duration::from_secs(10));
    println!("closed:     {:?}", report.closed);
    println!("still open: {:?}", report.still_open);
    println!("skipped:    {:?}", report.skipped);
    println!("took {:.1}s", t.elapsed().as_secs_f32());
}
