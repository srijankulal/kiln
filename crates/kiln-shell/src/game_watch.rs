//! Watches a game Kiln just launched, on a background thread:
//! - while the game is starting, keeps Steam / Epic windows from covering Kiln (for a while;
//!   after that the launcher probably needs the user, so it's shown instead);
//! - once the game appears, makes sure it is the window in front;
//! - when it closes, records the real play time and brings Kiln back to the front.

use kiln_library::db::LibraryDatabase;
use kiln_library::tracker::{game_pids, path_is_inside, SessionTracker, Step, TrackerConfig};
use kiln_sysctl::focus;
use kiln_sysctl::launchers::Launcher;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const POLL: Duration = Duration::from_secs(1);

/// Everything the watcher needs about the launched game.
#[derive(Debug, Clone)]
pub struct WatchRequest {
    pub game_id: String,
    pub title: String,
    pub install_dir: String,
    /// The store launcher that starts this game, if any.
    pub launcher: Option<Launcher>,
}

impl WatchRequest {
    /// Only games with a real install folder can be tracked (their processes are found by
    /// exe location).
    pub fn is_trackable(&self) -> bool {
        let dir = self.install_dir.trim_end_matches(['\\', '/']);
        path_is_inside(&format!("{dir}\\game.exe"), dir)
    }
}

/// Progress reports for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    Starting {
        title: String,
    },
    /// The launcher has been busy for a while; it's shown so the user can deal with it.
    LauncherNeedsYou {
        title: String,
        launcher: &'static str,
    },
    Playing {
        title: String,
    },
    Ended {
        title: String,
        played_seconds: u64,
    },
    NotStarted {
        title: String,
    },
}

/// Game ids currently being watched (so pressing Play twice doesn't double-count).
pub type Watching = Arc<Mutex<HashSet<String>>>;

/// Starts watching on a background thread. Returns false if this game is already watched.
pub fn spawn(
    req: WatchRequest,
    db_path: PathBuf,
    watching: Watching,
    raise_kiln: impl Fn() + Send + 'static,
    report: impl Fn(WatchEvent) + Send + 'static,
) -> bool {
    if !watching.lock().unwrap().insert(req.game_id.clone()) {
        return false;
    }
    std::thread::spawn(move || {
        run(&req, &db_path, &raise_kiln, &report);
        watching.lock().unwrap().remove(&req.game_id);
    });
    true
}

/// How long a launcher's windows are kept behind Kiln while its game starts. Steam's main
/// window is never needed to start a game, so it's hidden for a while (after that a dialog
/// probably needs the user). Epic only shows a window when it needs the user (sign-in,
/// update, terms), so it is never hidden.
fn hide_launcher_for(launcher: Option<Launcher>) -> Duration {
    match launcher {
        Some(Launcher::Steam) => Duration::from_secs(45),
        _ => Duration::ZERO,
    }
}

fn run(
    req: &WatchRequest,
    db_path: &std::path::Path,
    raise_kiln: &dyn Fn(),
    report: &dyn Fn(WatchEvent),
) {
    let self_pid = std::process::id();
    let title = req.title.clone();
    let config = TrackerConfig {
        hide_launcher_for: hide_launcher_for(req.launcher),
        ..TrackerConfig::default()
    };
    let mut tracker = SessionTracker::new(Instant::now(), config);
    let mut last: Option<Step> = None;
    let mut launcher_shown = false;
    report(WatchEvent::Starting {
        title: title.clone(),
    });

    loop {
        std::thread::sleep(POLL);
        let procs = focus::process_image_paths();
        let pids: Vec<u32> = game_pids(&req.install_dir, &procs)
            .into_iter()
            .filter(|&p| p != self_pid)
            .collect();
        let step = tracker.step(Instant::now(), !pids.is_empty());

        match step {
            Step::Waiting {
                hide_launcher: true,
            } => {
                // A launcher window jumped in front (cold start, sign-in splash): tuck it
                // away and keep Kiln's "Starting…" screen up.
                if let Some((_, exe)) = focus::foreground_exe() {
                    if Launcher::from_exe(&exe) == Some(Launcher::Steam) {
                        focus::set_windows_minimized(&Launcher::Steam.pids(), true);
                        raise_kiln();
                    }
                }
            }
            Step::Waiting {
                hide_launcher: false,
            } => {
                // The launcher has a window up (Epic sign-in / update / terms, or a Steam
                // dialog after the hiding period): put it in front, once, for the user to
                // deal with. The controller drives the mouse over launcher windows.
                if let (false, Some(l)) = (launcher_shown, req.launcher) {
                    let lp = l.pids();
                    if focus::set_windows_minimized(&lp, false) > 0 {
                        launcher_shown = true;
                        for pid in lp {
                            if focus::bring_process_to_front(pid) {
                                break;
                            }
                        }
                        report(WatchEvent::LauncherNeedsYou {
                            title: title.clone(),
                            launcher: l.name(),
                        });
                    }
                }
            }
            Step::Playing { focus_game } => {
                if matches!(last, None | Some(Step::Waiting { .. })) {
                    println!("Game session: {} is running (pids {:?})", title, pids);
                    report(WatchEvent::Playing {
                        title: title.clone(),
                    });
                }
                if focus_game {
                    let in_front = focus::foreground_pid();
                    if !in_front.is_some_and(|p| pids.contains(&p)) {
                        for &pid in &pids {
                            if focus::bring_process_to_front(pid) {
                                break;
                            }
                        }
                    }
                }
            }
            Step::Ended { played_seconds } => {
                println!("Game session: {} closed after {}s", title, played_seconds);
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                match LibraryDatabase::open(db_path) {
                    Ok(db) => {
                        if let Err(e) = db.update_playtime(&req.game_id, played_seconds, now) {
                            eprintln!("Couldn't save play time: {e}");
                        }
                    }
                    Err(e) => eprintln!("Couldn't open library to save play time: {e}"),
                }
                raise_kiln();
                report(WatchEvent::Ended {
                    title,
                    played_seconds,
                });
                return;
            }
            Step::TimedOut => {
                println!("Game session: {} never started", title);
                raise_kiln();
                report(WatchEvent::NotStarted { title });
                return;
            }
        }
        last = Some(step);
    }
}

/// Footer line for a watcher event.
pub fn status_text(ev: &WatchEvent) -> String {
    match ev {
        WatchEvent::Starting { title } => format!("Starting {title}…"),
        WatchEvent::LauncherNeedsYou { title, launcher } => format!(
            "{launcher} needs you to finish starting {title}. The stick moves the mouse, A clicks. \
             Hold both sticks in to come back."
        ),
        WatchEvent::Playing { title } => {
            format!("Playing {title}. Hold both sticks in to come back to Kiln.")
        }
        WatchEvent::Ended {
            title,
            played_seconds,
        } => format!("Played {title} for {}", short_duration(*played_seconds)),
        WatchEvent::NotStarted { title } => format!("{title} didn't start"),
    }
}

/// First line on the launch screen, before anything is known.
pub fn starting_text(launcher: &str) -> String {
    match launcher {
        "Steam" => "Starting through Steam…".into(),
        "Epic" => "Starting through Epic Games…".into(),
        _ => "Starting…".into(),
    }
}

/// Status line on the launch screen for a watcher event.
pub fn launch_screen_text(ev: &WatchEvent) -> String {
    match ev {
        WatchEvent::LauncherNeedsYou { launcher, .. } => format!(
            "{launcher} needs you before the game can start. Finish there:              the stick moves the mouse and A clicks."
        ),
        WatchEvent::NotStarted { .. } => {
            "The game didn't start. Check Steam or Epic Games, then try again.".into()
        }
        WatchEvent::Playing { .. } | WatchEvent::Ended { .. } => "Have fun!".into(),
        WatchEvent::Starting { .. } => "Starting…".into(),
    }
}

fn short_duration(s: u64) -> String {
    match s {
        0..=59 => "less than a minute".into(),
        60..=3599 => format!("{}m", s / 60),
        _ => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(dir: &str) -> WatchRequest {
        WatchRequest {
            game_id: "g".into(),
            title: "Stardew Valley".into(),
            install_dir: dir.into(),
            launcher: Some(Launcher::Steam),
        }
    }

    #[test]
    fn trackable_needs_an_absolute_install_dir() {
        assert!(req(r"C:\Steam\steamapps\common\Stardew Valley").is_trackable());
        assert!(req(r"D:\Games\Hades\").is_trackable());
        assert!(!req("").is_trackable());
        assert!(!req("Hades").is_trackable());
    }

    #[test]
    fn status_lines() {
        let ended = WatchEvent::Ended {
            title: "Stardew Valley".into(),
            played_seconds: 3725,
        };
        assert_eq!(status_text(&ended), "Played Stardew Valley for 1h 2m");
        assert_eq!(short_duration(30), "less than a minute");
        assert_eq!(short_duration(600), "10m");
    }
}
