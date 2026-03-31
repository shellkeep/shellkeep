// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Network utility functions (gateway detection, etc.).

/// FR-RECONNECT-08: read default gateway from /proc/net/route (Linux only).
///
/// Returns the hex-encoded gateway address for the default route, or `None`
/// if no default route is found.
#[cfg(target_os = "linux")]
pub fn read_default_gateway() -> Option<String> {
    let content = std::fs::read_to_string("/proc/net/route").ok()?;
    // Each line: Iface Destination Gateway Flags RefCnt Use Metric Mask MTU Window IRTT
    // Default route has destination 00000000
    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 3 && fields[1] == "00000000" {
            return Some(fields[2].to_string());
        }
    }
    None
}
