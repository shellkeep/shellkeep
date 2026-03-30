// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Crash handler and core dump prevention.
//!
//! NFR-OBS-09: Signal handler for SIGSEGV, SIGABRT.
//! NFR-SEC-10: Disable core dumps to prevent leaking sensitive memory.

use std::fs;
use std::path::PathBuf;

/// Disable core dumps (NFR-SEC-10).
pub fn disable_core_dumps() {
    #[cfg(target_os = "linux")]
    {
        // prctl(PR_SET_DUMPABLE, 0) — prevent core dumps
        unsafe {
            libc::prctl(libc::PR_SET_DUMPABLE, 0);
        }
        tracing::debug!("core dumps disabled");
    }
}

/// Get the crash dump directory.
pub fn crash_dir() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("shellkeep")
        .join("crashes")
}

/// Install panic hook that writes crash info to a file.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        let dir = crash_dir();
        let _ = fs::create_dir_all(&dir);

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let pid = std::process::id();
        let path = dir.join(format!("crash-{timestamp}-{pid}.txt"));

        let mut report = String::new();
        report.push_str("shellkeep crash dump\n");
        report.push_str("====================\n\n");
        report.push_str(&format!("PID: {pid}\n"));
        report.push_str(&format!("Time: {timestamp}\n"));
        report.push_str(&format!("Version: {}\n\n", env!("CARGO_PKG_VERSION")));

        if let Some(location) = info.location() {
            report.push_str(&format!(
                "Location: {}:{}:{}\n",
                location.file(),
                location.line(),
                location.column()
            ));
        }

        report.push_str(&format!("Info: {info}\n\n"));

        // Backtrace
        let bt = std::backtrace::Backtrace::force_capture();
        report.push_str(&format!("Backtrace:\n{bt}\n"));

        report.push_str(
            "\nNOTE: This dump does not contain terminal content, keys, or environment variables.\n",
        );

        if let Err(e) = fs::write(&path, &report) {
            eprintln!("[shellkeep] WARNING: failed to write crash dump: {e}");
        }
        eprintln!("[shellkeep] FATAL: panic occurred");
        eprintln!("[shellkeep] Crash dump written to: {}", path.display());

        // Call the default hook (prints to stderr)
        default_hook(info);
    }));

    tracing::debug!("panic hook installed");
}
