// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use iced::{keyboard, window, Point, Size};

use shellkeep::ssh;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum Message {
    TerminalEvent(iced_term::Event),
    SshData(u64, Vec<u8>),
    PasteToTerminal(u64, Vec<u8>),
    SshDisconnected(u64, String),
    SshConnected(u64, Result<(), String>),
    ExistingSessionsFound(Result<Vec<String>, String>),
    SelectTab(usize),
    CloseTab(usize),
    NewTab,
    ReconnectTab(usize),
    AutoReconnectTick,
    ContextMenuCopy,
    ContextMenuPaste,
    ContextMenuDismiss,
    TabContextMenu(usize, f32, f32),
    TabMoveLeft(usize),
    TabMoveRight(usize),
    /// Hide tab: disconnect SSH but keep tmux session alive on server
    HideTab(usize),
    /// Close all tabs except the one at this index
    CloseOtherTabs(usize),
    /// Close all tabs to the right of this index
    CloseTabsToRight(usize),
    /// FR-SESSION-10a: confirm close tab(s) — user clicked Terminate
    ConfirmCloseTabs,
    /// FR-SESSION-10a: cancel close tab(s)
    CancelCloseTabs,
    StartRename(usize),
    ConnectRecent(usize),
    RenameInputChanged(String),
    FinishRename,
    ToastDismiss,
    FlushState,
    HostInputChanged(String),
    PortInputChanged(String),
    UserInputChanged(String),
    IdentityInputChanged(String),
    Connect,
    KeyEvent(keyboard::Event),
    ConnectionPhaseTick,
    /// FR-RECONNECT-02: advance spinner animation frame
    SpinnerTick,
    /// FR-TRAY-01: poll tray menu events
    TrayPoll,
    /// FR-UI-07: create a fresh session replacing a dead tab
    CreateNewSession(usize),
    // FR-TABS-09: scrollback search
    SearchToggle,
    SearchInputChanged(String),
    SearchNext,
    SearchPrev,
    SearchClose,
    /// FR-CONFIG-04: config file changed on disk
    ConfigReloaded,
    /// FR-LOCK-04: periodic lock heartbeat
    LockHeartbeatTick,
    /// FR-LOCK-04: heartbeat result
    LockHeartbeatDone(Result<(), String>),
    /// FR-TABS-17: window close requested by window manager
    WindowCloseRequested(window::Id),
    /// FR-TABS-17: close dialog — quit application
    CloseDialogClose,
    /// FR-TABS-17: close dialog — cancel (dismiss dialog)
    CloseDialogCancel,
    /// FR-STATE-14: window moved or resized
    WindowMoved(Point),
    WindowResized(Size),
    /// FR-TERMINAL-18: export scrollback to file
    ExportScrollback,
    /// FR-TABS-12: copy entire scrollback to clipboard
    CopyScrollback,
    // FR-ENV-03: environment selection dialog
    #[allow(dead_code)]
    ShowEnvDialog,
    EnvFilterChanged(String),
    SelectEnv(String),
    ConfirmEnv,
    NewEnvFromDialog,
    // FR-ENV-07..09: environment management
    #[allow(dead_code)]
    ShowNewEnvDialog,
    NewEnvInputChanged(String),
    ConfirmNewEnv,
    CancelNewEnv,
    #[allow(dead_code)]
    ShowRenameEnvDialog(String),
    RenameEnvInputChanged(String),
    ConfirmRenameEnv,
    CancelRenameEnv,
    #[allow(dead_code)]
    ShowDeleteEnvDialog(String),
    ConfirmDeleteEnv,
    CancelDeleteEnv,
    CancelEnvDialog,
    /// FR-RECONNECT-08: network change detected (Linux)
    NetworkChanged,
    /// FR-ENV-10: switch to a different environment
    SwitchEnvironment(String),
    /// FR-CONN-20: remote state syncer initialized
    StateSyncerReady(Result<Arc<ssh::sftp::StateSyncer>, String>),
    /// FR-STATE-02: server state loaded (takes precedence over local)
    ServerStateLoaded(Result<Option<String>, String>),
    /// FR-CONN-03: host key TOFU — accept and save
    #[allow(dead_code)]
    HostKeyAcceptSave,
    /// FR-CONN-03: host key TOFU — connect once without saving
    #[allow(dead_code)]
    HostKeyConnectOnce,
    /// FR-CONN-03: host key TOFU — reject and disconnect
    #[allow(dead_code)]
    HostKeyReject,
    /// FR-CONN-02: host key changed — dismiss (disconnect already happened)
    #[allow(dead_code)]
    HostKeyChangedDismiss,
    /// FR-CONN-09: password dialog input changed
    #[allow(dead_code)]
    PasswordInputChanged(String),
    /// FR-CONN-09: password dialog — submit
    #[allow(dead_code)]
    PasswordSubmit,
    /// FR-CONN-09: password dialog — cancel
    #[allow(dead_code)]
    PasswordCancel,
    /// FR-LOCK-05: lock conflict — take over
    #[allow(dead_code)]
    LockTakeOver,
    /// FR-LOCK-05: lock conflict — cancel
    #[allow(dead_code)]
    LockCancel,
    /// FR-UI-01: toggle advanced connection options
    ToggleAdvanced,
    /// FR-UI-03: client-id naming input changed
    ClientIdInputChanged(String),
    /// FR-UI-04/05: periodic latency measurement tick
    LatencyTick,
    /// FR-UI-04/05: latency measurement result (tab_id, latency_ms or None on error)
    LatencyMeasured(u64, Option<u32>),
}
