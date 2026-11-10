//! Startup screen logic: turns the app and display scans into the rows the UI shows,
//! and works out what to close / keep / where to run when the user presses Start.

use crate::app::{LaunchApp, LaunchDisplay};
use kiln_sysctl::monitors::{DisplayScan, MonitorInfo};
use kiln_sysctl::optimizer::{CloseReport, RunningApp};
use slint::SharedString;
use std::collections::HashMap;

/// One selectable place for Kiln to run.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayTarget {
    pub title: String,
    pub detail: String,
    pub external: bool,
    pub monitor: Option<MonitorInfo>,
}

impl DisplayTarget {
    pub fn to_row(&self) -> LaunchDisplay {
        LaunchDisplay {
            title: SharedString::from(&self.title),
            detail: SharedString::from(&self.detail),
            external: self.external,
            available: self.monitor.is_some(),
        }
    }
}

/// External screens first (the TV is the point of Kiln), then the laptop screen.
pub fn display_targets(scan: &DisplayScan) -> Vec<DisplayTarget> {
    let mut out: Vec<DisplayTarget> = scan
        .external()
        .map(|m| DisplayTarget {
            title: m.name.clone(),
            detail: format!("External display · {} × {}", m.width, m.height),
            external: true,
            monitor: Some(m.clone()),
        })
        .collect();
    if out.is_empty() {
        out.push(DisplayTarget {
            title: "TV or external monitor".into(),
            detail: if scan.inactive_external > 0 {
                "Connected, but turned off in Windows".into()
            } else {
                "Not detected".into()
            },
            external: true,
            monitor: None,
        });
    }
    if let Some(m) = scan.internal() {
        out.push(DisplayTarget {
            title: if m.internal {
                "Laptop screen".into()
            } else {
                m.name.clone()
            },
            detail: format!(
                "{} · {} × {}",
                if m.internal {
                    "Built-in display"
                } else {
                    "Main display"
                },
                m.width,
                m.height
            ),
            external: false,
            monitor: Some(m.clone()),
        });
    }
    out
}

/// Remembered preference ("external"/"internal") if it's available, otherwise the first
/// available external screen, otherwise the laptop screen.
pub fn default_display_choice(targets: &[DisplayTarget], preferred: &str) -> usize {
    let available = |t: &DisplayTarget| t.monitor.is_some();
    let pick = |external: bool| {
        targets
            .iter()
            .position(|t| t.external == external && available(t))
    };
    match preferred {
        "internal" => pick(false).or_else(|| pick(true)),
        _ => pick(true).or_else(|| pick(false)),
    }
    .unwrap_or(0)
}

/// Rows for the app list. `previous` keeps the user's Close/Keep choices across a refresh.
pub fn app_rows(
    apps: &[RunningApp],
    always_keep: &[String],
    previous: &HashMap<String, bool>,
) -> Vec<LaunchApp> {
    apps.iter()
        .map(|a| {
            let close = previous.get(&a.exe).copied().unwrap_or_else(|| {
                a.category.closes_by_default() && !always_keep.iter().any(|k| k == &a.exe)
            });
            LaunchApp {
                exe: SharedString::from(&a.exe),
                name: SharedString::from(&a.name),
                category: SharedString::from(a.category.label()),
                processes: a.process_count as i32,
                close,
            }
        })
        .collect()
}

/// Updates the always-keep list from the user's choices: apps set to Close are removed
/// (an explicit choice wins), apps set to Keep are added when `remember` is on.
pub fn update_always_keep(current: &[String], rows: &[LaunchApp], remember: bool) -> Vec<String> {
    let mut list: Vec<String> = current
        .iter()
        .filter(|k| !rows.iter().any(|r| r.close && r.exe.as_str() == k.as_str()))
        .cloned()
        .collect();
    if remember {
        for r in rows.iter().filter(|r| !r.close) {
            if !list.iter().any(|k| k == r.exe.as_str()) {
                list.push(r.exe.to_string());
            }
        }
    }
    list.sort();
    list
}

/// Short summary for the status line after Start.
/// Loading-screen line while apps close: names up to two apps, then counts.
pub fn closing_status(names: &[String]) -> String {
    match names {
        [] => "Getting your library ready…".into(),
        [a] => format!("Closing {a}…"),
        [a, b] => format!("Closing {a} and {b}…"),
        _ => format!("Closing {} apps…", names.len()),
    }
}

pub fn summarize(report: &CloseReport, dry_run: bool) -> String {
    let join = |v: &[String]| v.join(", ");
    let mut parts = Vec::new();
    if !report.closed.is_empty() {
        parts.push(format!(
            "{} {}",
            if dry_run { "Would close" } else { "Closed" },
            join(&report.closed)
        ));
    }
    if !report.still_open.is_empty() {
        parts.push(format!(
            "{} stayed open (it may have unsaved work)",
            join(&report.still_open)
        ));
    }
    if parts.is_empty() {
        "Ready to play".into()
    } else {
        parts.join(" · ")
    }
}

/// Focus order on the startup screen: apps, "remember" checkbox, displays, Quit, Start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchFocus {
    App(usize),
    Remember,
    Display(usize),
    Quit,
    Start,
}

pub fn focus_item(index: i32, apps: usize, displays: usize) -> LaunchFocus {
    let i = index.max(0) as usize;
    if i < apps {
        LaunchFocus::App(i)
    } else if i == apps {
        LaunchFocus::Remember
    } else if i < apps + 1 + displays {
        LaunchFocus::Display(i - apps - 1)
    } else if i == apps + 1 + displays {
        LaunchFocus::Quit
    } else {
        LaunchFocus::Start
    }
}

pub fn start_index(apps: usize, displays: usize) -> i32 {
    (apps + displays + 2) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_sysctl::optimizer::AppCategory;

    fn mon(name: &str, internal: bool, x: i32) -> MonitorInfo {
        MonitorInfo {
            name: name.into(),
            internal,
            primary: x == 0,
            x,
            y: 0,
            width: 1920,
            height: 1080,
        }
    }

    fn running(exe: &str, category: AppCategory) -> RunningApp {
        RunningApp {
            exe: exe.into(),
            name: exe.into(),
            category,
            process_count: 1,
        }
    }

    #[test]
    fn external_screen_listed_first_and_preferred() {
        let scan = DisplayScan {
            active: vec![mon("Built-in display", true, 0), mon("LG TV", false, 1920)],
            inactive_external: 0,
        };
        let t = display_targets(&scan);
        assert_eq!(t[0].title, "LG TV");
        assert_eq!(t[1].title, "Laptop screen");
        assert_eq!(default_display_choice(&t, ""), 0);
        assert_eq!(default_display_choice(&t, "internal"), 1);
    }

    #[test]
    fn missing_tv_shows_disabled_option_and_defaults_to_laptop() {
        let scan = DisplayScan {
            active: vec![mon("Built-in display", true, 0)],
            inactive_external: 1,
        };
        let t = display_targets(&scan);
        assert!(t[0].monitor.is_none());
        assert_eq!(t[0].detail, "Connected, but turned off in Windows");
        assert_eq!(default_display_choice(&t, "external"), 1);
    }

    #[test]
    fn defaults_respect_category_and_always_keep_list() {
        let apps = [
            running("chrome.exe", AppCategory::Browser),
            running("code.exe", AppCategory::Productivity),
            running("discord.exe", AppCategory::Chat),
        ];
        let rows = app_rows(&apps, &["discord.exe".into()], &HashMap::new());
        let close: Vec<bool> = rows.iter().map(|r| r.close).collect();
        assert_eq!(close, [true, false, false]);
    }

    #[test]
    fn explicit_close_removes_from_keep_list_and_keep_is_remembered() {
        let apps = [
            running("chrome.exe", AppCategory::Browser),
            running("slack.exe", AppCategory::Chat),
        ];
        let mut rows = app_rows(&apps, &["chrome.exe".into()], &HashMap::new());
        rows[0].close = true; // user now wants Chrome closed
        rows[1].close = false; // and Slack kept
        assert_eq!(
            update_always_keep(&["chrome.exe".into()], &rows, true),
            ["slack.exe"]
        );
        assert!(update_always_keep(&["chrome.exe".into()], &rows, false).is_empty());
    }

    #[test]
    fn focus_order_maps_indices() {
        assert_eq!(focus_item(0, 2, 2), LaunchFocus::App(0));
        assert_eq!(focus_item(2, 2, 2), LaunchFocus::Remember);
        assert_eq!(focus_item(4, 2, 2), LaunchFocus::Display(1));
        assert_eq!(focus_item(5, 2, 2), LaunchFocus::Quit);
        assert_eq!(focus_item(6, 2, 2), LaunchFocus::Start);
        assert_eq!(start_index(2, 2), 6);
    }

    #[test]
    fn summary_mentions_apps_that_stayed_open() {
        let r = CloseReport {
            closed: vec!["Discord".into()],
            still_open: vec!["Slack".into()],
            skipped: vec![],
        };
        assert_eq!(
            summarize(&r, false),
            "Closed Discord · Slack stayed open (it may have unsaved work)"
        );
        assert_eq!(summarize(&CloseReport::default(), false), "Ready to play");
    }

    #[test]
    fn closing_status_names_up_to_two_apps() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(closing_status(&n(&[])), "Getting your library ready…");
        assert_eq!(closing_status(&n(&["Vivaldi"])), "Closing Vivaldi…");
        assert_eq!(
            closing_status(&n(&["Vivaldi", "Discord"])),
            "Closing Vivaldi and Discord…"
        );
        assert_eq!(closing_status(&n(&["a", "b", "c"])), "Closing 3 apps…");
    }
}
