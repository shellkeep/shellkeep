// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_terminal.h
 * @brief Terminal layer -- VTE widget wrapper public API.
 *
 * Provides SkTerminalTab, a VTE terminal wrapper that routes I/O between
 * an SSH channel and the on-screen terminal widget.  Handles PTY resize
 * (SIGWINCH), input routing, scrollback search, selection/copy, config
 * application, and dead session rendering.
 *
 * Threading model (INV-IO-1): Data I/O uses g_io_add_watch() on the SSH
 * file descriptor.  The GTK main thread is NEVER blocked.
 */

#ifndef SK_TERMINAL_H
#define SK_TERMINAL_H

#include "shellkeep/sk_types.h"

#include <gtk/gtk.h>

#include <glib.h>

#include <stdbool.h>

#ifdef __cplusplus
extern "C"
{
#endif

  /* ------------------------------------------------------------------ */
  /* Opaque types                                                        */
  /* ------------------------------------------------------------------ */

  /** Terminal tab wrapping a VTE widget + SSH channel I/O. */
  typedef struct _SkTerminalTab SkTerminalTab;

  /** Search overlay bar for scrollback search. */
  typedef struct _SkTerminalSearch SkTerminalSearch;

  /** Dead session overlay rendering. */
  typedef struct _SkTerminalDead SkTerminalDead;

  /* ------------------------------------------------------------------ */
  /* Cursor shapes (FR-TERMINAL-13)                                      */
  /* ------------------------------------------------------------------ */

  typedef enum
  {
    SK_CURSOR_SHAPE_BLOCK = 0,
    SK_CURSOR_SHAPE_IBEAM = 1,
    SK_CURSOR_SHAPE_UNDERLINE = 2,
  } SkTermCursorShape;

  /* ------------------------------------------------------------------ */
  /* Terminal theme (FR-TERMINAL-11)                                     */
  /* ------------------------------------------------------------------ */

  /**
   * Terminal color theme compatible with Gogh/base16 format.
   * 16 ANSI colors + foreground, background, cursor, highlight.
   */
  typedef struct
  {
    char *name; /**< Theme name. */

    /* 16 standard ANSI colors (indices 0-15). */
    GdkRGBA palette[16];

    /* Special colors. */
    GdkRGBA foreground;
    GdkRGBA background;
    GdkRGBA cursor_color;
    GdkRGBA cursor_fg;
    GdkRGBA highlight_bg;
    GdkRGBA highlight_fg;

    bool has_cursor_color;
    bool has_cursor_fg;
    bool has_highlight_bg;
    bool has_highlight_fg;
  } SkTerminalTheme;

  /* ------------------------------------------------------------------ */
  /* Terminal tab configuration (FR-TERMINAL-10..16)                     */
  /* ------------------------------------------------------------------ */

  /**
   * Configuration parameters applied to a terminal tab.
   * Populated from SkConfig by the caller.
   */
  typedef struct
  {
    const char *font_family;    /**< e.g. "Monospace" */
    int font_size;              /**< In points, e.g. 12 */
    int scrollback_lines;       /**< Default 10000 (FR-TERMINAL-14) */
    SkTermCursorShape cursor_shape; /**< FR-TERMINAL-13 */
    bool cursor_blink;          /**< FR-TERMINAL-13 */
    bool bold_is_bright;        /**< Map bold to bright ANSI colors */
    bool allow_hyperlinks;      /**< OSC 8 hyperlinks */
    const char *word_chars;     /**< Characters treated as word chars */
    bool audible_bell;          /**< Audible bell enabled */
  } SkTerminalConfig;

  /* ------------------------------------------------------------------ */
  /* Terminal tab lifecycle                                               */
  /* ------------------------------------------------------------------ */

  /**
   * Create a new terminal tab widget.
   *
   * @param config  Terminal configuration (copied internally).
   * @return New terminal tab, or NULL on failure.
   */
  SkTerminalTab *sk_terminal_tab_new(const SkTerminalConfig *config);

  /**
   * Connect the terminal tab to an SSH channel for I/O.
   * Sets up g_io_add_watch() on the SSH connection fd.
   *
   * @param tab      Terminal tab.
   * @param conn     SSH connection (for fd and IO watch).
   * @param channel  SSH channel (for read/write, ownership NOT transferred).
   * @param error    Return location for error.
   * @return true on success.
   */
  bool sk_terminal_tab_connect(SkTerminalTab *tab, SkSshConnection *conn, SkSshChannel *channel,
                               GError **error);

  /**
   * Disconnect SSH channel without destroying the VTE widget.
   * The widget remains visible with its current content.
   *
   * @param tab  Terminal tab.
   */
  void sk_terminal_tab_disconnect(SkTerminalTab *tab);

  /**
   * Feed raw data into the VTE terminal.
   * Used for replaying history data.
   *
   * @param tab   Terminal tab.
   * @param data  Raw terminal data.
   * @param len   Length of data, or -1 for NUL-terminated.
   */
  void sk_terminal_tab_feed(SkTerminalTab *tab, const char *data, gssize len);

  /**
   * Free the terminal tab and all associated resources.
   *
   * @param tab  Terminal tab (may be NULL).
   */
  void sk_terminal_tab_free(SkTerminalTab *tab);

  /* ------------------------------------------------------------------ */
  /* Widget access                                                       */
  /* ------------------------------------------------------------------ */

  /**
   * Get the top-level container widget for embedding in a notebook/tab.
   * This is an overlay containing the VTE terminal and search bar.
   *
   * @param tab  Terminal tab.
   * @return GtkWidget (caller does NOT own).
   */
  GtkWidget *sk_terminal_tab_get_widget(SkTerminalTab *tab);

  /**
   * Get the underlying VteTerminal widget directly.
   *
   * @param tab  Terminal tab.
   * @return VteTerminal widget (caller does NOT own).
   */
  GtkWidget *sk_terminal_tab_get_vte(SkTerminalTab *tab);

  /* ------------------------------------------------------------------ */
  /* Properties                                                          */
  /* ------------------------------------------------------------------ */

  /**
   * Get the current terminal dimensions.
   *
   * @param tab   Terminal tab.
   * @param cols  Output: column count.
   * @param rows  Output: row count.
   */
  void sk_terminal_tab_get_size(const SkTerminalTab *tab, int *cols, int *rows);

  /**
   * Check if the terminal tab has an active SSH connection.
   *
   * @param tab  Terminal tab.
   * @return true if connected and I/O is running.
   */
  bool sk_terminal_tab_is_connected(const SkTerminalTab *tab);

  /**
   * Check if the terminal is in read-only (dead session) mode.
   *
   * @param tab  Terminal tab.
   * @return true if dead session mode is active.
   */
  bool sk_terminal_tab_is_dead(const SkTerminalTab *tab);

  /* ------------------------------------------------------------------ */
  /* Config application (FR-TERMINAL-10..16)                             */
  /* ------------------------------------------------------------------ */

  /**
   * Apply configuration to the terminal (font, scrollback, cursor, etc.).
   * Safe to call at any time; hot-reloadable settings take effect immediately.
   *
   * @param tab     Terminal tab.
   * @param config  Configuration to apply (copied internally).
   */
  void sk_terminal_tab_apply_config(SkTerminalTab *tab, const SkTerminalConfig *config);

  /**
   * Apply a color theme to the terminal.
   *
   * @param tab    Terminal tab.
   * @param theme  Theme to apply (colors copied internally).
   */
  void sk_terminal_tab_apply_theme(SkTerminalTab *tab, const SkTerminalTheme *theme);

  /* ------------------------------------------------------------------ */
  /* Selection & copy (FR-TERMINAL-03..05, FR-TABS-11..12)               */
  /* ------------------------------------------------------------------ */

  /**
   * Copy the current selection to the system clipboard.
   * (Ctrl+Shift+C action.)
   *
   * @param tab  Terminal tab.
   */
  void sk_terminal_tab_copy_clipboard(SkTerminalTab *tab);

  /**
   * Paste from the system clipboard into the terminal.
   * (Ctrl+Shift+V action.)
   *
   * @param tab  Terminal tab.
   */
  void sk_terminal_tab_paste_clipboard(SkTerminalTab *tab);

  /**
   * Copy the entire scrollback buffer to the clipboard.
   * (Ctrl+Shift+A action, FR-TABS-12.)
   *
   * @param tab  Terminal tab.
   */
  void sk_terminal_tab_copy_all(SkTerminalTab *tab);

  /**
   * Export scrollback to a file (FR-TERMINAL-18).
   * Shows a file chooser dialog.
   *
   * @param tab     Terminal tab.
   * @param parent  Parent window for dialog.
   * @return true if export succeeded.
   */
  bool sk_terminal_tab_export_scrollback(SkTerminalTab *tab, GtkWindow *parent);

  /* ------------------------------------------------------------------ */
  /* Search (FR-TERMINAL-07, FR-TABS-09)                                 */
  /* ------------------------------------------------------------------ */

  /**
   * Toggle the search bar overlay visibility.
   * (Ctrl+Shift+F action.)
   *
   * @param tab  Terminal tab.
   */
  void sk_terminal_tab_toggle_search(SkTerminalTab *tab);

  /**
   * Check whether the search bar is currently visible.
   *
   * @param tab  Terminal tab.
   * @return true if search bar is shown.
   */
  bool sk_terminal_tab_search_is_visible(const SkTerminalTab *tab);

  /* ------------------------------------------------------------------ */
  /* Zoom (FR-TABS-13)                                                   */
  /* ------------------------------------------------------------------ */

  /**
   * Increase font size by 1 point.
   */
  void sk_terminal_tab_zoom_in(SkTerminalTab *tab);

  /**
   * Decrease font size by 1 point (minimum 4).
   */
  void sk_terminal_tab_zoom_out(SkTerminalTab *tab);

  /**
   * Reset font size to the configured default.
   */
  void sk_terminal_tab_zoom_reset(SkTerminalTab *tab);

  /* ------------------------------------------------------------------ */
  /* Dead session rendering (FR-HISTORY-05..08)                          */
  /* ------------------------------------------------------------------ */

  /**
   * Enter dead session mode: feed raw history data, show overlay banner,
   * and make the terminal read-only.
   *
   * @param tab          Terminal tab.
   * @param history_data Raw history data to feed (may be NULL).
   * @param history_len  Length of history data.
   * @param message      Banner message (e.g. "This session has ended on the server.").
   */
  void sk_terminal_tab_set_dead(SkTerminalTab *tab, const char *history_data, gssize history_len,
                                const char *message);

  /**
   * Callback type for the "Create new session" button in dead overlay.
   */
  typedef void (*SkTerminalNewSessionCb)(SkTerminalTab *tab, gpointer user_data);

  /**
   * Set the callback for the "Create new session" button.
   *
   * @param tab        Terminal tab.
   * @param callback   Callback function.
   * @param user_data  User data for callback.
   */
  void sk_terminal_tab_set_new_session_cb(SkTerminalTab *tab, SkTerminalNewSessionCb callback,
                                          gpointer user_data);

  /* ------------------------------------------------------------------ */
  /* Theme helpers                                                       */
  /* ------------------------------------------------------------------ */

  /**
   * Load a theme from a JSON file (Gogh/base16 compatible).
   *
   * @param path   Path to JSON theme file.
   * @param error  Return location for error.
   * @return Theme object, or NULL on error.  Caller must free with
   *         sk_terminal_theme_free().
   */
  SkTerminalTheme *sk_terminal_theme_load(const char *path, GError **error);

  /**
   * Create the built-in default theme (dark).
   *
   * @return Default theme.  Caller must free with sk_terminal_theme_free().
   */
  SkTerminalTheme *sk_terminal_theme_default(void);

  /**
   * Free a theme object.
   *
   * @param theme  Theme (may be NULL).
   */
  void sk_terminal_theme_free(SkTerminalTheme *theme);

  /* ------------------------------------------------------------------ */
  /* Legacy API (kept for compatibility with sk_types.h SkTerminal)      */
  /* ------------------------------------------------------------------ */

  /**
   * Create a new terminal widget bound to an SSH channel.
   * @deprecated Use sk_terminal_tab_new() + sk_terminal_tab_connect().
   */
  SkTerminal *sk_terminal_new(SkSshChannel *channel);
  GtkWidget *sk_terminal_get_widget(SkTerminal *term);
  bool sk_terminal_start(SkTerminal *term, GError **error);
  void sk_terminal_stop(SkTerminal *term);
  void sk_terminal_free(SkTerminal *term);
  void sk_terminal_get_size(const SkTerminal *term, int *cols, int *rows);
  bool sk_terminal_is_active(const SkTerminal *term);

#ifdef __cplusplus
}
#endif

#endif /* SK_TERMINAL_H */
