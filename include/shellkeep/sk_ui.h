// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ui.h
 * @brief UI layer -- GTK windows, tabs, dialogs, welcome screen, toasts.
 *
 * Top-level layer.  Depends on Terminal and State layers.
 * NEVER depends on SSH layer directly (NFR-ARCH-02).
 *
 * NOTE: The UI window/tab types are named SkAppWindow/SkAppTab to avoid
 * collision with the state-layer structs SkWindow/SkTab in sk_state.h.
 * The opaque forward declarations SkWindow/SkTab in sk_types.h refer to
 * these UI types internally.
 *
 * Requirements: FR-TABS-*, FR-UI-*, FR-ENV-03..05, FR-CONN-16
 */

#ifndef SK_UI_H
#define SK_UI_H

#include "shellkeep/sk_types.h"

#include <gtk/gtk.h>

#include <glib.h>

#include <stdbool.h>

#ifdef __cplusplus
extern "C"
{
#endif

/* ------------------------------------------------------------------ */
/* Forward declarations                                                */
/* ------------------------------------------------------------------ */

/** Forward-declare terminal tab (from sk_terminal.h). */
#ifndef SK_TERMINAL_H
  typedef struct _SkTerminalTab SkTerminalTab;
#endif

  /* Lock info and recent connections are used by callers that include
   * both sk_session.h and sk_ui.h.  The dialog functions below take
   * plain strings so no additional types are needed here. */

  /* ------------------------------------------------------------------ */
  /* Connection indicator colors (FR-UI-04)                              */
  /* ------------------------------------------------------------------ */

  typedef enum
  {
    SK_CONN_INDICATOR_GREEN = 0,  /**< Connected, no visible icon. */
    SK_CONN_INDICATOR_YELLOW = 1, /**< High latency (>300ms). */
    SK_CONN_INDICATOR_RED = 2,    /**< Disconnected / reconnecting. */
  } SkConnIndicator;

  /* ------------------------------------------------------------------ */
  /* Connection phase (FR-CONN-16)                                       */
  /* ------------------------------------------------------------------ */

  typedef enum
  {
    SK_CONN_PHASE_IDLE = 0,
    SK_CONN_PHASE_CONNECTING,
    SK_CONN_PHASE_AUTHENTICATING,
    SK_CONN_PHASE_CHECKING_TMUX,
    SK_CONN_PHASE_LOADING_STATE,
    SK_CONN_PHASE_RESTORING,
    SK_CONN_PHASE_DONE,
    SK_CONN_PHASE_ERROR,
  } SkConnPhase;

  /* ------------------------------------------------------------------ */
  /* Close window dialog result (FR-TABS-17)                             */
  /* ------------------------------------------------------------------ */

  typedef enum
  {
    SK_CLOSE_RESULT_HIDE = 0,      /**< Hide window (default). */
    SK_CLOSE_RESULT_TERMINATE = 1, /**< Terminate sessions. */
    SK_CLOSE_RESULT_CANCEL = 2,    /**< Cancel the close. */
  } SkCloseResult;

  /* ------------------------------------------------------------------ */
  /* Host key dialog result (FR-CONN-01..05)                             */
  /* ------------------------------------------------------------------ */

  typedef enum
  {
    SK_HOST_KEY_ACCEPT_SAVE = 0, /**< Accept and save to known_hosts. */
    SK_HOST_KEY_CONNECT_ONCE,    /**< Connect once, do not save. */
    SK_HOST_KEY_REJECT,          /**< Reject / cancel. */
  } SkHostKeyDialogResult;

  /* ------------------------------------------------------------------ */
  /* SkAppWindow -- GTK window with tab notebook (FR-TABS-*)             */
  /* ------------------------------------------------------------------ */

  /** Application window with tabbed terminal notebook. */
  typedef struct _SkAppWindow SkAppWindow;

  /**
   * Create a new application window.
   *
   * @param app  GtkApplication instance.
   * @return New window, or NULL on failure.
   */
  SkAppWindow *sk_app_window_new(GtkApplication *app);

  /**
   * Create a new application window restoring from saved state geometry.
   *
   * @param app     GtkApplication instance.
   * @param title   Window title (may be NULL for default).
   * @param x       Saved X position (-1 to ignore).
   * @param y       Saved Y position (-1 to ignore).
   * @param width   Saved width (0 for default 800).
   * @param height  Saved height (0 for default 600).
   * @return New window, or NULL on failure.
   */
  SkAppWindow *sk_app_window_new_from_state(GtkApplication *app, const char *title, int x, int y,
                                            int width, int height);

  /**
   * Get the underlying GtkWindow widget.
   */
  GtkWidget *sk_app_window_get_widget(SkAppWindow *win);

  /**
   * Get the GtkWindow* cast.
   */
  GtkWindow *sk_app_window_get_gtk_window(SkAppWindow *win);

  /**
   * Show the window.
   */
  void sk_app_window_show(SkAppWindow *win);

  /**
   * Hide the window (not destroy).
   */
  void sk_app_window_hide(SkAppWindow *win);

  /**
   * Free a window and all its tabs.
   */
  void sk_app_window_free(SkAppWindow *win);

  /* ------------------------------------------------------------------ */
  /* Tab management                                                      */
  /* ------------------------------------------------------------------ */

  /** A tab within an application window. */
  typedef struct _SkAppTab SkAppTab;

  /**
   * Add a terminal tab to the window.
   *
   * @param win    Window.
   * @param tab    Terminal tab widget (ownership NOT transferred).
   * @param title  Initial tab title.
   * @return New app tab handle, or NULL on failure.
   */
  SkAppTab *sk_app_window_add_tab(SkAppWindow *win, SkTerminalTab *tab, const char *title);

  /**
   * Remove a tab from the window.
   * Does not free the underlying SkTerminalTab.
   */
  void sk_app_window_remove_tab(SkAppWindow *win, SkAppTab *tab);

  /**
   * Get the number of tabs in the window.
   */
  int sk_app_window_get_tab_count(const SkAppWindow *win);

  /**
   * Get the currently active tab.
   * @return Active tab, or NULL if no tabs.
   */
  SkAppTab *sk_app_window_get_active_tab(SkAppWindow *win);

  /**
   * Set the active tab by index.
   */
  void sk_app_window_set_active_tab(SkAppWindow *win, int index);

  /**
   * Get the current window geometry.
   */
  void sk_app_window_get_geometry(const SkAppWindow *win, int *x, int *y, int *width, int *height);

  /**
   * Check if window is currently visible.
   */
  bool sk_app_window_is_visible(const SkAppWindow *win);

  /* ------------------------------------------------------------------ */
  /* Tab properties                                                      */
  /* ------------------------------------------------------------------ */

  /**
   * Get the tab title.
   * @return Internal string, do NOT free.
   */
  const char *sk_app_tab_get_title(const SkAppTab *tab);

  /**
   * Set the tab title. FR-SESSION-06, FR-TABS-08.
   */
  void sk_app_tab_set_title(SkAppTab *tab, const char *title);

  /**
   * Set the connection indicator for the tab. FR-UI-04.
   */
  void sk_app_tab_set_indicator(SkAppTab *tab, SkConnIndicator indicator);

  /**
   * Mark the tab as dead (red title + warning icon). FR-UI-07.
   */
  void sk_app_tab_set_dead(SkAppTab *tab, bool dead);

  /**
   * Check if the tab is marked dead.
   */
  bool sk_app_tab_is_dead(const SkAppTab *tab);

  /**
   * Get the SkTerminalTab associated with this app tab.
   */
  SkTerminalTab *sk_app_tab_get_terminal(SkAppTab *tab);

  /**
   * Begin editing the tab title (F2 or double-click). FR-TABS-08.
   */
  void sk_app_tab_begin_rename(SkAppTab *tab);

  /* ------------------------------------------------------------------ */
  /* Tab rename callback                                                 */
  /* ------------------------------------------------------------------ */

  /**
   * Callback invoked when a tab title is changed by the user.
   *
   * @param tab        The tab being renamed.
   * @param new_title  The new title string.
   * @param user_data  User-supplied pointer.
   */
  typedef void (*SkTabRenamedCb)(SkAppTab *tab, const char *new_title, gpointer user_data);

  /**
   * Set the callback for tab rename events.
   */
  void sk_app_window_set_tab_renamed_cb(SkAppWindow *win, SkTabRenamedCb cb, gpointer user_data);

  /* ------------------------------------------------------------------ */
  /* Tab close callback                                                  */
  /* ------------------------------------------------------------------ */

  /**
   * Callback invoked when a tab is closed by the user.
   */
  typedef void (*SkTabClosedCb)(SkAppTab *tab, gpointer user_data);

  /**
   * Set the callback for tab close events.
   */
  void sk_app_window_set_tab_closed_cb(SkAppWindow *win, SkTabClosedCb cb, gpointer user_data);

  /* ------------------------------------------------------------------ */
  /* Drag and drop tabs between windows (FR-TABS-03)                     */
  /* ------------------------------------------------------------------ */

  /**
   * Enable drag-and-drop for tabs between windows.
   * Call once after creating the window.
   */
  void sk_app_window_enable_tab_dnd(SkAppWindow *win);

  /* ------------------------------------------------------------------ */
  /* Dialogs                                                             */
  /* ------------------------------------------------------------------ */

  /**
   * Show TOFU dialog for unknown host key (FR-CONN-03).
   *
   * @param parent       Parent window.
   * @param hostname     Remote hostname.
   * @param fingerprint  SHA-256 fingerprint string.
   * @param key_type     Key type (e.g. "ssh-ed25519").
   * @return Dialog result.
   */
  SkHostKeyDialogResult sk_dialog_host_key_unknown(GtkWindow *parent, const char *hostname,
                                                   const char *fingerprint, const char *key_type);

  /**
   * Show blocking dialog for changed host key (FR-CONN-02).
   * No override button -- user must resolve manually.
   *
   * @param parent           Parent window.
   * @param hostname         Remote hostname.
   * @param old_fingerprint  Previous fingerprint (may be NULL).
   * @param new_fingerprint  Current fingerprint.
   * @param key_type         Key type.
   */
  void sk_dialog_host_key_changed(GtkWindow *parent, const char *hostname,
                                  const char *old_fingerprint, const char *new_fingerprint,
                                  const char *key_type);

  /**
   * Show password dialog with masked input (FR-CONN-09).
   *
   * @param parent  Parent window.
   * @param prompt  Prompt from the server.
   * @return Newly allocated password, or NULL if cancelled.
   *         Caller must explicit_bzero + g_free.
   */
  char *sk_dialog_auth_password(GtkWindow *parent, const char *prompt);

  /**
   * Show keyboard-interactive / MFA dialog (FR-CONN-10).
   *
   * @param parent      Parent window.
   * @param name        Auth request name (may be empty).
   * @param instruction Instruction from server (may be empty).
   * @param prompts     Array of prompt strings.
   * @param show_input  Array of booleans; FALSE = masked.
   * @param n_prompts   Number of prompts.
   * @return Array of response strings, or NULL if cancelled.
   *         Caller must explicit_bzero + g_free each element and the array.
   */
  char **sk_dialog_auth_mfa(GtkWindow *parent, const char *name, const char *instruction,
                            const char **prompts, const gboolean *show_input, int n_prompts);

  /**
   * Show conflict dialog when another client holds the lock (FR-LOCK-05).
   * Uses user-friendly language -- no "client-id" term visible.
   *
   * @param parent    Parent window.
   * @param hostname  Hostname of the machine holding the lock.
   * @param connected_at  When the other client connected (ISO 8601).
   * @return TRUE if user chooses "Disconnect and connect here", FALSE to cancel.
   */
  bool sk_dialog_conflict(GtkWindow *parent, const char *hostname, const char *connected_at);

  /**
   * Show environment selection dialog (FR-ENV-03..05).
   *
   * @param parent       Parent window.
   * @param envs         Array of environment name strings.
   * @param n_envs       Number of environments.
   * @param last_env     Last-used environment name (pre-selected), or NULL.
   * @return Newly allocated selected environment name, or NULL if cancelled.
   */
  char *sk_dialog_environment_select(GtkWindow *parent, const char **envs, int n_envs,
                                     const char *last_env);

  /**
   * Show close window dialog (FR-TABS-17).
   *
   * @param parent    Parent window.
   * @param n_active  Number of active (live) tabs.
   * @return Close result.
   */
  SkCloseResult sk_dialog_close_window(GtkWindow *parent, int n_active);

  /**
   * Show error dialog with descriptive message.
   */
  void sk_dialog_error(GtkWindow *parent, const char *title, const char *message);

  /**
   * Show informational dialog.
   */
  void sk_dialog_info(GtkWindow *parent, const char *title, const char *message);

  /* ------------------------------------------------------------------ */
  /* Welcome screen (FR-UI-01..04)                                       */
  /* ------------------------------------------------------------------ */

  /** Result from the welcome screen. */
  typedef struct
  {
    char *host;        /**< Hostname or IP (caller frees). */
    char *user;        /**< Username or NULL (caller frees). */
    int port;          /**< Port (0 = default 22). */
    char *client_name; /**< Friendly client-id name or NULL (caller frees). */
  } SkWelcomeResult;

  /**
   * Show the welcome screen.
   *
   * @param parent       Parent window.
   * @param recent       Array of recent connection display strings, or NULL.
   * @param recent_hosts Array of host strings (parallel to recent).
   * @param recent_users Array of user strings (parallel to recent).
   * @param recent_ports Array of port ints (parallel to recent).
   * @param n_recent     Number of recent entries.
   * @param first_use    TRUE if this is the first time the app is used.
   * @return Result struct, or NULL fields if cancelled.
   *         Caller must free all strings in the result.
   */
  SkWelcomeResult *sk_welcome_screen_show(GtkWindow *parent, const char **recent,
                                          const char **recent_hosts, const char **recent_users,
                                          const int *recent_ports, int n_recent, bool first_use);

  /**
   * Free a welcome result.
   */
  void sk_welcome_result_free(SkWelcomeResult *result);

  /* ------------------------------------------------------------------ */
  /* Connection feedback (FR-CONN-16)                                    */
  /* ------------------------------------------------------------------ */

  /** Connection feedback overlay widget. */
  typedef struct _SkConnFeedback SkConnFeedback;

  /**
   * Create a connection feedback overlay for a window.
   *
   * @param parent  GtkWindow to show the overlay in.
   * @return Feedback handle.
   */
  SkConnFeedback *sk_conn_feedback_new(GtkWindow *parent);

  /**
   * Update the connection phase display.
   *
   * @param fb     Feedback handle.
   * @param phase  Current phase.
   */
  void sk_conn_feedback_set_phase(SkConnFeedback *fb, SkConnPhase phase);

  /**
   * Update restoration progress.
   *
   * @param fb       Feedback handle.
   * @param current  Current session being restored (1-based).
   * @param total    Total sessions to restore.
   */
  void sk_conn_feedback_set_progress(SkConnFeedback *fb, int current, int total);

  /**
   * Set an error message on the feedback overlay.
   */
  void sk_conn_feedback_set_error(SkConnFeedback *fb, const char *message);

  /**
   * Hide and destroy the feedback overlay.
   */
  void sk_conn_feedback_free(SkConnFeedback *fb);

  /* ------------------------------------------------------------------ */
  /* Toasts (FR-UI-08, FR-SESSION-11, FR-TABS-19)                        */
  /* ------------------------------------------------------------------ */

  /**
   * Show a toast notification at the bottom of the window.
   * Auto-dismisses after timeout_ms milliseconds.
   *
   * @param parent      Window to show the toast in.
   * @param message     Toast message text.
   * @param timeout_ms  Auto-dismiss timeout (0 = default 5000ms).
   */
  void sk_toast_show(GtkWindow *parent, const char *message, int timeout_ms);

  /**
   * Show "Session kept on server" toast (FR-SESSION-11).
   */
  void sk_toast_session_kept(GtkWindow *parent);

  /**
   * Show "continues in tray" toast (FR-TABS-19).
   */
  void sk_toast_continues_in_tray(GtkWindow *parent);

  /* ------------------------------------------------------------------ */
  /* Legacy compatibility wrappers                                       */
  /* ------------------------------------------------------------------ */

  /**
   * Legacy: Show a host-key verification dialog.
   * @deprecated Use sk_dialog_host_key_unknown() or sk_dialog_host_key_changed().
   */
  bool sk_ui_dialog_host_key(GtkWindow *parent, const char *host, const char *fingerprint,
                             bool changed);

  /**
   * Legacy: Show a password input dialog.
   * @deprecated Use sk_dialog_auth_password().
   */
  char *sk_ui_dialog_password(GtkWindow *parent, const char *prompt);

  /**
   * Legacy: Show an error dialog.
   * @deprecated Use sk_dialog_error().
   */
  void sk_ui_dialog_error(GtkWindow *parent, const char *title, const char *message);

  /**
   * Legacy: Show an informational dialog.
   * @deprecated Use sk_dialog_info().
   */
  void sk_ui_dialog_info(GtkWindow *parent, const char *title, const char *message);

#ifdef __cplusplus
}
#endif

#endif /* SK_UI_H */
