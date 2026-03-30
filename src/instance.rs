// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

pub(crate) struct PidGuard {
    path: std::path::PathBuf,
}

impl Drop for PidGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Check if another instance is running. Returns a PidGuard on success
/// or None if another instance holds the PID file.
pub(crate) fn check_single_instance() -> Option<PidGuard> {
    let runtime_dir = dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("shellkeep");
    let _ = std::fs::create_dir_all(&runtime_dir);
    let pid_path = runtime_dir.join("shellkeep.pid");

    if pid_path.exists()
        && let Ok(pid_str) = std::fs::read_to_string(&pid_path)
        && let Ok(pid) = pid_str.trim().parse::<u32>()
    {
        #[cfg(unix)]
        if std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return None;
        }
        #[cfg(windows)]
        {
            // On Windows, check if PID file is very recent as a heuristic
            if let Ok(meta) = std::fs::metadata(&pid_path) {
                if let Ok(modified) = meta.modified() {
                    if modified.elapsed().unwrap_or_default() < std::time::Duration::from_secs(5) {
                        return None;
                    }
                }
            }
        }
    }

    if let Err(e) = std::fs::write(&pid_path, std::process::id().to_string()) {
        tracing::warn!("failed to write PID file: {e}");
    }

    Some(PidGuard { path: pid_path })
}
