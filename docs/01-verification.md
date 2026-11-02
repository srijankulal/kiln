# Technical Verification Report: Win32 APIs, Crates & Subsystems

*Completed: 2 Nov 2026 | Project: Kiln | Lead Architect*

---

## 1. Executive Summary

This document records the verification of all `[verify]` items flagged in `docs/SPEC.md`. Every subsystem interaction has been confirmed against official Microsoft Win32/WinRT documentation, crate specifications, or verified open-source implementations.

---

## 2. Subsystem Verifications

### 2.1 Power, Lid Action & Sleep Prevention (Section 5.2) — **CONFIRMED**
- **Win32 APIs:**
  - `PowerGetActiveScheme` / `PowerSetActiveScheme` in `windows::Win32::System::Power`.
  - `PowerReadACValueIndex` / `PowerWriteACValueIndex` / `PowerWriteDCValueIndex`.
  - `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED)`.
- **GUIDs:**
  - Subgroup (`GUID_SYSTEM_BUTTON_SUBGROUP`): `4f971e89-eebd-4455-a8de-9e59040e7347`
  - Lid Close Action (`GUID_LIDCLOSE_ACTION`): `5ca83367-6e45-459f-a27b-476b1d01c936`
  - Setting Value `0`: "Do nothing". Values `1`, `2`, `3` correspond to Sleep, Hibernate, and Shut down.
- **Safety Mechanism:** Kiln snapshots both AC and DC indices to `session.json` prior to writing `0`, and calls `PowerSetActiveScheme` to apply immediately. On exit, previous indices are rewritten and applied.

---

### 2.2 Background Process Trimming (Section 5.3) — **CONFIRMED**
- **Tier 0 (EcoQoS / Efficiency Mode):**
  - Uses documented `SetProcessInformation` with `ProcessPowerThrottling`.
  - Requires `PROCESS_SET_INFORMATION` access.
  - Struct `PROCESS_POWER_THROTTLING_STATE` with `ControlMask = PROCESS_POWER_THROTTLING_EXECUTION_SPEED` and `StateMask = PROCESS_POWER_THROTTLING_EXECUTION_SPEED`.
  - Paired with `SetPriorityClass(IDLE_PRIORITY_CLASS)`.
- **Tier 1 (Suspend / Resume):**
  - `NtSuspendProcess` and `NtResumeProcess` are resolved via `GetProcAddress(GetModuleHandleW("ntdll.dll"), ...)`.
  - **Recycled PID Safety Check:** Before suspending, Kiln queries `GetProcessTimes` to obtain the 64-bit `CreationTime` filetime. The journal records `(PID, CreationTime)`. On exit, Kiln checks `GetProcessTimes` again; if the start time differs or the process is gone, `NtResumeProcess` is skipped to avoid un-pausing an unrelated recycled PID.
- **Never-Touch List:**
  - Hardcoded safety blocklist: `System`, `smss.exe`, `csrss.exe`, `wininit.exe`, `services.exe`, `lsass.exe`, `svchost.exe`, `dwm.exe`, `explorer.exe` (Phase 1), `audiodg.exe`, `ctfmon.exe`, `MsMpEng.exe`.
  - Anti-cheat patterns: `EasyAntiCheat*`, `BEService*`, `vgc.exe`, `vgk.sys`, `zksvc.exe`.

---

### 2.3 Audio Endpoint Switching (Section 5.5) — **CONFIRMED**
- **Undocumented COM Interface:** `IPolicyConfig`
  - `CLSID_PolicyConfig`: `{870af99c-171d-4f9e-af0d-e63df40c2bc9}`
  - `IID_IPolicyConfig`: `{f8679f50-850a-41cf-9c72-430f290290c8}`
  - Method `SetDefaultEndpoint(LPCWSTR wszDeviceId, ERole role)` where `ERole` targets `eMultimedia` (1) and `eConsole` (0).
- **Safety Fallback:** Because `IPolicyConfig` is private and can change across Windows major releases, all audio calls are wrapped behind `AudioApi` in `kiln-core`. Failure to switch audio logs an informational warning and never blocks entering Game Mode.

---

### 2.4 Launcher Manifests & URI Launchers (Section 5.6) — **CONFIRMED**
- **Steam:**
  - `steamlocate` (v2.1+) resolves Steam install directories and libraries.
  - Reads `steamapps/appmanifest_<appid>.acf`. Excludes `228980` (redistributables) and tooling depots.
  - Launch URI: `steam://rungameid/<appid>`.
- **Epic Games:**
  - Manifest directory: `C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests\*.item`.
  - JSON attributes: `AppName`, `DisplayName`, `InstallLocation`, `LaunchExecutable`, `bIsApplication`.
  - Filter: Ignore entries prefixed with `UE_` (Unreal Engine).
  - Launch URI: `com.epicgames.launcher://apps/<AppName>?action=launch&silent=true`.
- **GOG Galaxy:**
  - Registry: `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games\<gameID>` (`gameName`, `path`, `exe`, `launchCommand`).
- **EA App:**
  - Registry: `HKLM\SOFTWARE\WOW6432Node\Electronic Arts\EA Core\Installed Games`.
- **Ubisoft Connect:**
  - Registry: `HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\<gameID>` (`InstallDir`).
  - Launch URI: `uplay://launch/<gameID>/0`.
- **Xbox / MS Store:**
  - WinRT `PackageManager::FindPackagesForUserWithPackageTypes`.
  - Launch URI: `shell:AppsFolder\<PackageFamilyName>!<AppId>`.

---

### 2.5 Input: gilrs & Slint Event Dispatch (Section 5.7) — **CONFIRMED**
- **gilrs Configuration:**
  - `gilrs` defaults to `wgi` (Windows Gaming Input), which suppresses input when the process window loses focus.
  - Setting `default-features = false, features = ["xinput"]` binds directly to XInput DLLs (`xinput1_4.dll` / `xinput9_1_0.dll`), allowing `kiln-daemon` to receive controller events while running in the background.
- **Slint Key Event Dispatch:**
  - Slint provides `window.dispatch_event(slint::platform::WindowEvent::KeyPressed { text: ... })` and `KeyReleased`.
  - Mapped inputs: D-Pad & Sticks -> Up/Down/Left/Right; A -> Return; B -> Escape; LB/RB -> Tab switches.
  - InGame suppression: When `InGame`, all input dispatch is inhibited except the 5-second `Select + Start` panic chord.

---

### 2.6 Credential & Secret Storage (Section 13.5) — **CONFIRMED**
- `keyring` crate provides native bindings to Windows Credential Manager (`CredReadW` / `CredWriteW` / `CredDeleteW`).
- The user's SteamGridDB API key is stored securely under service target `Kiln:SteamGridDB`, keeping plain secrets out of `config.toml` and logs.

---

### 2.7 Dedicated Console Account (`KilnConsole`) (Section 13.5) — **CONFIRMED**
- Standard user profile isolation: An audited PowerShell script (`scripts/create-console-user.ps1`) allows the user to optionally create `KilnConsole`.
- Setting `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Winlogon\Shell` inside that user's hive replaces Explorer without modifying machine-wide settings or touching the primary user account.
