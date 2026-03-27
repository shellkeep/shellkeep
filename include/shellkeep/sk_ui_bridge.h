// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ui_bridge.h
 * @brief Toolkit-agnostic UI callback vtable.
 *
 * Decouples the C backend (connect flow, reconnection, session management)
 * from any specific UI toolkit. The backend calls these function pointers
 * to request dialogs, update progress, create terminal widgets, etc.
 *
 * GTK3 and Qt6 each provide their own implementation of this vtable.
 * This allows the connect layer (sk_connect.c) to work without including
 * any toolkit headers.
 */

#ifndef SK_UI_BRIDGE_H
#define SK_UI_BRIDGE_H

#include "shellkeep/sk_types.h"

#include <glib.h>

#include <stdbool.h>

#ifdef __cplusplus
extern "C"
{
#endif

  /* ------------------------------------------------------------------ */
  /* Opaque UI handle                                                    */
  /* ------------------------------------------------------------------ */

  /**
   * Opaque pointer to the UI implementation's context.
   * For Qt6: points to SkUiBridgeQt (C++ object, cast to void*).
   * For GTK3: points to a struct wrapping GtkApplication + GtkWindow.
   */
  typedef void *SkUiHandle;

  /* ------------------------------------------------------------------ */
  /* Host key dialog                                                     */
  /* ------------------------------------------------------------------ */

  /** Host key dialog result (mirrors SkHostKeyDialogResult). */
  typedef enum
  {
    SK_BRIDGE_HOST_KEY_ACCEPT_SAVE = 0,
    SK_BRIDGE_HOST_KEY_CONNECT_ONCE,
    SK_BRIDGE_HOST_KEY_REJECT,
  } SkBridgeHostKeyResult;

  /* ------------------------------------------------------------------ */
  /* Close dialog                                                        */
  /* ------------------------------------------------------------------ */

  typedef enum
  {
    SK_BRIDGE_CLOSE_HIDE = 0,
    SK_BRIDGE_CLOSE_TERMINATE = 1,
    SK_BRIDGE_CLOSE_CANCEL = 2,
  } SkBridgeCloseResult;

  /* ------------------------------------------------------------------ */
  /* Connection phase                                                    */
  /* ------------------------------------------------------------------ */

  typedef enum
  {
    SK_BRIDGE_PHASE_IDLE = 0,
    SK_BRIDGE_PHASE_CONNECTING,
    SK_BRIDGE_PHASE_AUTHENTICATING,
    SK_BRIDGE_PHASE_CHECKING_TMUX,
    SK_BRIDGE_PHASE_LOADING_STATE,
    SK_BRIDGE_PHASE_RESTORING,
    SK_BRIDGE_PHASE_DONE,
    SK_BRIDGE_PHASE_ERROR,
  } SkBridgeConnPhase;

  /* ------------------------------------------------------------------ */
  /* Connection indicator                                                */
  /* ------------------------------------------------------------------ */

  typedef enum
  {
    SK_BRIDGE_INDICATOR_GREEN = 0,
    SK_BRIDGE_INDICATOR_YELLOW = 1,
    SK_BRIDGE_INDICATOR_RED = 2,
  } SkBridgeConnIndicator;

  /* ------------------------------------------------------------------ */
  /* Terminal handle                                                      */
  /* ------------------------------------------------------------------ */

  /**
   * Opaque handle to a terminal widget created by the UI layer.
   * For Qt6: SkTerminalWidget*.
   * For GTK3: SkTerminalTab*.
   */
  typedef void *SkBridgeTerminal;

  /**
   * Opaque handle to a UI tab.
   */
  typedef void *SkBridgeTab;

  /**
   * Opaque handle to a UI window.
   */
  typedef void *SkBridgeWindow;

  /* ------------------------------------------------------------------ */
  /* UI Bridge vtable                                                    */
  /* ------------------------------------------------------------------ */

  /**
   * Function pointer table for all UI operations the backend needs.
   * The backend never includes toolkit-specific headers; it only calls
   * through this vtable.
   *
   * All dialog functions are thread-safe: the UI implementation is
   * responsible for dispatching to the main/UI thread and blocking
   * until the user responds. The backend calls these from worker threads.
   */
  typedef struct SkUiBridge
  {
    /* -- Dialogs (blocking, called from worker threads) -- */

    /**
     * Show TOFU dialog for unknown host key (FR-CONN-03).
     * Must block until user responds.
     */
    SkBridgeHostKeyResult (*host_key_unknown)(SkUiHandle ui, const char *hostname,
                                              const char *fingerprint, const char *key_type);

    /**
     * Show warning dialog for changed host key (FR-CONN-02).
     * Must block until dismissed.
     */
    void (*host_key_changed)(SkUiHandle ui, const char *hostname, const char *old_fingerprint,
                             const char *new_fingerprint, const char *key_type);

    /**
     * Show password input dialog (FR-CONN-09).
     * Must block until user responds.
     * @return Newly allocated password, or NULL if cancelled.
     */
    char *(*auth_password)(SkUiHandle ui, const char *prompt);

    /**
     * Show keyboard-interactive / MFA dialog (FR-CONN-10).
     * Must block until user responds.
     * @return Array of response strings, or NULL if cancelled.
     */
    char **(*auth_mfa)(SkUiHandle ui, const char *name, const char *instruction,
                       const char **prompts, const gboolean *show_input, int n_prompts);

    /**
     * Show passphrase dialog for encrypted key (FR-CONN-09).
     * @return Newly allocated passphrase, or NULL if cancelled.
     */
    char *(*auth_passphrase)(SkUiHandle ui, const char *key_path);

    /**
     * Show lock conflict dialog (FR-LOCK-05).
     * @return true if user chooses to take over, false to cancel.
     */
    bool (*conflict_dialog)(SkUiHandle ui, const char *hostname, const char *connected_at);

    /**
     * Show environment selection dialog (FR-ENV-03).
     * @return Newly allocated selected env name, or NULL if cancelled.
     */
    char *(*environment_select)(SkUiHandle ui, const char **envs, int n_envs, const char *last_env);

    /**
     * Show close window dialog (FR-TABS-17).
     */
    SkBridgeCloseResult (*close_dialog)(SkUiHandle ui, int n_active);

    /**
     * Show error dialog.
     */
    void (*error_dialog)(SkUiHandle ui, const char *title, const char *message);

    /**
     * Show info dialog.
     */
    void (*info_dialog)(SkUiHandle ui, const char *title, const char *message);

    /* -- Connection feedback (called from main thread) -- */

    /**
     * Create/show connection feedback overlay.
     * @return Opaque handle to the feedback widget.
     */
    void *(*feedback_create)(SkUiHandle ui);

    /**
     * Update feedback phase.
     */
    void (*feedback_set_phase)(void *feedback, SkBridgeConnPhase phase);

    /**
     * Update feedback restoration progress.
     */
    void (*feedback_set_progress)(void *feedback, int current, int total);

    /**
     * Set feedback error message.
     */
    void (*feedback_set_error)(void *feedback, const char *message);

    /**
     * Destroy feedback overlay.
     */
    void (*feedback_free)(void *feedback);

    /* -- Window/tab management (called from main thread) -- */

    /**
     * Create a new window.
     * @return Opaque window handle.
     */
    SkBridgeWindow (*window_new)(SkUiHandle ui, const char *title, int x, int y, int width,
                                 int height);

    /**
     * Show a window.
     */
    void (*window_show)(SkBridgeWindow win);

    /**
     * Destroy a window.
     */
    void (*window_free)(SkBridgeWindow win);

    /**
     * Create a new terminal widget.
     * @return Opaque terminal handle.
     */
    SkBridgeTerminal (*terminal_new)(SkUiHandle ui, const char *font_family, int font_size,
                                     int scrollback_lines);

    /**
     * Connect terminal to an SSH channel for I/O.
     * @param ssh_fd  File descriptor from the SSH connection.
     */
    bool (*terminal_connect)(SkBridgeTerminal term, int ssh_fd, SkSshChannel *channel);

    /**
     * Disconnect terminal (keep widget visible with content).
     */
    void (*terminal_disconnect)(SkBridgeTerminal term);

    /**
     * Feed raw data into terminal for display.
     */
    void (*terminal_feed)(SkBridgeTerminal term, const char *data, int len);

    /**
     * Enter dead session mode on terminal.
     */
    void (*terminal_set_dead)(SkBridgeTerminal term, const char *history_data, int history_len,
                              const char *message);

    /**
     * Free a terminal widget.
     */
    void (*terminal_free)(SkBridgeTerminal term);

    /**
     * Add a terminal tab to a window.
     * @return Opaque tab handle.
     */
    SkBridgeTab (*window_add_tab)(SkBridgeWindow win, SkBridgeTerminal term, const char *title);

    /**
     * Set tab connection indicator color.
     */
    void (*tab_set_indicator)(SkBridgeTab tab, SkBridgeConnIndicator indicator);

    /**
     * Mark tab as dead.
     */
    void (*tab_set_dead)(SkBridgeTab tab, bool dead);

    /* -- Toast notifications -- */

    /**
     * Show a toast notification.
     */
    void (*toast_show)(SkUiHandle ui, const char *message, int timeout_ms);

    /* -- Welcome screen -- */

    /**
     * Show the welcome screen.
     * Fills out_host, out_user, out_port with user input.
     * @return true if user provided a host, false if cancelled.
     */
    bool (*welcome_show)(SkUiHandle ui, const char **recent, int n_recent, bool first_use,
                         char **out_host, char **out_user, int *out_port);

  } SkUiBridge;

  /* ------------------------------------------------------------------ */
  /* Global bridge instance                                              */
  /* ------------------------------------------------------------------ */

  /**
   * Set the global UI bridge implementation.
   * Must be called once at startup before any connect flow begins.
   * @param bridge  Pointer to the vtable (must outlive the application).
   * @param ui      Opaque handle to the UI context.
   */
  void sk_ui_bridge_set(const SkUiBridge *bridge, SkUiHandle ui);

  /**
   * Get the global UI bridge vtable.
   * @return Pointer to the bridge, or NULL if not set.
   */
  const SkUiBridge *sk_ui_bridge_get(void);

  /**
   * Get the global UI handle.
   */
  SkUiHandle sk_ui_bridge_get_handle(void);

  /**
   * Check if the UI bridge has been initialized.
   */
  bool sk_ui_bridge_is_set(void);

#ifdef __cplusplus
}
#endif

#endif /* SK_UI_BRIDGE_H */
