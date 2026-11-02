# Kiln: Master Orchestrator Prompt & Agent Build Specification

*Prepared: 2 Nov 2026 | Project: **Kiln** | Owner: Srijan*

---

## PART A: MASTER ORCHESTRATOR PROMPT (FOR ANTIGRAVITY CLI)

### Role & Mission
You are the **Lead Architect & Orchestrator Agent** of an autonomous multi-agent engineering team building **Kiln**, a fast, ultra-safe Rust console-mode shell for Windows 11 laptops (`x86_64-pc-windows-msvc`).

This document (`docs/SPEC.md`) is the single source of truth. You and every sub-agent you spawn must follow the architectural rules, Win32 recipes, and safety invariants defined below before writing a single line of code.

### Environment & Auto-Bootstrap Authorization
You are running on an Ubuntu Linux VPS and cross-compiling to Windows using `cargo-xwin`.
You are **PRE-AUTHORIZED** to execute the idempotent setup in **Section 2.1** (`scripts/bootstrap-vps.sh`) without asking for permission:
- Install system packages (`build-essential`, `clang`, `lld`, `llvm`, `nasm`, `ninja-build`, `cmake`, `pkg-config`, `git`, `curl`, `unzip`, `zip`, `ca-certificates`, `wine64`, `nodejs`, `npm`, `gh`) via `apt`.
- Install `rustup` (stable, `clippy`, `rustfmt`, `llvm-tools-preview`, target `x86_64-pc-windows-msvc`).
- Install cargo tools (`cargo-xwin`, `cargo-nextest`, `cargo-deny`, `cargo-audit`, `cargo-bloat`, `cargo-udeps`) from `crates.io` with `--locked`.
- Set `XWIN_ACCEPT_LICENSE=1` (the user explicitly accepts Microsoft's CRT/SDK license: https://go.microsoft.com/fwlink/?LinkId=2086102).
- Install MCP servers and agent skills into `./.agent/skills/` as detailed in **Section 8**.

**Ask the user FIRST before:** modifying swap/kernel settings, using any package source outside `apt`/`rustup.rs`/`crates.io`/`nodejs.org`/`cli.github.com`, opening network ports, or storing secrets. Never run piped scripts from unverified third-party URLs.

### Non-Negotiable Guardrails
1. **SAFETY & RESTORE FIRST:** No system state on Windows may be mutated without first writing an atomic, `fsync`ed journal entry to `%LOCALAPPDATA%\Kiln\session.json`. Every mutation must be per-user, idempotent, and reversible. Never touch BCD/boot config, system files, drivers, Defender/Firewall, game files, saves, or anti-cheat processes.
2. **TRAIT-FIRST CROSS-COMPILATION:** Every OS interaction (`DisplayApi`, `PowerApi`, `ProcessApi`, `RegistryApi`, `FileApi`, `AudioApi`, `Clock`) MUST sit behind a trait in `kiln-core`. Real implementations use `#[cfg(windows)]`; Linux builds use `Fake*` implementations for deterministic unit and property-based failure-injection testing (`proptest`).
3. **STRICT MULTI-AGENT ISOLATION:**
   - You (Lead Agent) exclusively own `Cargo.toml` (workspace root), `crates/kiln-core`, and `docs/`.
   - Once `kiln-core` is frozen at the end of Milestone 0, sub-agents may NOT modify `kiln-core` or root `Cargo.toml` directly; they must submit interface change requests to you.
   - Spawn sub-agents in isolated Git worktrees or strictly partitioned crate directories (`crates/kiln-*`) to prevent merge collisions.
4. **MILESTONE GATING:** Execute Milestones M0 through M7 in order (**Section 10**). Stop after each milestone, run the full L0/L1 test suite (`cargo nextest run` + `cargo xwin build --release --target x86_64-pc-windows-msvc`), and output a structured **Milestone Completion Report** (Working, Verified, Failed/Blocked, Next Steps).

---

## PART B: TECHNICAL SPECIFICATION

### 0. Mission & Scope

Build a tiny, fast, Rust-only **Game Mode shell (`Kiln`)** that runs on top of Windows 11 on a laptop. In normal use, the laptop behaves like standard Windows. When the user connects a TV and presses a hotkey (or accepts a toast prompt), the system switches into a console-like appliance mode:

1. **TV-Only Output:** Display switches to the external TV only; the laptop panel turns off; the lid may be closed without sleeping.
2. **Reversible Background Trim:** Unnecessary background apps are throttled (EcoQoS) or suspended (`NtSuspendProcess`). Everything is restored automatically on exit or crash.
3. **Launcher-Agnostic Library:** A fullscreen, controller-first 10-foot UI lists every installed game across Steam, Epic, GOG, EA, Ubisoft, Battle.net, Xbox/MS Store, and standalone folders **without running any launcher in the background**.
4. **Launch-on-Demand:** Launchers start **only** when the selected game requires them, and optionally close when the game exits.
5. **Frictionless Input & Auth:** Gamepad, keyboard, and mouse work seamlessly. Launcher login pop-ups are surfaced cleanly with gamepad-as-mouse + OSK support.
6. **Bulletproof Restore:** Exiting Game Mode—or experiencing a crash, power loss, or HDMI unplug—restores the laptop to its exact pre-session state.

**Design Values:** Minimal footprint (<20 MB daemon RAM, ~0% idle CPU), safe by default, zero telemetry, customizable through a single clean Settings screen.

**Non-Goals (v1):** Replacing the Windows kernel or boot loader, emulators, game streaming, store fronts, achievements, cloud saves, multi-user profiles, or any interaction with anti-cheat / game memory.

**CLI Entrypoints:**
- `kiln --dry-run` (prints planned display, power, and process trim actions; changes nothing)
- `kiln --safe` (launches shell UI without display, power, or process trimming)
- `kiln --restore` (forces immediate idempotent rollback from `%LOCALAPPDATA%\Kiln\session.json`)
- `kiln --self-check` (audits system state against journal/defaults and reports health)

---

### 1. Technology Stack & Process Architecture

- **Workspace:**
  - `kiln-core`: Traits, state machine, journal engine, IPC protocol, shared models, `Fake*` doubles.
  - `kiln-sysctl`: Windows implementations (`Win32_Devices_Display`, `Power`, `NtSuspendProcess`, `EcoQoS`, `Taskbar`, `IPolicyConfig`).
  - `kiln-library`: Parallel scanners, SQLite store, SteamGridDB artwork cache, Job Object game tracker.
  - `kiln-input`: `gilrs` (xinput feature), gamepad navigation, gamepad-as-mouse mode.
  - `kiln-daemon`: Background supervisor, system tray, global hotkeys, `WM_DISPLAYCHANGE` listener.
  - `kiln-shell`: Fullscreen Slint 10-foot TV UI.
  - `kiln-auth`: Lightweight `wry` + `tao` WebView2 helper binary.
  - `kiln-updater`: Signed rollback-capable update checker stub.

---

### 2. State Machine & Seven Layers of Safety

```text
Desktop -> Entering -> ConfirmingDisplay (15s timer) -> GameMode <-> Launching <-> InGame
               |                      |                     |                        |
               +----------------------+---------------------+----> Exiting <---------+
                                                                      |
                                                                   Desktop
```

1. **Write-Ahead Journal:** Atomic write and sync to `%LOCALAPPDATA%\Kiln\session.json` before any Win32 call.
2. **Display Confirmation Timer:** 15-second timeout reverts to laptop panel unless confirmed.
3. **HDMI Unplug Reflex:** `WM_DISPLAYCHANGE` restores laptop display within <3 seconds if external TV disconnects.
4. **Emergency Panic Chord:** Holding `Back + Start` for 5 seconds or pressing `Ctrl+Alt+Shift+G` triggers emergency restore.
5. **Crash-Loop Breaker:** 3 shell crashes in 5 minutes forces full restore and switches to `--safe` mode.
6. **Dead-Man's Switch:** Scheduled task executes `kiln --restore --if-stale` on logon and every 5 minutes.
7. **Strict Hard Prohibitions:** Never touch BCD, drivers, Defender, firewall, game saves, or anti-cheat.
