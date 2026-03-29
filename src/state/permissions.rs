// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! File and directory permission enforcement.
//!
//! NFR-SEC-02: Config/data dirs 0700, files 0600.
//! NFR-SEC-03: Auto-correct on startup.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// Ensure a directory has 0700 permissions (owner only).
pub fn ensure_dir_permissions(path: &Path) {
    #[cfg(unix)]
    if path.exists()
        && let Ok(meta) = std::fs::metadata(path)
    {
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o700 {
            tracing::debug!(
                "fixing dir permissions on {}: {:o} -> 700",
                path.display(),
                mode
            );
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
        }
    }
}

/// Ensure a file has 0600 permissions (owner read/write only).
pub fn ensure_file_permissions(path: &Path) {
    #[cfg(unix)]
    if path.exists()
        && let Ok(meta) = std::fs::metadata(path)
    {
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o600 {
            tracing::debug!(
                "fixing file permissions on {}: {:o} -> 600",
                path.display(),
                mode
            );
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

/// Verify and fix permissions on all shellkeep directories.
pub fn verify_and_fix() {
    if let Some(config_dir) = dirs::config_dir() {
        let sk_dir = config_dir.join("shellkeep");
        ensure_dir_permissions(&sk_dir);
    }

    if let Some(data_dir) = dirs::data_dir() {
        let sk_dir = data_dir.join("shellkeep");
        ensure_dir_permissions(&sk_dir);
        ensure_dir_permissions(&sk_dir.join("state"));

        // Fix permissions on recent.json
        ensure_file_permissions(&sk_dir.join("recent.json"));
    }

    if let Some(state_dir) = dirs::state_dir() {
        let sk_dir = state_dir.join("shellkeep");
        ensure_dir_permissions(&sk_dir);
    }
}
