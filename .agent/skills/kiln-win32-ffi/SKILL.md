---
name: kiln-win32-ffi
description: Enforces safe wrappers around windows crate unsafe blocks, proper HANDLE/HWND RAII cleanup, and strict #[cfg(windows)] isolation behind kiln-core traits.
---

# Kiln Win32 FFI & Windows API Conventions

All Windows platform-specific code in Kiln lives in `crates/kiln-sysctl` and is isolated behind abstract traits defined in `crates/kiln-core`.

## Core Guidelines

1. **Strict Trait Isolation:**
   - Any OS interaction (`DisplayApi`, `PowerApi`, `ProcessApi`, `RegistryApi`, `FileApi`, `AudioApi`, `Clock`) is defined as a trait in `kiln-core`.
   - The concrete Win32 implementations reside in `kiln-sysctl` with `#[cfg(windows)]`.
   - Non-Windows targets compile `Fake*` implementations so the entire state machine, IPC, and scanners can be compiled and verified on Linux VPS and CI.

2. **Scoping `unsafe` Blocks:**
   - Keep `unsafe` blocks as small as possible.
   - Every `unsafe` block MUST have a `// SAFETY:` comment explaining why invariants are upheld (e.g. pointer validity, buffer sizes, API preconditions).

3. **Handle & Resource RAII:**
   - Never leak Win32 `HANDLE`, `HKEY`, or COM interface pointers.
   - Use safe RAII wrappers (`windows::Win32::Foundation::HANDLE`, `CloseHandle` in `Drop`, or `windows::core::IUnknown` COM smart pointers).
   - Verify `INVALID_HANDLE_VALUE` and null checks immediately upon receipt.

4. **Specific Win32 APIs:**
   - Display: Use `GetDisplayConfigBufferSizes`, `QueryDisplayConfig`, `SetDisplayConfig` (`Win32_Devices_Display`). Always provide fallback to `SDC_TOPOLOGY_INTERNAL`.
   - Power: Use `PowerSetActiveScheme`, `PowerReadACValueIndex`, `PowerWriteACValueIndex` with `GUID_SYSTEM_BUTTON_SUBGROUP` and `GUID_LIDCLOSE_ACTION`. Use `SetThreadExecutionState` during active sessions.
   - Trim: Use `SetProcessInformation` (`ProcessPowerThrottling`) for Tier 0 EcoQoS. Resolve `NtSuspendProcess`/`NtResumeProcess` dynamically from `ntdll.dll` for Tier 1 with PID + CreationTime verification.
   - Taskbar: Find `Shell_TrayWnd` and `Shell_SecondaryTrayWnd` and use `ShowWindow(..., SW_HIDE)` and `SW_SHOW`.
   - Audio: Enumerate endpoints via `IMMDeviceEnumerator`, use `IPolicyConfig` COM interface, and handle COM failures non-fatally.
