// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! FR-STATE-21: event-driven state watcher via persistent SSH channel.
//!
//! Opens a long-running SSH channel on the server that watches `~/.shellkeep/shared.json`
//! for changes (inotifywait with stat-poll fallback). On change, emits the file contents
//! framed by STATE_BEGIN/STATE_END delimiters.

use std::sync::Arc;

use iced::futures::SinkExt;
use iced::futures::stream::BoxStream;
use shellkeep::ssh::manager::{ConnKey, ConnectionManager};
use tokio::sync::Mutex;

use super::message::Message;

/// State passed to the watcher subscription.
#[derive(Clone)]
pub(crate) struct StateWatcherData {
    pub conn_key: ConnKey,
    pub conn_manager: Arc<Mutex<ConnectionManager>>,
}

// Implement Hash/Eq so iced can deduplicate subscriptions
impl std::hash::Hash for StateWatcherData {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.conn_key.hash(state);
        "state-watcher".hash(state);
    }
}

impl PartialEq for StateWatcherData {
    fn eq(&self, other: &Self) -> bool {
        self.conn_key == other.conn_key
    }
}

impl Eq for StateWatcherData {}

/// Shell script that watches shared.json for changes.
/// - First line: WATCHER_MODE=inotify|poll (for client to show sync indicator)
/// - On each change: STATE_BEGIN\n<json>\nSTATE_END
const WATCHER_SCRIPT: &str = r#"
STATE="$HOME/.shellkeep/shared.json"
DIR="$HOME/.shellkeep"
mkdir -p "$DIR"

emit() { printf 'STATE_BEGIN\n'; cat "$STATE" 2>/dev/null; printf '\nSTATE_END\n'; }

if command -v inotifywait >/dev/null 2>&1; then
    printf 'WATCHER_MODE=inotify\n'
    emit
    inotifywait -m -q -e moved_to,close_write "$DIR" --format '%f' 2>/dev/null |
    while IFS= read -r f; do
        [ "$f" = "shared.json" ] && emit
    done
else
    printf 'WATCHER_MODE=poll\n'
    emit
    last_mtime=0
    while sleep 2; do
        mtime=$(stat -c %Y "$STATE" 2>/dev/null || stat -f %m "$STATE" 2>/dev/null || echo 0)
        if [ "$mtime" != "$last_mtime" ]; then
            last_mtime="$mtime"
            emit
        fi
    done
fi
"#;

/// Create the state watcher stream for use with `Subscription::run_with`.
pub(crate) fn state_watcher_stream(data: &StateWatcherData) -> BoxStream<'static, Message> {
    let conn_key = data.conn_key.clone();
    let mgr = data.conn_manager.clone();

    Box::pin(iced::stream::channel(100, async move |mut output| {
        // Get SSH handle
        let handle_arc = {
            let m = mgr.lock().await;
            match m.get_cached(&conn_key) {
                Some(h) => h.clone(),
                None => {
                    tracing::warn!("state watcher: no cached connection");
                    iced::futures::future::pending::<()>().await;
                    return;
                }
            }
        };

        // Open channel and exec watcher script
        let mut channel = {
            let h = handle_arc.lock().await;
            match h.channel_open_session().await {
                Ok(ch) => ch,
                Err(e) => {
                    tracing::warn!("state watcher: failed to open channel: {e}");
                    let _ = output.send(Message::WatcherDisconnected).await;
                    return;
                }
            }
        };

        // Execute the watcher script via sh
        if let Err(e) = channel.exec(true, WATCHER_SCRIPT).await {
            tracing::warn!("state watcher: exec failed: {e}");
            let _ = output.send(Message::WatcherDisconnected).await;
            return;
        }

        tracing::info!("state watcher: channel opened, waiting for events");

        let mut buffer = String::new();
        let mut in_state_block = false;
        let mut state_content = String::new();

        loop {
            match channel.wait().await {
                Some(russh::ChannelMsg::Data { data }) => {
                    let chunk = String::from_utf8_lossy(&data);
                    buffer.push_str(&chunk);

                    // Process complete lines
                    while let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer[..newline_pos].to_string();
                        buffer = buffer[newline_pos + 1..].to_string();

                        if line.starts_with("WATCHER_MODE=") {
                            let mode = line.trim_start_matches("WATCHER_MODE=").to_string();
                            tracing::info!("state watcher mode: {mode}");
                            let _ = output.send(Message::WatcherMode(mode)).await;
                        } else if line == "STATE_BEGIN" {
                            in_state_block = true;
                            state_content.clear();
                        } else if line == "STATE_END" {
                            in_state_block = false;
                            if !state_content.is_empty() {
                                match serde_json::from_str::<
                                    shellkeep::state::state_file::SharedState,
                                >(&state_content)
                                {
                                    Ok(state) => {
                                        let _ = output
                                            .send(Message::RemoteStateChanged(Box::new(state)))
                                            .await;
                                    }
                                    Err(e) => {
                                        tracing::warn!("state watcher: failed to parse state: {e}");
                                    }
                                }
                            }
                        } else if in_state_block {
                            if !state_content.is_empty() {
                                state_content.push('\n');
                            }
                            state_content.push_str(&line);
                        }
                    }
                }
                Some(russh::ChannelMsg::Eof) | None => {
                    tracing::info!("state watcher: channel closed");
                    let _ = output.send(Message::WatcherDisconnected).await;
                    break;
                }
                _ => {}
            }
        }
    }))
}
