# Kiln 🎮

> **Fast, Rust-only Game Mode console shell for Windows 11.**

[![CI](https://github.com/srijankulal/kiln/actions/workflows/ci.yml/badge.svg)](https://github.com/srijankulal/kiln/actions/workflows/ci.yml)
[![Release](https://github.com/srijankulal/kiln/actions/workflows/release.yml/badge.svg)](https://github.com/srijankulal/kiln/actions/workflows/release.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

---

## Overview

In normal daily use, a laptop functions as a standard Windows 11 workstation. When connected to a TV, **Kiln** transforms the machine into a dedicated gaming console:

1. **TV-Only Display:** Automatically shifts output to the external TV (`SDC_TOPOLOGY_EXTERNAL`), turns off the laptop panel, and configures the lid close action to "do nothing" so the laptop lid can stay closed.
2. **Crash-Safe Architecture:** Write-Ahead Logging (WAL) via an atomic journal (`%LOCALAPPDATA%\Kiln\session.json`). If interrupted or crashed, a full idempotent restore runs automatically on next boot.
3. **Emergency Abort:** Holding **Back + Start** on any gamepad for 5.0 seconds (or pressing `Ctrl+Alt+Shift+G`) instantly aborts Game Mode and restores the laptop desktop.
4. **10-Foot Controller Shell:** Native declarative [Slint](https://slint.dev) UI designed for 4K/1080p TVs with 5% safe overscan margins, fluid focus animations, dynamic filtering, and carousel views.
5. **Universal Game Library:** Scans and indexes games from Steam, Epic Games, Xbox/Microsoft Store, and standalone directory installs without requiring launchers to be open.
6. **Background App Trimming:** Suspends non-essential user background apps (Tier 1) or throttles them via EcoQoS (Tier 0) while gaming, and restores them on exit. Never touches critical system or anti-cheat processes.
7. **Gamepad-as-Mouse Mode:** Pressing the Guide/Mode button switches the analog stick into an accelerated mouse cursor ($speed \propto magnitude^{1.5}$) with A/B button clicks for launcher dialogs.
8. **On-Demand Web Helper:** Spawns a short-lived `kiln-auth` popup only when a launcher requires web authentication, keeping idle memory negligible.

---

## Install

1. Download **`kiln-<version>-setup.exe`** from the [latest release](https://github.com/srijankulal/kiln/releases/latest).
2. Run it. Kiln installs for your Windows account only (no admin prompt) and adds a Start menu shortcut. If Windows shows "Windows protected your PC", click **More info**, then **Run anyway** (Kiln isn't code-signed yet).
3. Kiln checks for updates by itself and offers them in **Settings › About**, where the version you're running is shown.

To uninstall, use **Settings › Apps › Installed apps › Kiln › Uninstall**. Your library and settings in `%LOCALAPPDATA%\Kiln` are kept unless you choose to delete them.

Prefer no install? The release also has a portable zip: extract it and run `Kiln.exe`.

Every push to `main` that passes CI is released automatically with the next `0.xxx.xx` version.

---

## Workspace Architecture

```
kiln/
├── crates/
│   ├── kiln-core/       Shared domain models, state machine, WAL journal engine, and IPC schema
│   ├── kiln-library/    Multi-source game scanners (Steam, Epic, Xbox, Standalone) and SQLite store
│   ├── kiln-sysctl/     Win32 display, power plan, taskbar, process trimming, and audio controls
│   ├── kiln-input/      Gamepad poller (gilrs), circular deadzones, emergency chord, mouse mode
│   ├── kiln-daemon/     Always-on background supervisor daemon, named pipe IPC server, global hotkeys
│   ├── kiln-shell/      Fullscreen 10-foot Slint TV UI binary
│   ├── kiln-auth/       Lightweight on-demand OAuth login pop-up helper
│   └── kiln-updater/    Checks GitHub releases, downloads and runs the installer to update
├── installer/           NSIS installer script (kiln-<version>-setup.exe) and portable readme
├── docs/                Architecture specs and findings
└── .github/workflows/   Automated CI and release cross-compilation pipeline
```

---

## State Machine & Crash Recovery

```
Desktop -> Entering -> GameMode -> Launching -> InGame -> GameMode
                          |                                 |
                          +-------------> Exiting <---------+
                                             |
                                          Desktop
```

- **Journaling:** The session journal is atomically written to disk **before** any Win32 mutation occurs.
- **Automated Recovery:** On daemon startup, if an uncommitted journal is detected, Kiln restores process priorities, unhides the taskbar, reverts power schemes, and switches display back to the internal panel.
- **Crash Loop Breaker:** If `kiln-shell` crashes 3 times within a 5-minute window, Game Mode aborts safely to Desktop.

---

## CLI Usage

Kiln binaries support unified flags for safe testing and diagnostic checks:

```powershell
# Run shell in safe windowed mode with dry-run simulation
kiln-shell --safe --dry-run

# Run daemon with dry-run simulation
kiln-daemon --dry-run

# Perform immediate emergency system restore
kiln-daemon --restore

# Run system capability diagnostic check
kiln-daemon --self-check

# Run standalone headless game library scanner
kiln-scan --verbose
```

---

## Input & Gamepad Controls

| Input | Action |
| --- | --- |
| **D-Pad / Left Stick** | Navigate game cards and settings options |
| **Ⓐ (South)** | Select game / Launch / Confirm modal |
| **Ⓑ (East)** | Back / Close modal / Open Power menu |
| **Ⓧ (West)** | Toggle game Favorite status |
| **Ⓨ (North)** | Trigger full library rescan |
| **LB / RB (Triggers)** | Switch category filter tabs (All, Recent, Favorites, Steam, Epic, Other, Settings) |
| **Guide / Mode Button** | Toggle Gamepad-as-Mouse mode |
| **Back + Start (Hold 5s)** | **Emergency Abort:** Instantly restore desktop and exit |
| **L3 + R3 (both sticks in, hold 1s, in a game or launcher)** | Bring Kiln back to the front |
| **Left stick / Ⓐ / Ⓑ over Steam or Epic** | Mouse pointer turns on automatically: move, click, right-click (Guide toggles it) |
| **Ctrl + Alt + Shift + G** | Global keyboard emergency toggle |

---

## Building from Source

### Prerequisites

- Rust stable toolchain (`1.75+`)
- `cargo-xwin` (for Windows MSVC cross-compilation on Linux or macOS)

```bash
# Run tests natively
cargo test --workspace

# Run clippy linter
cargo clippy --workspace --all-targets -- -D warnings

# Cross-compile release binaries for Windows (x86_64-pc-windows-msvc)
cargo xwin build --release --target x86_64-pc-windows-msvc
```

Compiled Windows PE x64 binaries will be located in:
`target/x86_64-pc-windows-msvc/release/*.exe`

---

## License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
