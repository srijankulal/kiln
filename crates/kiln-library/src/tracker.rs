//! Game session tracking: works out when a launched game is actually running and for how
//! long, from periodic process snapshots. Pure logic (no Win32): the shell feeds it "is a
//! process from the game's install folder running right now?" about once a second.

use std::time::{Duration, Instant};

/// Timings for a session. The defaults suit launcher-started games (Steam / Epic).
#[derive(Debug, Clone, Copy)]
pub struct TrackerConfig {
    /// While waiting for the game, launcher windows are hidden behind Kiln for this long.
    /// After that the launcher probably needs the user (sign-in, terms, cloud conflict).
    pub hide_launcher_for: Duration,
    /// Give up if the game hasn't appeared after this long.
    pub wait_timeout: Duration,
    /// The game counts as closed once none of its processes have been seen for this long
    /// (some games restart their own exe, or swap a launcher exe for the real one).
    pub exit_grace: Duration,
    /// After the game appears, keep handing it the foreground for this long.
    pub focus_game_for: Duration,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            hide_launcher_for: Duration::from_secs(45),
            wait_timeout: Duration::from_secs(10 * 60),
            exit_grace: Duration::from_secs(10),
            focus_game_for: Duration::from_secs(20),
        }
    }
}

/// What the watcher should do after a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The game isn't running yet. `hide_launcher`: keep launcher windows out of the way.
    Waiting { hide_launcher: bool },
    /// The game is running. `focus_game`: it just started, make sure it is in front.
    Playing { focus_game: bool },
    /// The game closed after `played_seconds` of play.
    Ended { played_seconds: u64 },
    /// The game never started.
    TimedOut,
}

#[derive(Debug)]
pub struct SessionTracker {
    config: TrackerConfig,
    launched_at: Instant,
    first_seen: Option<Instant>,
    last_seen: Option<Instant>,
    done: bool,
}

impl SessionTracker {
    pub fn new(launched_at: Instant, config: TrackerConfig) -> Self {
        Self {
            config,
            launched_at,
            first_seen: None,
            last_seen: None,
            done: false,
        }
    }

    /// Feeds one snapshot. After `Ended` / `TimedOut` the session is over and further calls
    /// repeat the final step.
    pub fn step(&mut self, now: Instant, game_running: bool) -> Step {
        let c = self.config;
        match (self.first_seen, self.last_seen) {
            (Some(first), Some(last)) => {
                if game_running && !self.done {
                    self.last_seen = Some(now);
                    return Step::Playing {
                        focus_game: now.saturating_duration_since(first) < c.focus_game_for,
                    };
                }
                if self.done || now.saturating_duration_since(last) >= c.exit_grace {
                    self.done = true;
                    return Step::Ended {
                        played_seconds: last.saturating_duration_since(first).as_secs(),
                    };
                }
                Step::Playing { focus_game: false }
            }
            _ => {
                if game_running && !self.done {
                    self.first_seen = Some(now);
                    self.last_seen = Some(now);
                    return Step::Playing { focus_game: true };
                }
                let waited = now.saturating_duration_since(self.launched_at);
                if self.done || waited >= c.wait_timeout {
                    self.done = true;
                    return Step::TimedOut;
                }
                Step::Waiting {
                    hide_launcher: waited < c.hide_launcher_for,
                }
            }
        }
    }
}

/// Helper exes that live in game folders but aren't the game itself.
fn is_helper_exe(file_name: &str) -> bool {
    let n = file_name.to_ascii_lowercase();
    [
        "crashhandler",
        "crashpad",
        "crashreport",
        "unins",
        "redist",
        "vc_redist",
        "dxsetup",
        "dotnet",
        "setup",
        "installer",
        "easyanticheat_setup",
    ]
    .iter()
    .any(|h| n.contains(h))
}

/// Case-insensitive "is `image` inside `dir`", on a path-component boundary.
pub fn path_is_inside(image: &str, dir: &str) -> bool {
    let norm = |s: &str| {
        s.replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    };
    let (img, dir) = (norm(image), norm(dir));
    // Refuse empty / relative dirs: they'd match everything.
    // (Checked by hand, not Path::is_absolute: these are Windows paths on any host.)
    let b = dir.as_bytes();
    let absolute = (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\')
        || dir.starts_with("\\\\");
    if !absolute {
        return false;
    }
    img.len() > dir.len() && img.starts_with(&dir) && img.as_bytes()[dir.len()] == b'\\'
}

/// PIDs of running processes that belong to the game: their exe is inside `install_dir` and
/// isn't a crash handler / installer. `procs` is (pid, full image path).
pub fn game_pids(install_dir: &str, procs: &[(u32, String)]) -> Vec<u32> {
    procs
        .iter()
        .filter(|(_, path)| {
            let name = path.rsplit(['\\', '/']).next().unwrap_or("");
            path_is_inside(path, install_dir) && !is_helper_exe(name)
        })
        .map(|(pid, _)| *pid)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn full_session_measures_time_between_first_and_last_sighting() {
        let t0 = Instant::now();
        let mut t = SessionTracker::new(t0, TrackerConfig::default());
        assert_eq!(
            t.step(t0, false),
            Step::Waiting {
                hide_launcher: true
            }
        );
        assert_eq!(
            t.step(t0 + secs(30), true),
            Step::Playing { focus_game: true }
        );
        assert_eq!(
            t.step(t0 + secs(60), true),
            Step::Playing { focus_game: false }
        );
        assert_eq!(
            t.step(t0 + secs(3630), true),
            Step::Playing { focus_game: false }
        );
        // Gone, but within the grace period.
        assert_eq!(
            t.step(t0 + secs(3635), false),
            Step::Playing { focus_game: false }
        );
        assert_eq!(
            t.step(t0 + secs(3641), false),
            Step::Ended {
                played_seconds: 3600
            }
        );
        // Stays ended.
        assert_eq!(
            t.step(t0 + secs(3700), true),
            Step::Ended {
                played_seconds: 3600
            }
        );
    }

    #[test]
    fn exe_restart_within_grace_is_one_session() {
        let t0 = Instant::now();
        let mut t = SessionTracker::new(t0, TrackerConfig::default());
        t.step(t0, true);
        t.step(t0 + secs(5), false);
        t.step(t0 + secs(9), true);
        assert_eq!(
            t.step(t0 + secs(100), true),
            Step::Playing { focus_game: false }
        );
        assert_eq!(
            t.step(t0 + secs(111), false),
            Step::Ended {
                played_seconds: 100
            }
        );
    }

    #[test]
    fn launcher_hidden_only_for_a_while_then_timeout() {
        let t0 = Instant::now();
        let mut t = SessionTracker::new(t0, TrackerConfig::default());
        assert_eq!(
            t.step(t0 + secs(44), false),
            Step::Waiting {
                hide_launcher: true
            }
        );
        assert_eq!(
            t.step(t0 + secs(46), false),
            Step::Waiting {
                hide_launcher: false
            }
        );
        assert_eq!(t.step(t0 + secs(601), false), Step::TimedOut);
        assert_eq!(t.step(t0 + secs(602), true), Step::TimedOut);
    }

    #[test]
    fn path_matching_is_case_insensitive_and_on_boundaries() {
        let dir = r"C:\Program Files (x86)\Steam\steamapps\common\Stardew Valley";
        assert!(path_is_inside(
            r"c:\program files (x86)\steam\steamapps\common\stardew valley\Stardew Valley.exe",
            dir
        ));
        assert!(path_is_inside(
            r"C:/Program Files (x86)/Steam/steamapps/common/Stardew Valley/bin/x.exe",
            &format!("{dir}\\")
        ));
        assert!(!path_is_inside(
            r"C:\Program Files (x86)\Steam\steamapps\common\Stardew Valley 2\game.exe",
            dir
        ));
        assert!(!path_is_inside(r"C:\anything.exe", ""));
        assert!(!path_is_inside(r"C:\anything.exe", "relative\\dir"));
    }

    #[test]
    fn game_pids_skip_helpers() {
        let procs = vec![
            (1, r"D:\Games\Hades\Hades.exe".to_string()),
            (2, r"D:\Games\Hades\UnityCrashHandler64.exe".to_string()),
            (3, r"D:\Games\Other\Other.exe".to_string()),
            (
                4,
                r"D:\Games\Hades\_CommonRedist\vc_redist.x64.exe".to_string(),
            ),
        ];
        assert_eq!(game_pids(r"D:\Games\Hades", &procs), vec![1]);
    }
}
