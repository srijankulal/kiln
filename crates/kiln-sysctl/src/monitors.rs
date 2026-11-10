//! Read-only display detection: which screens are on, their names, positions and sizes.
//!
//! Nothing here changes Windows display settings. Kiln uses this to place its own window
//! on the screen the user picks.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorInfo {
    /// Friendly name from the display's EDID (e.g. "LG TV SSCR2"), or a generic fallback.
    pub name: String,
    /// Built-in laptop panel.
    pub internal: bool,
    /// Windows primary display (desktop origin).
    pub primary: bool,
    /// Top-left corner in virtual-desktop physical pixels.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayScan {
    /// Displays that are currently on (part of the desktop).
    pub active: Vec<MonitorInfo>,
    /// External displays that are plugged in but turned off in Windows (e.g. "PC screen only").
    pub inactive_external: usize,
}

impl DisplayScan {
    pub fn external(&self) -> impl Iterator<Item = &MonitorInfo> {
        self.active.iter().filter(|m| !m.internal)
    }

    pub fn internal(&self) -> Option<&MonitorInfo> {
        self.active
            .iter()
            .find(|m| m.internal)
            .or_else(|| self.active.iter().find(|m| m.primary))
    }
}

pub fn scan_displays() -> DisplayScan {
    #[cfg(windows)]
    {
        windows_impl::scan()
    }
    #[cfg(not(windows))]
    {
        DisplayScan {
            active: vec![MonitorInfo {
                name: "Built-in display".into(),
                internal: true,
                primary: true,
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            }],
            inactive_external: 0,
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::{DisplayScan, MonitorInfo};
    use std::collections::HashSet;
    use windows::Win32::Devices::Display::{
        DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
        DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_MODE_INFO,
        DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
        DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED,
        DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_TARGET_DEVICE_NAME,
        DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY, QDC_ALL_PATHS, QDC_ONLY_ACTIVE_PATHS,
        QUERY_DISPLAY_CONFIG_FLAGS,
    };
    use windows::Win32::Foundation::LUID;

    const PATH_ACTIVE: u32 = 0x1;

    fn query(
        flags: QUERY_DISPLAY_CONFIG_FLAGS,
    ) -> Option<(Vec<DISPLAYCONFIG_PATH_INFO>, Vec<DISPLAYCONFIG_MODE_INFO>)> {
        // SAFETY: buffers are sized from GetDisplayConfigBufferSizes and truncated to the
        // counts QueryDisplayConfig reports back.
        unsafe {
            let (mut np, mut nm) = (0u32, 0u32);
            GetDisplayConfigBufferSizes(flags, &mut np, &mut nm)
                .ok()
                .ok()?;
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
            QueryDisplayConfig(
                flags,
                &mut np,
                paths.as_mut_ptr(),
                &mut nm,
                modes.as_mut_ptr(),
                None,
            )
            .ok()
            .ok()?;
            paths.truncate(np as usize);
            modes.truncate(nm as usize);
            Some((paths, modes))
        }
    }

    fn is_internal(tech: DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY) -> bool {
        tech == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
            || tech == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED
            || tech == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED
    }

    fn target_name(
        adapter: LUID,
        id: u32,
    ) -> Option<(String, DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY)> {
        let mut req = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
        req.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
        req.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
        req.header.adapterId = adapter;
        req.header.id = id;
        // SAFETY: header type/size match the DISPLAYCONFIG_TARGET_DEVICE_NAME request packet.
        let status = unsafe { DisplayConfigGetDeviceInfo(&mut req.header) };
        if status != 0 {
            return None;
        }
        let len = req
            .monitorFriendlyDeviceName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(req.monitorFriendlyDeviceName.len());
        Some((
            String::from_utf16_lossy(&req.monitorFriendlyDeviceName[..len]),
            req.outputTechnology,
        ))
    }

    fn key(adapter: LUID, id: u32) -> (u32, i32, u32) {
        (adapter.LowPart, adapter.HighPart, id)
    }

    pub fn scan() -> DisplayScan {
        let mut scan = DisplayScan::default();
        let mut active_targets = HashSet::new();

        if let Some((paths, modes)) = query(QDC_ONLY_ACTIVE_PATHS) {
            for path in &paths {
                let t = &path.targetInfo;
                active_targets.insert(key(t.adapterId, t.id));
                let (name, tech) =
                    target_name(t.adapterId, t.id).unwrap_or((String::new(), t.outputTechnology));
                let internal = is_internal(tech);
                // SAFETY: modeInfoIdx is the plain index variant for paths returned without
                // the virtual-mode flag; it is bounds- and type-checked before use.
                let idx = unsafe { path.sourceInfo.Anonymous.modeInfoIdx } as usize;
                let Some(mode) = modes.get(idx) else { continue };
                if mode.infoType != DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE {
                    continue;
                }
                let src = unsafe { mode.Anonymous.sourceMode };
                scan.active.push(MonitorInfo {
                    name: if !name.is_empty() {
                        name
                    } else if internal {
                        "Built-in display".into()
                    } else {
                        "External display".into()
                    },
                    internal,
                    primary: src.position.x == 0 && src.position.y == 0,
                    x: src.position.x,
                    y: src.position.y,
                    width: src.width,
                    height: src.height,
                });
            }
        }

        if let Some((paths, _)) = query(QDC_ALL_PATHS) {
            let mut seen = HashSet::new();
            for path in &paths {
                let t = &path.targetInfo;
                let k = key(t.adapterId, t.id);
                if path.flags & PATH_ACTIVE != 0
                    || !t.targetAvailable.as_bool()
                    || active_targets.contains(&k)
                    || !seen.insert(k)
                {
                    continue;
                }
                let tech = target_name(t.adapterId, t.id)
                    .map(|(_, tech)| tech)
                    .unwrap_or(t.outputTechnology);
                if !is_internal(tech) {
                    scan.inactive_external += 1;
                }
            }
        }

        // Clone mode puts several targets on one desktop area; keep one entry per position.
        let mut positions = HashSet::new();
        scan.active
            .retain(|m| positions.insert((m.x, m.y, m.width, m.height, m.internal)));
        scan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_is_read_only_and_reports_at_least_one_screen_when_available() {
        let scan = scan_displays();
        for m in &scan.active {
            assert!(m.width > 0 && m.height > 0);
            assert!(!m.name.is_empty());
        }
    }

    #[test]
    fn internal_falls_back_to_primary() {
        let scan = DisplayScan {
            active: vec![MonitorInfo {
                name: "Monitor".into(),
                internal: false,
                primary: true,
                x: 0,
                y: 0,
                width: 2560,
                height: 1440,
            }],
            inactive_external: 0,
        };
        assert_eq!(scan.internal().map(|m| m.width), Some(2560));
        assert_eq!(scan.external().count(), 1);
    }
}
