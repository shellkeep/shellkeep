// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! E2E tests for state persistence, SFTP, tmux lifecycle, lock mechanism,
//! workspace isolation, and session reconciliation against real SSH server.
//!
//! Run with: cargo test --test e2e_state -- --ignored --test-threads=1

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use tokio::sync::Mutex;

use shellkeep::ssh::connection::{SshHandler, exec_command};
use shellkeep::ssh::lock;
use shellkeep::ssh::sftp::{StateSyncer, open_sftp, read_file, write_file_atomic};
use shellkeep::ssh::tmux;
use shellkeep::state::state_file::{
    DeviceState, HiddenTabState, HiddenWindowState, SharedState, TabState, WindowGeometry,
    Workspace,
};

fn ssh_key_path() -> String {
    dirs::home_dir()
        .map(|h| h.join(".ssh").join("id_shellkeep").display().to_string())
        .unwrap_or_else(|| "/root/.ssh/id_shellkeep".to_string())
}
const SSH_HOST: &str = "209.38.150.61";
const SSH_PORT: u16 = 22;
const SSH_USER: &str = "root";

async fn connect() -> russh::client::Handle<SshHandler> {
    let key = ssh_key_path();
    shellkeep::ssh::connection::connect(SSH_HOST, SSH_PORT, SSH_USER, Some(&key), None, 15)
        .await
        .expect("failed to connect")
        .handle
}

async fn exec(handle: &russh::client::Handle<SshHandler>, cmd: &str) -> String {
    exec_command(handle, cmd).await.unwrap_or_default()
}

async fn cleanup_tmux(handle: &russh::client::Handle<SshHandler>, prefix: &str) {
    let sessions = exec(
        handle,
        "tmux list-sessions -F '#{session_name}' 2>/dev/null",
    )
    .await;
    for name in sessions.lines() {
        let name = name.trim();
        if name.starts_with(prefix) {
            let _ = exec(
                handle,
                &format!("tmux kill-session -t '{name}' 2>/dev/null"),
            )
            .await;
        }
    }
}

async fn cleanup_state(handle: &russh::client::Handle<SshHandler>, dir: &str) {
    let _ = exec(handle, &format!("rm -rf {dir}")).await;
}

/// Create an Arc<Mutex<Handle>> for StateSyncer (it requires this wrapper).
fn wrap_handle(
    handle: russh::client::Handle<SshHandler>,
) -> Arc<Mutex<russh::client::Handle<SshHandler>>> {
    Arc::new(Mutex::new(handle))
}

fn test_client_id(suffix: &str) -> String {
    format!("e2e-state-{suffix}")
}

// =========================================================================
// State Persistence
// =========================================================================

#[tokio::test]
#[ignore]
async fn test_state_roundtrip_shared() {
    let handle = connect().await;
    let client_id = test_client_id("rt-shared");
    cleanup_state(&handle, "~/.shellkeep").await;

    let handle_arc = wrap_handle(handle);
    let syncer = StateSyncer::new(handle_arc.clone(), &client_id)
        .await
        .expect("syncer creation failed");
    assert!(syncer.is_sftp(), "expected SFTP transport");

    // Build a SharedState with realistic data
    let mut shared = SharedState::new();
    let ws_uuid = uuid::Uuid::new_v4().to_string();
    let session_uuid = uuid::Uuid::new_v4().to_string();
    let tmux_name = tmux::make_tmux_session_name(&ws_uuid, &session_uuid);
    shared.workspaces.insert(
        "TestWorkspace".to_string(),
        Workspace {
            name: "TestWorkspace".to_string(),
            uuid: ws_uuid.clone(),
            tabs: vec![TabState {
                session_uuid: session_uuid.clone(),
                tmux_session_name: tmux_name.clone(),
                title: "My Tab".to_string(),
                position: 0,
                server_window_id: Some("win-001".to_string()),
                updated_at: String::new(),
            }],
            updated_at: String::new(),
        },
    );
    shared.last_workspace = Some("TestWorkspace".to_string());

    // Write
    let json = serde_json::to_string_pretty(&shared).unwrap();
    syncer
        .write_shared_state(&json)
        .await
        .expect("write shared failed");

    // Read back
    let read_json = syncer
        .read_shared_state()
        .await
        .expect("read failed")
        .expect("no data");
    let read: SharedState = serde_json::from_str(&read_json).expect("parse failed");

    assert_eq!(read.last_workspace, Some("TestWorkspace".to_string()));
    let ws = read
        .workspaces
        .get("TestWorkspace")
        .expect("workspace missing");
    assert_eq!(ws.uuid, ws_uuid);
    assert_eq!(ws.tabs.len(), 1);
    assert_eq!(ws.tabs[0].session_uuid, session_uuid);
    assert_eq!(ws.tabs[0].title, "My Tab");
    assert_eq!(ws.tabs[0].server_window_id, Some("win-001".to_string()));

    // Cleanup
    let guard = handle_arc.lock().await;
    cleanup_state(&guard, "~/.shellkeep").await;
}

#[tokio::test]
#[ignore]
async fn test_state_roundtrip_device() {
    let handle = connect().await;
    let client_id = test_client_id("rt-device");
    cleanup_state(&handle, "~/.shellkeep").await;

    let handle_arc = wrap_handle(handle);
    let syncer = StateSyncer::new(handle_arc.clone(), &client_id)
        .await
        .expect("syncer creation failed");

    let mut device = DeviceState::new(&client_id);
    device.window_geometry.insert(
        "win-001".to_string(),
        WindowGeometry {
            x: Some(100),
            y: Some(200),
            width: 900,
            height: 600,
        },
    );
    device.window_geometry.insert(
        "win-002".to_string(),
        WindowGeometry {
            x: Some(500),
            y: Some(300),
            width: 800,
            height: 500,
        },
    );
    device.hidden_sessions = vec!["hidden-uuid-1".to_string(), "hidden-uuid-2".to_string()];
    device.last_active_window = Some("win-001".to_string());

    let json = serde_json::to_string_pretty(&device).unwrap();
    syncer
        .write_device_state(&json)
        .await
        .expect("write device failed");

    let read_json = syncer
        .read_device_state()
        .await
        .expect("read failed")
        .expect("no data");
    let read: DeviceState = serde_json::from_str(&read_json).expect("parse failed");

    assert_eq!(read.client_id, client_id);
    assert_eq!(read.window_geometry.len(), 2);
    assert_eq!(read.window_geometry["win-001"].x, Some(100));
    assert_eq!(read.window_geometry["win-002"].width, 800);
    assert_eq!(read.hidden_sessions.len(), 2);
    assert_eq!(read.last_active_window, Some("win-001".to_string()));

    let guard = handle_arc.lock().await;
    cleanup_state(&guard, "~/.shellkeep").await;
}

#[tokio::test]
#[ignore]
async fn test_state_preserves_other_workspaces() {
    let handle = connect().await;
    let client_id = test_client_id("preserve-ws");
    cleanup_state(&handle, "~/.shellkeep").await;

    let handle_arc = wrap_handle(handle);
    let syncer = StateSyncer::new(handle_arc.clone(), &client_id)
        .await
        .expect("syncer creation failed");

    // Write state with workspace A
    let mut shared = SharedState::new();
    shared.workspaces.insert(
        "WorkspaceA".to_string(),
        Workspace {
            name: "WorkspaceA".to_string(),
            uuid: "uuid-a".to_string(),
            tabs: vec![TabState {
                session_uuid: "tab-a".to_string(),
                tmux_session_name: "shellkeep--uuid-a--tab-a".to_string(),
                title: "Tab A".to_string(),
                position: 0,
                server_window_id: None,
                updated_at: String::new(),
            }],
            updated_at: String::new(),
        },
    );
    let json = serde_json::to_string_pretty(&shared).unwrap();
    syncer.write_shared_state(&json).await.unwrap();

    // Read, add workspace B, write back (simulating read-modify-write)
    let read_json = syncer.read_shared_state().await.unwrap().unwrap();
    let mut read: SharedState = serde_json::from_str(&read_json).unwrap();
    read.workspaces.insert(
        "WorkspaceB".to_string(),
        Workspace {
            name: "WorkspaceB".to_string(),
            uuid: "uuid-b".to_string(),
            tabs: vec![TabState {
                session_uuid: "tab-b".to_string(),
                tmux_session_name: "shellkeep--uuid-b--tab-b".to_string(),
                title: "Tab B".to_string(),
                position: 0,
                server_window_id: None,
                updated_at: String::new(),
            }],
            updated_at: String::new(),
        },
    );
    let json2 = serde_json::to_string_pretty(&read).unwrap();
    syncer.write_shared_state(&json2).await.unwrap();

    // Verify both workspaces present
    let final_json = syncer.read_shared_state().await.unwrap().unwrap();
    let final_state: SharedState = serde_json::from_str(&final_json).unwrap();
    assert!(
        final_state.workspaces.contains_key("WorkspaceA"),
        "WorkspaceA clobbered"
    );
    assert!(
        final_state.workspaces.contains_key("WorkspaceB"),
        "WorkspaceB missing"
    );
    assert_eq!(final_state.workspaces["WorkspaceA"].tabs[0].title, "Tab A");
    assert_eq!(final_state.workspaces["WorkspaceB"].tabs[0].title, "Tab B");

    let guard = handle_arc.lock().await;
    cleanup_state(&guard, "~/.shellkeep").await;
}

#[tokio::test]
#[ignore]
async fn test_state_multi_window_tabs() {
    let handle = connect().await;
    let client_id = test_client_id("multi-win");
    cleanup_state(&handle, "~/.shellkeep").await;

    let handle_arc = wrap_handle(handle);
    let syncer = StateSyncer::new(handle_arc.clone(), &client_id)
        .await
        .expect("syncer creation failed");

    // Create state with 3 tabs across 2 windows
    let mut shared = SharedState::new();
    shared.workspaces.insert(
        "Default".to_string(),
        Workspace {
            name: "Default".to_string(),
            uuid: "ws-uuid".to_string(),
            tabs: vec![
                TabState {
                    session_uuid: "tab-1".to_string(),
                    tmux_session_name: "shellkeep--ws-uuid--tab-1".to_string(),
                    title: "Window1-Tab1".to_string(),
                    position: 0,
                    server_window_id: Some("window-A".to_string()),
                    updated_at: String::new(),
                },
                TabState {
                    session_uuid: "tab-2".to_string(),
                    tmux_session_name: "shellkeep--ws-uuid--tab-2".to_string(),
                    title: "Window1-Tab2".to_string(),
                    position: 1,
                    server_window_id: Some("window-A".to_string()),
                    updated_at: String::new(),
                },
                TabState {
                    session_uuid: "tab-3".to_string(),
                    tmux_session_name: "shellkeep--ws-uuid--tab-3".to_string(),
                    title: "Window2-Tab1".to_string(),
                    position: 0,
                    server_window_id: Some("window-B".to_string()),
                    updated_at: String::new(),
                },
            ],
            updated_at: String::new(),
        },
    );

    let json = serde_json::to_string_pretty(&shared).unwrap();
    syncer.write_shared_state(&json).await.unwrap();

    // Read back and verify grouping
    let read_json = syncer.read_shared_state().await.unwrap().unwrap();
    let read: SharedState = serde_json::from_str(&read_json).unwrap();
    let tabs = &read.workspaces["Default"].tabs;
    assert_eq!(tabs.len(), 3);

    // Group by server_window_id
    let window_a: Vec<_> = tabs
        .iter()
        .filter(|t| t.server_window_id.as_deref() == Some("window-A"))
        .collect();
    let window_b: Vec<_> = tabs
        .iter()
        .filter(|t| t.server_window_id.as_deref() == Some("window-B"))
        .collect();
    assert_eq!(window_a.len(), 2, "window-A should have 2 tabs");
    assert_eq!(window_b.len(), 1, "window-B should have 1 tab");

    let guard = handle_arc.lock().await;
    cleanup_state(&guard, "~/.shellkeep").await;
}

#[tokio::test]
#[ignore]
async fn test_state_hidden_windows_in_shared() {
    let handle = connect().await;
    let client_id = test_client_id("hidden-win");
    cleanup_state(&handle, "~/.shellkeep").await;

    let handle_arc = wrap_handle(handle);
    let syncer = StateSyncer::new(handle_arc.clone(), &client_id)
        .await
        .expect("syncer creation failed");

    let mut shared = SharedState::new();
    shared.hidden_windows = vec![HiddenWindowState {
        server_window_id: "hidden-win-1".to_string(),
        name: "Hidden Window".to_string(),
        workspace: Some("Default".to_string()),
        tabs: vec![HiddenTabState {
            session_uuid: "hidden-tab-1".to_string(),
            tmux_session_name: "shellkeep--ws--hidden-tab-1".to_string(),
            label: "Hidden Tab".to_string(),
        }],
    }];

    let json = serde_json::to_string_pretty(&shared).unwrap();
    syncer.write_shared_state(&json).await.unwrap();

    let read_json = syncer.read_shared_state().await.unwrap().unwrap();
    let read: SharedState = serde_json::from_str(&read_json).unwrap();
    assert_eq!(read.hidden_windows.len(), 1, "hidden windows lost");
    assert_eq!(read.hidden_windows[0].server_window_id, "hidden-win-1");
    assert_eq!(read.hidden_windows[0].tabs.len(), 1);
    assert_eq!(read.hidden_windows[0].tabs[0].label, "Hidden Tab");

    let guard = handle_arc.lock().await;
    cleanup_state(&guard, "~/.shellkeep").await;
}

#[tokio::test]
#[ignore]
async fn test_device_state_isolation() {
    let handle = connect().await;
    cleanup_state(&handle, "~/.shellkeep").await;

    let handle_arc = wrap_handle(handle);
    let client_a = "e2e-device-A";
    let client_b = "e2e-device-B";

    // Create syncers for two different client IDs
    let syncer_a = StateSyncer::new(handle_arc.clone(), client_a)
        .await
        .unwrap();
    let syncer_b = StateSyncer::new(handle_arc.clone(), client_b)
        .await
        .unwrap();

    // Write different device states
    let mut device_a = DeviceState::new(client_a);
    device_a.window_geometry.insert(
        "win-a".to_string(),
        WindowGeometry {
            x: Some(0),
            y: Some(0),
            width: 1920,
            height: 1080,
        },
    );
    let mut device_b = DeviceState::new(client_b);
    device_b.window_geometry.insert(
        "win-b".to_string(),
        WindowGeometry {
            x: Some(100),
            y: Some(100),
            width: 800,
            height: 600,
        },
    );

    syncer_a
        .write_device_state(&serde_json::to_string(&device_a).unwrap())
        .await
        .unwrap();
    syncer_b
        .write_device_state(&serde_json::to_string(&device_b).unwrap())
        .await
        .unwrap();

    // Read back — each client should only see its own state
    let read_a: DeviceState =
        serde_json::from_str(&syncer_a.read_device_state().await.unwrap().unwrap()).unwrap();
    let read_b: DeviceState =
        serde_json::from_str(&syncer_b.read_device_state().await.unwrap().unwrap()).unwrap();

    assert_eq!(read_a.client_id, client_a);
    assert!(read_a.window_geometry.contains_key("win-a"));
    assert!(!read_a.window_geometry.contains_key("win-b"));

    assert_eq!(read_b.client_id, client_b);
    assert!(read_b.window_geometry.contains_key("win-b"));
    assert!(!read_b.window_geometry.contains_key("win-a"));

    let guard = handle_arc.lock().await;
    cleanup_state(&guard, "~/.shellkeep").await;
}

// =========================================================================
// SFTP Operations
// =========================================================================

#[tokio::test]
#[ignore]
async fn test_sftp_atomic_write() {
    let handle = connect().await;
    let sftp = open_sftp(&handle).await.expect("sftp open failed");

    let test_path = "/tmp/shellkeep-e2e-sftp-atomic.txt";
    let content = b"atomic write test content";

    write_file_atomic(&sftp, test_path, content)
        .await
        .expect("atomic write failed");

    let read_back = read_file(&sftp, test_path).await.expect("read failed");
    assert_eq!(read_back, content, "content mismatch after atomic write");

    // Verify no tmp files left behind
    let tmp_files = exec(
        &handle,
        &format!("ls {test_path}.tmp.* 2>/dev/null | wc -l"),
    )
    .await;
    assert_eq!(tmp_files.trim(), "0", "tmp files left behind: {tmp_files}");

    exec(&handle, &format!("rm -f {test_path}")).await;
}

#[tokio::test]
#[ignore]
async fn test_sftp_concurrent_writes() {
    let handle = connect().await;
    let sftp = open_sftp(&handle).await.expect("sftp open failed");

    let base_path = "/tmp/shellkeep-e2e-concurrent";
    let _ = exec(&handle, &format!("mkdir -p {base_path}")).await;

    // Write 10 files rapidly (sequentially — SftpSession is not Clone)
    let mut tasks = Vec::new();
    for i in 0..10 {
        let path = format!("{base_path}/file-{i}.txt");
        let content = format!("content-{i}-{}", uuid::Uuid::new_v4());
        tasks.push((path.clone(), content.clone()));
        write_file_atomic(&sftp, &path, content.as_bytes())
            .await
            .unwrap_or_else(|e| panic!("write {i} failed: {e}"));
    }

    // Verify all files exist and have correct content
    for (path, expected) in &tasks {
        let data = read_file(&sftp, path)
            .await
            .unwrap_or_else(|e| panic!("read {path} failed: {e}"));
        let actual = String::from_utf8_lossy(&data);
        assert_eq!(
            actual.as_ref(),
            expected.as_str(),
            "content mismatch for {path}"
        );
    }

    // Verify no tmp files left
    let tmp_count = exec(
        &handle,
        &format!("ls {base_path}/*.tmp.* 2>/dev/null | wc -l"),
    )
    .await;
    assert_eq!(tmp_count.trim(), "0", "tmp files left behind");

    exec(&handle, &format!("rm -rf {base_path}")).await;
}

#[tokio::test]
#[ignore]
async fn test_state_syncer_creation() {
    let handle = connect().await;
    cleanup_state(&handle, "~/.shellkeep").await;

    let handle_arc = wrap_handle(handle);
    let client_id = test_client_id("syncer-create");

    let syncer = StateSyncer::new(handle_arc.clone(), &client_id)
        .await
        .unwrap();
    assert!(syncer.is_sftp(), "expected SFTP mode");

    // Verify directories were created
    let guard = handle_arc.lock().await;
    let dir_check = exec(&guard, "test -d ~/.shellkeep/clients && echo OK").await;
    assert!(dir_check.contains("OK"), "state dirs not created");

    // Read should return None (no state yet)
    drop(guard);
    let shared = syncer.read_shared_state().await.unwrap();
    assert!(shared.is_none(), "expected no shared state on fresh setup");

    let device = syncer.read_device_state().await.unwrap();
    assert!(device.is_none(), "expected no device state on fresh setup");

    let guard = handle_arc.lock().await;
    cleanup_state(&guard, "~/.shellkeep").await;
}

// =========================================================================
// Tmux Session Lifecycle
// =========================================================================

#[tokio::test]
#[ignore]
async fn test_tmux_uuid_naming() {
    let handle = connect().await;
    let ws_uuid = "e2etest-ws-00000001";
    let session_uuid = "e2etest-ss-00000001";
    let prefix = "shellkeep--e2etest-";
    cleanup_tmux(&handle, prefix).await;

    let name = tmux::make_tmux_session_name(ws_uuid, session_uuid);
    assert_eq!(name, format!("shellkeep--{ws_uuid}--{session_uuid}"));

    tmux::create_session_russh(&handle, &name)
        .await
        .expect("create failed");

    let sessions = tmux::list_sessions_russh(&handle).await;
    assert!(
        sessions.contains(&name),
        "session not found in list: {sessions:?}"
    );

    cleanup_tmux(&handle, prefix).await;
}

#[tokio::test]
#[ignore]
async fn test_tmux_workspace_filter() {
    let handle = connect().await;
    let prefix = "shellkeep--e2efilter-";
    cleanup_tmux(&handle, prefix).await;

    let ws_a = "e2efilter-ws-aaaa";
    let ws_b = "e2efilter-ws-bbbb";
    let name_a1 = tmux::make_tmux_session_name(ws_a, "sess-a1");
    let name_a2 = tmux::make_tmux_session_name(ws_a, "sess-a2");
    let name_b1 = tmux::make_tmux_session_name(ws_b, "sess-b1");

    tmux::create_session_russh(&handle, &name_a1).await.unwrap();
    tmux::create_session_russh(&handle, &name_a2).await.unwrap();
    tmux::create_session_russh(&handle, &name_b1).await.unwrap();

    let all = tmux::list_sessions_russh(&handle).await;
    let filtered_a = tmux::filter_sessions_by_workspace(&all, ws_a, "WorkspaceA");
    let filtered_b = tmux::filter_sessions_by_workspace(&all, ws_b, "WorkspaceB");

    assert_eq!(
        filtered_a.len(),
        2,
        "workspace A should have 2 sessions: {filtered_a:?}"
    );
    assert_eq!(
        filtered_b.len(),
        1,
        "workspace B should have 1 session: {filtered_b:?}"
    );

    cleanup_tmux(&handle, prefix).await;
}

#[tokio::test]
#[ignore]
async fn test_tmux_kill_session() {
    let handle = connect().await;
    let name = "shellkeep--e2ekill--sess-kill";
    let _ = exec(
        &handle,
        &format!("tmux kill-session -t '{name}' 2>/dev/null"),
    )
    .await;

    tmux::create_session_russh(&handle, name).await.unwrap();
    let before = tmux::list_sessions_russh(&handle).await;
    assert!(before.contains(&name.to_string()), "session not created");

    exec(&handle, &format!("tmux kill-session -t '{name}'")).await;
    let after = tmux::list_sessions_russh(&handle).await;
    assert!(
        !after.contains(&name.to_string()),
        "session still exists after kill"
    );
}

#[tokio::test]
#[ignore]
async fn test_tmux_session_survives_disconnect() {
    let handle = connect().await;
    let name = "shellkeep--e2esurvive--sess-surv";
    let _ = exec(
        &handle,
        &format!("tmux kill-session -t '{name}' 2>/dev/null"),
    )
    .await;

    tmux::create_session_russh(&handle, name).await.unwrap();

    // Drop the handle (simulates SSH disconnect)
    drop(handle);

    // Reconnect
    let handle2 = connect().await;
    let sessions = tmux::list_sessions_russh(&handle2).await;
    assert!(
        sessions.contains(&name.to_string()),
        "session did not survive disconnect"
    );

    exec(
        &handle2,
        &format!("tmux kill-session -t '{name}' 2>/dev/null"),
    )
    .await;
}

#[tokio::test]
#[ignore]
async fn test_tmux_orphan_detection() {
    let handle = connect().await;
    let ws_uuid = "e2eorphan-ws";
    let prefix = "shellkeep--e2eorphan-";
    cleanup_tmux(&handle, prefix).await;

    // Create sessions — some in state, one orphaned
    let known = tmux::make_tmux_session_name(ws_uuid, "known-sess");
    let orphan = tmux::make_tmux_session_name(ws_uuid, "orphan-sess");
    tmux::create_session_russh(&handle, &known).await.unwrap();
    tmux::create_session_russh(&handle, &orphan).await.unwrap();

    // Simulate saved state that only knows about "known-sess"
    let saved_tabs = [TabState {
        session_uuid: "known-sess".to_string(),
        tmux_session_name: known.clone(),
        title: "Known".to_string(),
        position: 0,
        server_window_id: None,
        updated_at: String::new(),
    }];

    let all = tmux::list_sessions_russh(&handle).await;
    let ws_sessions = tmux::filter_sessions_by_workspace(&all, ws_uuid, "TestWS");

    // Find orphans: sessions on server that are NOT in saved state
    let orphans: Vec<_> = ws_sessions
        .iter()
        .filter(|s| !saved_tabs.iter().any(|t| &t.tmux_session_name == *s))
        .collect();

    assert_eq!(orphans.len(), 1, "expected 1 orphan: {orphans:?}");
    assert_eq!(orphans[0], &orphan);

    cleanup_tmux(&handle, prefix).await;
}

// =========================================================================
// Lock Mechanism
// =========================================================================

#[tokio::test]
#[ignore]
async fn test_lock_same_client_rejoin() {
    let handle = connect().await;
    let client_id = "e2e-lock-same";
    let workspace = "e2e-lock-test";
    let lock_name = format!("shellkeep-lock-{workspace}");
    let _ = exec(
        &handle,
        &format!("tmux kill-session -t '{lock_name}' 2>/dev/null"),
    )
    .await;

    // Join workspace
    lock::join_workspace(&handle, client_id, Some(15), workspace)
        .await
        .unwrap();

    // Join again with SAME client_id — should succeed, replacing stale entry (FR-LOCK-06)
    let others = lock::join_workspace(&handle, client_id, Some(15), workspace)
        .await
        .expect("same client_id rejoin should succeed");
    assert!(
        others.is_empty(),
        "no other devices expected on same-client rejoin"
    );

    // Verify device is listed
    let devices = lock::list_devices(&handle, Some(15), workspace).await;
    assert!(!devices.is_empty(), "device list should not be empty");
    assert!(
        devices.iter().any(|d| d.client_id == client_id),
        "our device should be in the list"
    );

    lock::leave_workspace(&handle, client_id, workspace)
        .await
        .ok();
}

#[tokio::test]
#[ignore]
async fn test_lock_multi_device_coexistence() {
    let handle = connect().await;
    let workspace = "e2e-lock-multi";
    let lock_name = format!("shellkeep-lock-{workspace}");
    let _ = exec(
        &handle,
        &format!("tmux kill-session -t '{lock_name}' 2>/dev/null"),
    )
    .await;

    // Client A joins
    let others_a = lock::join_workspace(&handle, "client-A", Some(15), workspace)
        .await
        .unwrap();
    assert!(others_a.is_empty(), "client-A should see no others");

    // Client B joins — should succeed and see client-A
    let others_b = lock::join_workspace(&handle, "client-B", Some(15), workspace)
        .await
        .unwrap();
    assert_eq!(
        others_b.len(),
        1,
        "client-B should see 1 other device: {others_b:?}"
    );
    assert_eq!(others_b[0].client_id, "client-A");

    // Verify both devices listed
    let devices = lock::list_devices(&handle, Some(15), workspace).await;
    assert_eq!(devices.len(), 2, "should have 2 devices: {devices:?}");
    let ids: Vec<&str> = devices.iter().map(|d| d.client_id.as_str()).collect();
    assert!(ids.contains(&"client-A"));
    assert!(ids.contains(&"client-B"));

    // Clean up
    lock::leave_workspace(&handle, "client-A", workspace)
        .await
        .ok();
    lock::leave_workspace(&handle, "client-B", workspace)
        .await
        .ok();
}

#[tokio::test]
#[ignore]
async fn test_lock_orphan_pruned() {
    let handle = connect().await;
    let workspace = "e2e-lock-orphan";
    let lock_name = format!("shellkeep-lock-{workspace}");
    let _ = exec(
        &handle,
        &format!("tmux kill-session -t '{lock_name}' 2>/dev/null"),
    )
    .await;

    // Client A joins
    lock::join_workspace(&handle, "client-orphan-A", Some(15), workspace)
        .await
        .unwrap();

    // Manually backdate the SHELLKEEP_LOCK_DEVICES to make client-A look orphaned
    let orphaned_device = serde_json::json!([{
        "client_id": "client-orphan-A",
        "hostname": "old-host",
        "connected_at": "2020-01-01T00:00:00Z",
        "pid": 1,
        "version": "0.1.0"
    }]);
    exec(
        &handle,
        &format!(
            "tmux set-environment -t '{lock_name}' SHELLKEEP_LOCK_DEVICES '{}'",
            orphaned_device
        ),
    )
    .await;

    // Client B joins — orphaned client-A should be pruned (FR-LOCK-07)
    let others = lock::join_workspace(&handle, "client-orphan-B", Some(15), workspace)
        .await
        .unwrap();
    assert!(
        others.is_empty(),
        "orphaned client-A should have been pruned: {others:?}"
    );

    // Verify only client-B is listed
    let devices = lock::list_devices(&handle, Some(15), workspace).await;
    assert_eq!(devices.len(), 1, "only client-B should remain: {devices:?}");
    assert_eq!(devices[0].client_id, "client-orphan-B");

    lock::leave_workspace(&handle, "client-orphan-B", workspace)
        .await
        .ok();
}

// =========================================================================
// Session Reconciliation
// =========================================================================

#[tokio::test]
#[ignore]
async fn test_reconcile_hidden_not_restored() {
    // Verify that tabs in hidden_sessions list are not considered for restore
    let handle = connect().await;
    let client_id = test_client_id("reconcile-hidden");
    cleanup_state(&handle, "~/.shellkeep").await;

    let handle_arc = wrap_handle(handle);
    let syncer = StateSyncer::new(handle_arc.clone(), &client_id)
        .await
        .unwrap();

    let ws_uuid = "reconcile-ws";
    let visible_uuid = "visible-tab-uuid";
    let hidden_uuid = "hidden-tab-uuid";

    let mut shared = SharedState::new();
    shared.workspaces.insert(
        "Default".to_string(),
        Workspace {
            name: "Default".to_string(),
            uuid: ws_uuid.to_string(),
            tabs: vec![
                TabState {
                    session_uuid: visible_uuid.to_string(),
                    tmux_session_name: tmux::make_tmux_session_name(ws_uuid, visible_uuid),
                    title: "Visible".to_string(),
                    position: 0,
                    server_window_id: None,
                    updated_at: String::new(),
                },
                TabState {
                    session_uuid: hidden_uuid.to_string(),
                    tmux_session_name: tmux::make_tmux_session_name(ws_uuid, hidden_uuid),
                    title: "Hidden".to_string(),
                    position: 1,
                    server_window_id: None,
                    updated_at: String::new(),
                },
            ],
            updated_at: String::new(),
        },
    );

    let mut device = DeviceState::new(&client_id);
    device.hidden_sessions = vec![hidden_uuid.to_string()];

    syncer
        .write_shared_state(&serde_json::to_string(&shared).unwrap())
        .await
        .unwrap();
    syncer
        .write_device_state(&serde_json::to_string(&device).unwrap())
        .await
        .unwrap();

    // Read back and simulate reconciliation logic
    let read_shared: SharedState =
        serde_json::from_str(&syncer.read_shared_state().await.unwrap().unwrap()).unwrap();
    let read_device: DeviceState =
        serde_json::from_str(&syncer.read_device_state().await.unwrap().unwrap()).unwrap();

    let tabs = &read_shared.workspaces["Default"].tabs;
    let restorable: Vec<_> = tabs
        .iter()
        .filter(|t| !read_device.hidden_sessions.contains(&t.session_uuid))
        .collect();

    assert_eq!(restorable.len(), 1, "only visible tab should be restorable");
    assert_eq!(restorable[0].session_uuid, visible_uuid);

    let guard = handle_arc.lock().await;
    cleanup_state(&guard, "~/.shellkeep").await;
}

#[tokio::test]
#[ignore]
async fn test_reconcile_dead_session() {
    let handle = connect().await;
    let ws_uuid = "e2edead-ws";
    let prefix = "shellkeep--e2edead-";
    cleanup_tmux(&handle, prefix).await;

    // Create one tmux session (alive), reference two in state (one will be dead)
    let alive_name = tmux::make_tmux_session_name(ws_uuid, "alive-sess");
    let dead_name = tmux::make_tmux_session_name(ws_uuid, "dead-sess");
    tmux::create_session_russh(&handle, &alive_name)
        .await
        .unwrap();
    // dead_name is NOT created on server — simulates a killed session

    let saved_tabs = vec![
        TabState {
            session_uuid: "alive-sess".to_string(),
            tmux_session_name: alive_name.clone(),
            title: "Alive".to_string(),
            position: 0,
            server_window_id: None,
            updated_at: String::new(),
        },
        TabState {
            session_uuid: "dead-sess".to_string(),
            tmux_session_name: dead_name.clone(),
            title: "Dead".to_string(),
            position: 1,
            server_window_id: None,
            updated_at: String::new(),
        },
    ];

    let server_sessions = tmux::list_sessions_russh(&handle).await;

    // Simulate reconciliation: identify dead tabs
    let dead_tabs: Vec<_> = saved_tabs
        .iter()
        .filter(|t| !server_sessions.contains(&t.tmux_session_name))
        .collect();

    assert_eq!(dead_tabs.len(), 1, "expected 1 dead tab");
    assert_eq!(dead_tabs[0].session_uuid, "dead-sess");

    let alive_tabs: Vec<_> = saved_tabs
        .iter()
        .filter(|t| server_sessions.contains(&t.tmux_session_name))
        .collect();
    assert_eq!(alive_tabs.len(), 1);
    assert_eq!(alive_tabs[0].session_uuid, "alive-sess");

    cleanup_tmux(&handle, prefix).await;
}

// =========================================================================
// Workspace Operations
// =========================================================================

#[tokio::test]
#[ignore]
async fn test_workspace_create_rename() {
    use shellkeep::state::workspace;

    let mut state = SharedState::new();
    workspace::create_workspace(&mut state, "MyProject").unwrap();
    assert!(state.workspaces.contains_key("MyProject"));
    let original_uuid = state.workspaces["MyProject"].uuid.clone();

    // Rename
    workspace::rename_workspace(&mut state, "MyProject", "RenamedProject").unwrap();
    assert!(
        !state.workspaces.contains_key("MyProject"),
        "old name should be gone"
    );
    assert!(
        state.workspaces.contains_key("RenamedProject"),
        "new name should exist"
    );

    // UUID must be preserved after rename
    assert_eq!(
        state.workspaces["RenamedProject"].uuid, original_uuid,
        "UUID should survive rename"
    );
}

#[tokio::test]
#[ignore]
async fn test_workspace_delete() {
    use shellkeep::state::workspace;

    let mut state = SharedState::new();
    workspace::create_workspace(&mut state, "ToDelete").unwrap();
    workspace::create_workspace(&mut state, "ToKeep").unwrap();
    assert_eq!(state.workspaces.len(), 2);

    workspace::delete_workspace(&mut state, "ToDelete").unwrap();
    assert_eq!(state.workspaces.len(), 1);
    assert!(!state.workspaces.contains_key("ToDelete"));
    assert!(state.workspaces.contains_key("ToKeep"));
}

#[tokio::test]
#[ignore]
async fn test_workspace_isolation() {
    let handle = connect().await;
    let prefix = "shellkeep--e2eiso-";
    cleanup_tmux(&handle, prefix).await;

    let ws_a = "e2eiso-ws-aaa";
    let ws_b = "e2eiso-ws-bbb";

    // Create sessions in different workspaces
    let a1 = tmux::make_tmux_session_name(ws_a, "a1");
    let a2 = tmux::make_tmux_session_name(ws_a, "a2");
    let b1 = tmux::make_tmux_session_name(ws_b, "b1");
    tmux::create_session_russh(&handle, &a1).await.unwrap();
    tmux::create_session_russh(&handle, &a2).await.unwrap();
    tmux::create_session_russh(&handle, &b1).await.unwrap();

    let all = tmux::list_sessions_russh(&handle).await;

    // Workspace A should only see its sessions
    let a_sessions = tmux::filter_sessions_by_workspace(&all, ws_a, "WSA");
    assert_eq!(a_sessions.len(), 2);
    assert!(a_sessions.iter().all(|s| s.contains(ws_a)));

    // Workspace B should only see its sessions
    let b_sessions = tmux::filter_sessions_by_workspace(&all, ws_b, "WSB");
    assert_eq!(b_sessions.len(), 1);
    assert!(b_sessions.iter().all(|s| s.contains(ws_b)));

    cleanup_tmux(&handle, prefix).await;
}

#[tokio::test]
#[ignore]
async fn test_workspace_uuid_stable_across_rename() {
    use shellkeep::state::workspace;

    let handle = connect().await;
    let client_id = test_client_id("ws-rename");
    cleanup_state(&handle, "~/.shellkeep").await;

    let handle_arc = wrap_handle(handle);
    let syncer = StateSyncer::new(handle_arc.clone(), &client_id)
        .await
        .unwrap();

    // Create state with workspace, persist, rename, persist again
    let mut shared = SharedState::new();
    workspace::create_workspace(&mut shared, "OriginalName").unwrap();
    let original_uuid = shared.workspaces["OriginalName"].uuid.clone();

    // Add a tab to the workspace
    shared
        .workspaces
        .get_mut("OriginalName")
        .unwrap()
        .tabs
        .push(TabState {
            session_uuid: "tab-1".to_string(),
            tmux_session_name: tmux::make_tmux_session_name(&original_uuid, "tab-1"),
            title: "Tab 1".to_string(),
            position: 0,
            server_window_id: None,
            updated_at: String::new(),
        });

    // Write original
    syncer
        .write_shared_state(&serde_json::to_string(&shared).unwrap())
        .await
        .unwrap();

    // Rename
    workspace::rename_workspace(&mut shared, "OriginalName", "NewName").unwrap();
    syncer
        .write_shared_state(&serde_json::to_string(&shared).unwrap())
        .await
        .unwrap();

    // Read back
    let read: SharedState =
        serde_json::from_str(&syncer.read_shared_state().await.unwrap().unwrap()).unwrap();

    assert!(!read.workspaces.contains_key("OriginalName"));
    assert!(read.workspaces.contains_key("NewName"));
    assert_eq!(
        read.workspaces["NewName"].uuid, original_uuid,
        "UUID changed after rename!"
    );
    assert_eq!(read.workspaces["NewName"].tabs.len(), 1);
    // tmux session name should still reference the original UUID
    assert!(
        read.workspaces["NewName"].tabs[0]
            .tmux_session_name
            .contains(&original_uuid)
    );

    let guard = handle_arc.lock().await;
    cleanup_state(&guard, "~/.shellkeep").await;
}

// =========================================================================
// FR-TABS-03: Tab movement between windows within the same workspace
// =========================================================================

/// FR-TABS-03: moving a tab between windows updates server_window_id and
/// persists correctly. Verifies the invariant that all tabs in a window
/// belong to the same workspace.
#[tokio::test]
#[ignore]
async fn test_fr_tabs_03_move_tab_between_windows() {
    let handle = connect().await;
    let client_id = test_client_id("fr-tabs-03");
    cleanup_state(&handle, "~/.shellkeep").await;

    let handle_arc = wrap_handle(handle);
    let syncer = StateSyncer::new(handle_arc.clone(), &client_id)
        .await
        .expect("syncer creation failed");

    // Setup: 2 windows in "Default" workspace, 2 tabs in window-A, 1 in window-B
    let mut shared = SharedState::new();
    shared.workspaces.insert(
        "Default".to_string(),
        Workspace {
            name: "Default".to_string(),
            uuid: "ws-main".to_string(),
            tabs: vec![
                TabState {
                    session_uuid: "tab-1".to_string(),
                    tmux_session_name: "shellkeep--ws-main--tab-1".to_string(),
                    title: "Tab1".to_string(),
                    position: 0,
                    server_window_id: Some("window-A".to_string()),
                    updated_at: String::new(),
                },
                TabState {
                    session_uuid: "tab-2".to_string(),
                    tmux_session_name: "shellkeep--ws-main--tab-2".to_string(),
                    title: "Tab2".to_string(),
                    position: 1,
                    server_window_id: Some("window-A".to_string()),
                    updated_at: String::new(),
                },
                TabState {
                    session_uuid: "tab-3".to_string(),
                    tmux_session_name: "shellkeep--ws-main--tab-3".to_string(),
                    title: "Tab3".to_string(),
                    position: 0,
                    server_window_id: Some("window-B".to_string()),
                    updated_at: String::new(),
                },
            ],
            updated_at: String::new(),
        },
    );

    // Persist initial state
    let json = serde_json::to_string_pretty(&shared).unwrap();
    syncer.write_shared_state(&json).await.unwrap();

    // Simulate moving tab-2 from window-A to window-B (FR-TABS-03)
    let ws = shared.workspaces.get_mut("Default").unwrap();
    let tab = ws
        .tabs
        .iter_mut()
        .find(|t| t.session_uuid == "tab-2")
        .unwrap();
    tab.server_window_id = Some("window-B".to_string());
    tab.position = 1; // second tab in window-B

    // Persist updated state
    let json = serde_json::to_string_pretty(&shared).unwrap();
    syncer.write_shared_state(&json).await.unwrap();

    // Read back and verify
    let read_json = syncer.read_shared_state().await.unwrap().unwrap();
    let read: SharedState = serde_json::from_str(&read_json).unwrap();
    let tabs = &read.workspaces["Default"].tabs;
    assert_eq!(tabs.len(), 3, "total tab count must be preserved");

    // Window-A should now have 1 tab, window-B should have 2
    let window_a: Vec<_> = tabs
        .iter()
        .filter(|t| t.server_window_id.as_deref() == Some("window-A"))
        .collect();
    let window_b: Vec<_> = tabs
        .iter()
        .filter(|t| t.server_window_id.as_deref() == Some("window-B"))
        .collect();
    assert_eq!(window_a.len(), 1, "window-A should have 1 tab after move");
    assert_eq!(window_b.len(), 2, "window-B should have 2 tabs after move");

    // Verify tab-2 is now in window-B
    let moved_tab = tabs.iter().find(|t| t.session_uuid == "tab-2").unwrap();
    assert_eq!(
        moved_tab.server_window_id.as_deref(),
        Some("window-B"),
        "tab-2 should have moved to window-B"
    );

    // Verify workspace isolation: all tabs in each window belong to same workspace
    for tab in tabs {
        assert!(
            tab.tmux_session_name.contains("ws-main"),
            "tab {} should reference workspace UUID ws-main",
            tab.session_uuid
        );
    }

    let guard = handle_arc.lock().await;
    cleanup_state(&guard, "~/.shellkeep").await;
}

/// FR-TABS-03: workspace isolation — tabs from different workspaces must not
/// share a server_window_id. Verifies the invariant at the state level.
#[tokio::test]
#[ignore]
async fn test_fr_tabs_03_workspace_isolation() {
    let handle = connect().await;
    let client_id = test_client_id("fr-tabs-03-iso");
    cleanup_state(&handle, "~/.shellkeep").await;

    let handle_arc = wrap_handle(handle);
    let syncer = StateSyncer::new(handle_arc.clone(), &client_id)
        .await
        .expect("syncer creation failed");

    // Setup: 2 workspaces, each with tabs in their own windows
    let mut shared = SharedState::new();
    shared.workspaces.insert(
        "Dev".to_string(),
        Workspace {
            name: "Dev".to_string(),
            uuid: "ws-dev".to_string(),
            tabs: vec![TabState {
                session_uuid: "dev-tab-1".to_string(),
                tmux_session_name: "shellkeep--ws-dev--dev-tab-1".to_string(),
                title: "DevTab".to_string(),
                position: 0,
                server_window_id: Some("win-dev".to_string()),
                updated_at: String::new(),
            }],
            updated_at: String::new(),
        },
    );
    shared.workspaces.insert(
        "Prod".to_string(),
        Workspace {
            name: "Prod".to_string(),
            uuid: "ws-prod".to_string(),
            tabs: vec![TabState {
                session_uuid: "prod-tab-1".to_string(),
                tmux_session_name: "shellkeep--ws-prod--prod-tab-1".to_string(),
                title: "ProdTab".to_string(),
                position: 0,
                server_window_id: Some("win-prod".to_string()),
                updated_at: String::new(),
            }],
            updated_at: String::new(),
        },
    );

    // Persist and read back
    let json = serde_json::to_string_pretty(&shared).unwrap();
    syncer.write_shared_state(&json).await.unwrap();
    let read_json = syncer.read_shared_state().await.unwrap().unwrap();
    let read: SharedState = serde_json::from_str(&read_json).unwrap();

    // Verify each workspace's tabs have distinct window IDs (no cross-workspace mixing)
    let dev_window_ids: Vec<_> = read.workspaces["Dev"]
        .tabs
        .iter()
        .filter_map(|t| t.server_window_id.as_deref())
        .collect();
    let prod_window_ids: Vec<_> = read.workspaces["Prod"]
        .tabs
        .iter()
        .filter_map(|t| t.server_window_id.as_deref())
        .collect();

    // No window ID should appear in both workspaces
    for dev_wid in &dev_window_ids {
        assert!(
            !prod_window_ids.contains(dev_wid),
            "window {dev_wid} appears in both Dev and Prod — violates FR-TABS-03 isolation"
        );
    }

    let guard = handle_arc.lock().await;
    cleanup_state(&guard, "~/.shellkeep").await;
}
