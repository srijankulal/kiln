---
name: kiln-safety-journal
description: Enforces write-ahead journaling (session.json atomic write + fsync before any Win32 mutation), idempotent rollback patterns, and proptest failure-injection testing in Kiln.
---

# Kiln Safety & Journaling Guidelines

Safety and recovery are the highest priorities in Kiln. The shell runs on real user hardware, modifying display configurations, power schemes, taskbar visibility, and background processes. A bug or crash must never lock the user out of their display or leave background apps suspended.

## Core Rules

1. **Write-Ahead Journaling (WAL):**
   - No Windows API mutation (display topology, lid action, power scheme, process EcoQoS/suspension, taskbar hiding) may occur without first persisting the change to `%LOCALAPPDATA%\Kiln\session.json`.
   - Journal updates MUST be atomic:
     1. Serialize JSON to a temporary file (`session.json.tmp`).
     2. Call `FlushFileBuffers` (or Rust `sync_all()`) to commit bytes to disk.
     3. Atomically rename `session.json.tmp` over `session.json` (`MoveFileExW` with `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`).

2. **Strict Invariant: Read from Disk on Restore:**
   - The restore logic (`kiln --restore` or panic hook or watchdog) MUST read directly from `session.json` on disk, NEVER from in-memory state.
   - If memory is corrupted or a thread panicked, disk state remains the single source of truth.

3. **Idempotence & Partial Failure Recovery:**
   - Every restore action must be completely idempotent. Calling `restore()` twice in a row must succeed with no side effects.
   - If restoring one item fails (e.g., a suspended process already exited), log a warning and proceed immediately to restore the remaining items. Never abort the restore chain on a single step failure.

4. **15-Second Display Confirmation Timer:**
   - When switching to TV-only (`SDC_TOPOLOGY_EXTERNAL`), start a 15-second countdown timer.
   - If the user does not explicitly confirm via controller, keyboard, or mouse, revert automatically to internal display (`SDC_TOPOLOGY_INTERNAL`).

5. **Anti-Cheat & System Protection Never-Touch List:**
   - Hardcoded in code: Never inspect, open, suspend, or terminate system processes (`System`, `csrss.exe`, `dwm.exe`, `lsass.exe`, `services.exe`) or any anti-cheat process (`EasyAntiCheat*`, `BEService*`, `vgc.exe`, `vgk.sys`, `zksvc.exe`).

6. **L1 Proptest Failure Injection:**
   - All state transitions and rollback logic must be tested against simulated failures at every single execution step using `proptest`.
   - Property: For any arbitrary sequence of partial steps and injected errors, calling `restore()` on `FakeSystem` must return it to identical initial state.
