# Research Findings: Windows 11 Xbox Mode / Full Screen Experience (FSE)

*Completed: 2 Nov 2026 | Project: Kiln | Lead Architect*

---

## 1. Overview & Capabilities

Microsoft introduced the **Full Screen Experience (FSE)**—commonly referred to in Windows 11 updates (24H2 / 25H2) as **Xbox Mode**—primarily targeting handheld gaming PCs (e.g., ASUS ROG Ally, Lenovo Legion Go) and expanding to standard PCs and laptops.

### Key Capabilities Verified:
- **Startup Experience:** When enabled, Windows bypasses the traditional Windows Explorer desktop upon startup and boots directly into a controller-friendly, full-screen gaming interface.
- **Background Resource Optimization:** Microsoft reports that bypassing Explorer and non-essential shell background components can free up to ~2 GB of system memory and reduce background CPU contention.
- **Entry Shortcuts:** Users can enter or toggle the experience via `Win + F11`, Game Bar (`Win + G`), Task View, or configure it as the default startup shell.

---

## 2. Limitations & Differentiators for Kiln

| Feature | Windows 11 Native Xbox Mode | Kiln Console Shell |
| :--- | :--- | :--- |
| **Default Home App** | Restricted to the Xbox App (or certified OEM frontends on handhelds). | Custom lightweight Rust 10-foot shell (`kiln-shell`). |
| **Third-Party Launchers** | Does not manage launchers; games from Steam, Epic, GOG must either be manually imported into Xbox app or run with launchers kept active in background. | Scans games directly from disk/manifests/registry. Launchers start on demand and close on game exit. |
| **Display Switching** | Displays in current desktop topology. Does not switch to TV-only or handle lid-close actions. | Dedicated TV-only topology (`SDC_TOPOLOGY_EXTERNAL`) with 15-second auto-revert timer and HDMI unplug auto-recovery (<3s). |
| **Lid & Power** | Standard Windows power plans remain active. | Automatically applies High/Ultimate Performance, sets lid close to "Do Nothing", and restores on exit. |
| **Process Trimming** | System-managed generic background throttling without granular user control. | Journaled 3-tier trimming (Tier 0 EcoQoS, Tier 1 `NtSuspendProcess`) with allowlists, dry-run previews, and guaranteed resume. |
| **Public Extensibility API** | None. No public Win32 or GDK API exists for third-party launchers or shells to register as the official "Home App" in Settings. | Standalone user-space architecture requiring no system hacks. |

---

## 3. Potential Integration & Phase 2 Path

While Microsoft does not provide a public API for designating arbitrary applications as the Full Screen Experience startup application, the community has documented startup tricks:
1. **Startup Chaining:** Leveraging Xbox Mode's startup trigger to immediately spawn a custom shell (`kiln-shell.exe`) while terminating the background Xbox app.
2. **Dedicated Console Profile (`KilnConsole`):** A standard non-admin Windows account that auto-starts `kiln.exe` upon logon with Explorer hidden or replaced in `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Winlogon\Shell`.

**Conclusion:** Kiln's value proposition is confirmed. Kiln provides true launcher independence, automated TV-only switching, lid-close gaming, and journaled memory trimming without being tied to Microsoft's Xbox ecosystem.
