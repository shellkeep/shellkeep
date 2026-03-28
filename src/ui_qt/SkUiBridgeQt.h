// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#ifndef SK_UI_BRIDGE_QT_H
#define SK_UI_BRIDGE_QT_H

#include <QApplication>
#include <QHash>
#include <QObject>

#include "shellkeep/sk_ui_bridge.h"

class SkMainWindow;
class SkTrayIcon;

/**
 * Qt6 implementation of the SkUiBridge vtable.
 *
 * This singleton C++ object provides the concrete implementations
 * of each bridge function pointer. All dialog calls are dispatched
 * to the UI thread via QMetaObject::invokeMethod(BlockingQueuedConnection).
 *
 * Static extern "C" wrapper functions are used as function pointers
 * in the SkUiBridge vtable so the pure-C backend can call them.
 */
class SkUiBridgeQt : public QObject
{
    Q_OBJECT

public:
    explicit SkUiBridgeQt(QApplication *app, QObject *parent = nullptr);
    ~SkUiBridgeQt() override;

    /** Get the singleton instance. */
    static SkUiBridgeQt *instance();

    /** Get the populated bridge vtable. */
    const SkUiBridge *bridge() const { return &m_bridge; }

    /** Get the QApplication. */
    QApplication *app() const { return m_app; }

    /** Get or create the primary main window. */
    SkMainWindow *primaryWindow();

    /** Get the tray icon. */
    SkTrayIcon *trayIcon();

    /** Register a window by opaque handle. */
    void registerWindow(SkBridgeWindow handle, SkMainWindow *window);

    /** Unregister a window. */
    void unregisterWindow(SkBridgeWindow handle);

    /** Lookup a window by handle. */
    SkMainWindow *lookupWindow(SkBridgeWindow handle) const;

    /** Set auto-accept for unknown host keys (dev/testing only). */
    void setAutoAcceptHosts(bool accept) { m_autoAcceptHosts = accept; }
    bool autoAcceptHosts() const { return m_autoAcceptHosts; }

    void setAutoPassword(const QString &pw) { m_autoPassword = pw; }
    QString autoPassword() const { return m_autoPassword; }

private:
    void populateVtable();

    static SkUiBridgeQt *s_instance;

    QApplication *m_app = nullptr;
    SkUiBridge m_bridge{};
    SkMainWindow *m_primaryWindow = nullptr;
    SkTrayIcon *m_trayIcon = nullptr;
    QHash<SkBridgeWindow, SkMainWindow *> m_windows;
    bool m_autoAcceptHosts = false;
    QString m_autoPassword;
};

/* ------------------------------------------------------------------ */
/* extern "C" wrappers for the vtable function pointers                */
/* ------------------------------------------------------------------ */

extern "C" {

SkBridgeHostKeyResult sk_qt_host_key_unknown(SkUiHandle ui, const char *hostname,
                                              const char *fingerprint, const char *key_type);
void sk_qt_host_key_changed(SkUiHandle ui, const char *hostname, const char *old_fp,
                             const char *new_fp, const char *key_type);
char *sk_qt_auth_password(SkUiHandle ui, const char *prompt);
char **sk_qt_auth_mfa(SkUiHandle ui, const char *name, const char *instruction,
                       const char **prompts, const gboolean *show_input, int n_prompts);
char *sk_qt_auth_passphrase(SkUiHandle ui, const char *key_path);
bool sk_qt_conflict_dialog(SkUiHandle ui, const char *hostname, const char *connected_at);
char *sk_qt_environment_select(SkUiHandle ui, const char **envs, int n_envs,
                                const char *last_env);
SkBridgeCloseResult sk_qt_close_dialog(SkUiHandle ui, int n_active);
void sk_qt_error_dialog(SkUiHandle ui, const char *title, const char *message);
void sk_qt_info_dialog(SkUiHandle ui, const char *title, const char *message);

void *sk_qt_feedback_create(SkUiHandle ui);
void sk_qt_feedback_set_phase(void *feedback, SkBridgeConnPhase phase);
void sk_qt_feedback_set_progress(void *feedback, int current, int total);
void sk_qt_feedback_set_error(void *feedback, const char *message);
void sk_qt_feedback_free(void *feedback);

SkBridgeWindow sk_qt_window_new(SkUiHandle ui, const char *title, int x, int y,
                                 int width, int height);
void sk_qt_window_show(SkBridgeWindow win);
void sk_qt_window_free(SkBridgeWindow win);

SkBridgeTerminal sk_qt_terminal_new(SkUiHandle ui, const char *font_family,
                                     int font_size, int scrollback_lines);
bool sk_qt_terminal_connect(SkBridgeTerminal term, int ssh_fd, SkSshChannel *channel);
void sk_qt_terminal_disconnect(SkBridgeTerminal term);
void sk_qt_terminal_feed(SkBridgeTerminal term, const char *data, int len);
void sk_qt_terminal_set_dead(SkBridgeTerminal term, const char *history_data,
                              int history_len, const char *message);
void sk_qt_terminal_free(SkBridgeTerminal term);

SkBridgeTab sk_qt_window_add_tab(SkBridgeWindow win, SkBridgeTerminal term, const char *title);
void sk_qt_tab_set_indicator(SkBridgeTab tab, SkBridgeConnIndicator indicator);
void sk_qt_tab_set_dead(SkBridgeTab tab, bool dead);

void sk_qt_toast_show(SkUiHandle ui, const char *message, int timeout_ms);

bool sk_qt_welcome_show(SkUiHandle ui, const char **recent, int n_recent, bool first_use,
                         char **out_host, char **out_user, int *out_port);

} /* extern "C" */

#endif /* SK_UI_BRIDGE_QT_H */
