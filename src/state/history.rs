// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Session history — JSONL reading, writing, reconstruction, and retention.
//!
//! Each session's terminal output is stored as one `.jsonl` file per session UUID
//! in `$XDG_DATA_HOME/shellkeep/history/`.
//!
//! - Server-side: `tmux pipe-pane` captures raw output (FR-HISTORY-01)
//! - Client-side: `HistoryWriter` writes structured JSONL (FR-HISTORY-02..04)
//! - Rotation: per-file truncation + total directory size limit (FR-HISTORY-05..07)

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;

/// Maximum single-file size before rotation (50 MB).
const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// FR-HISTORY-01: a single JSONL history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// ISO 8601 timestamp
    pub ts: String,
    /// Event type
    #[serde(rename = "type")]
    pub event_type: HistoryEventType,
    /// Terminal text (present for Output events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Terminal size (present for Resize events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<(u32, u32)>,
}

/// FR-HISTORY-02: event types stored in history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoryEventType {
    Output,
    Start,
    End,
    Reconnect,
    Resize,
    Meta,
}

/// Directory where history JSONL files are stored.
pub fn history_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("shellkeep/history")
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// FR-HISTORY-09: read JSONL file, discarding invalid/truncated lines.
pub fn read_history(session_uuid: &str) -> Vec<HistoryEntry> {
    let path = history_dir().join(format!("{session_uuid}.jsonl"));
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<HistoryEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(_) => {
                // FR-HISTORY-09: discard invalid lines (crash recovery)
                tracing::debug!("skipping invalid JSONL line in {}", path.display());
            }
        }
    }
    entries
}

/// FR-UI-09..10: reconstruct terminal output text from history entries.
/// Returns None if no history file exists, Some("") if file exists but has no output.
pub fn reconstruct_output(session_uuid: &str) -> Option<String> {
    let path = history_dir().join(format!("{session_uuid}.jsonl"));
    if !path.exists() {
        return None;
    }

    let entries = read_history(session_uuid);
    let output: String = entries
        .iter()
        .filter(|e| e.event_type == HistoryEventType::Output)
        .filter_map(|e| e.text.as_deref())
        .collect();
    Some(output)
}

// ---------------------------------------------------------------------------
// Writing — client-side JSONL capture (FR-HISTORY-02..08)
// ---------------------------------------------------------------------------

/// Writes session history to a local JSONL file.
pub struct HistoryWriter {
    session_uuid: String,
    file: Option<fs::File>,
    bytes_written: u64,
    max_total_mb: u32,
}

impl HistoryWriter {
    /// Create a new writer for the given session UUID.
    /// Returns `None` if history is disabled (max_size_mb == 0).
    pub fn new(session_uuid: &str, max_size_mb: u32) -> Option<Self> {
        if max_size_mb == 0 {
            return None;
        }
        let dir = history_dir();
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(format!("{session_uuid}.jsonl"));
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();
        let bytes_written = file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map(|m| m.len())
            .unwrap_or(0);
        Some(Self {
            session_uuid: session_uuid.to_string(),
            file,
            bytes_written,
            max_total_mb: max_size_mb,
        })
    }

    /// FR-HISTORY-02: append terminal output data.
    pub fn append_output(&mut self, data: &[u8]) {
        self.write_entry(HistoryEntry {
            ts: chrono::Utc::now().to_rfc3339(),
            event_type: HistoryEventType::Output,
            text: Some(String::from_utf8_lossy(data).into_owned()),
            size: None,
        });
    }

    /// FR-HISTORY-04: append resize event.
    pub fn append_resize(&mut self, cols: u32, rows: u32) {
        self.write_entry(HistoryEntry {
            ts: chrono::Utc::now().to_rfc3339(),
            event_type: HistoryEventType::Resize,
            text: None,
            size: Some((cols, rows)),
        });
    }

    /// FR-HISTORY-03: append meta event (connect, disconnect, etc.).
    pub fn append_meta(&mut self, message: &str) {
        self.write_entry(HistoryEntry {
            ts: chrono::Utc::now().to_rfc3339(),
            event_type: HistoryEventType::Meta,
            text: Some(message.to_string()),
            size: None,
        });
    }

    fn write_entry(&mut self, entry: HistoryEntry) {
        let Some(ref mut f) = self.file else { return };
        let Ok(line) = serde_json::to_string(&entry) else {
            return;
        };
        if writeln!(f, "{line}").is_ok() {
            self.bytes_written += line.len() as u64 + 1;
        }
        // FR-HISTORY-05: rotate if single file exceeds limit
        if self.bytes_written > MAX_FILE_BYTES {
            self.rotate();
        }
    }

    /// FR-HISTORY-05..06: truncate oldest 25% of current file.
    fn rotate(&mut self) {
        let dir = history_dir();
        let path = dir.join(format!("{}.jsonl", self.session_uuid));
        let Ok(content) = fs::read_to_string(&path) else {
            return;
        };
        let lines: Vec<&str> = content.lines().collect();
        let keep_from = lines.len() / 4; // drop oldest 25%
        let kept = lines[keep_from..].join("\n");
        let tmp = path.with_extension("jsonl.tmp");
        if fs::write(&tmp, &kept).is_ok() && fs::rename(&tmp, &path).is_ok() {
            self.bytes_written = kept.len() as u64;
        }
        // FR-HISTORY-07: enforce total directory size limit
        self.enforce_total_limit();
    }

    /// FR-HISTORY-07: delete oldest history files if total exceeds limit.
    fn enforce_total_limit(&self) {
        let dir = history_dir();
        let max_bytes = u64::from(self.max_total_mb) * 1024 * 1024;
        let Ok(entries) = fs::read_dir(&dir) else {
            return;
        };
        let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext == "jsonl" || ext == "raw")
            })
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                let mtime = meta.modified().ok()?;
                Some((e.path(), meta.len(), mtime))
            })
            .collect();

        let total: u64 = files.iter().map(|(_, sz, _)| sz).sum();
        if total <= max_bytes {
            return;
        }

        // Sort oldest first
        files.sort_by_key(|(_, _, t)| *t);
        let mut freed: u64 = 0;
        let excess = total - max_bytes;
        for (path, size, _) in &files {
            if freed >= excess {
                break;
            }
            let _ = fs::remove_file(path);
            freed += size;
        }
    }
}

// ---------------------------------------------------------------------------
// Server-side capture (FR-HISTORY-01)
// ---------------------------------------------------------------------------

/// Build the tmux pipe-pane command for server-side capture.
pub fn pipe_pane_command(tmux_session: &str, session_uuid: &str) -> String {
    format!(
        "mkdir -p ~/.terminal-state/history && \
         tmux pipe-pane -t {tmux_session} \
         'cat >> ~/.terminal-state/history/{session_uuid}.raw'"
    )
}

// ---------------------------------------------------------------------------
// Retention (FR-HISTORY-11)
// ---------------------------------------------------------------------------

/// Delete history files older than `max_days`.
pub fn cleanup_old_history(max_days: u32) {
    let dir = history_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let cutoff =
        std::time::SystemTime::now() - std::time::Duration::from_secs(u64::from(max_days) * 86400);

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "jsonl")
            && let Ok(meta) = path.metadata()
            && let Ok(modified) = meta.modified()
            && modified < cutoff
        {
            tracing::info!("removing old history file: {}", path.display());
            let _ = fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_entries() {
        let lines = r#"{"ts":"2026-03-29T10:00:00Z","type":"start","text":null}
{"ts":"2026-03-29T10:00:01Z","type":"output","text":"hello world"}
{"ts":"2026-03-29T10:00:02Z","type":"output","text":"\n$ "}
{"ts":"2026-03-29T10:00:03Z","type":"end","text":null}"#;

        let mut entries = Vec::new();
        for line in lines.lines() {
            if let Ok(entry) = serde_json::from_str::<HistoryEntry>(line) {
                entries.push(entry);
            }
        }
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].event_type, HistoryEventType::Start);
        assert_eq!(entries[1].text.as_deref(), Some("hello world"));
    }

    #[test]
    fn skip_truncated_lines() {
        let lines = r#"{"ts":"2026-03-29T10:00:00Z","type":"output","text":"good"}
{"ts":"2026-03-29T10:00:01Z","type":"outp
{"ts":"2026-03-29T10:00:02Z","type":"output","text":"also good"}"#;

        let mut entries = Vec::new();
        for line in lines.lines() {
            if let Ok(entry) = serde_json::from_str::<HistoryEntry>(line) {
                entries.push(entry);
            }
        }
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text.as_deref(), Some("good"));
        assert_eq!(entries[1].text.as_deref(), Some("also good"));
    }

    #[test]
    fn reconstruct_output_text() {
        let lines = vec![
            HistoryEntry {
                ts: "2026-03-29T10:00:00Z".into(),
                event_type: HistoryEventType::Start,
                text: None,
                size: None,
            },
            HistoryEntry {
                ts: "2026-03-29T10:00:01Z".into(),
                event_type: HistoryEventType::Output,
                text: Some("$ ls\n".into()),
                size: None,
            },
            HistoryEntry {
                ts: "2026-03-29T10:00:02Z".into(),
                event_type: HistoryEventType::Output,
                text: Some("file.txt\n".into()),
                size: None,
            },
            HistoryEntry {
                ts: "2026-03-29T10:00:03Z".into(),
                event_type: HistoryEventType::End,
                text: None,
                size: None,
            },
        ];

        let output: String = lines
            .iter()
            .filter(|e| e.event_type == HistoryEventType::Output)
            .filter_map(|e| e.text.as_deref())
            .collect();
        assert_eq!(output, "$ ls\nfile.txt\n");
    }

    #[test]
    fn cleanup_no_panic_on_empty_dir() {
        cleanup_old_history(90);
    }

    #[test]
    fn history_entry_serialization() {
        let entry = HistoryEntry {
            ts: "2026-03-29T12:00:00Z".to_string(),
            event_type: HistoryEventType::Output,
            text: Some("hello\n".to_string()),
            size: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains(r#""type":"output"#));
        assert!(json.contains(r#""text":"hello\n"#));
        // size should be omitted when None
        assert!(!json.contains("size"));
    }

    #[test]
    fn resize_entry_serialization() {
        let entry = HistoryEntry {
            ts: "2026-03-29T12:00:00Z".to_string(),
            event_type: HistoryEventType::Resize,
            text: None,
            size: Some((80, 24)),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains(r#""type":"resize"#));
        assert!(json.contains("[80,24]"));
        assert!(!json.contains("text"));
    }

    #[test]
    fn writer_disabled_when_zero() {
        assert!(HistoryWriter::new("test-uuid", 0).is_none());
    }

    #[test]
    fn writer_writes_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.jsonl");
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();
        let mut writer = HistoryWriter {
            session_uuid: "test".to_string(),
            file,
            bytes_written: 0,
            max_total_mb: 500,
        };
        writer.append_output(b"hello world");
        writer.append_meta("connected");
        writer.append_resize(80, 24);
        drop(writer);

        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("output"));
        assert!(lines[1].contains("meta"));
        assert!(lines[2].contains("resize"));
    }

    #[test]
    fn pipe_pane_command_format() {
        let cmd = pipe_pane_command("sk-abc123", "uuid-456");
        assert!(cmd.contains("tmux pipe-pane -t sk-abc123"));
        assert!(cmd.contains("uuid-456.raw"));
        assert!(cmd.contains("mkdir -p"));
    }
}
