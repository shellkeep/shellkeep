// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_terminal_internal.h
 * @brief Internal header for the terminal layer.
 *
 * Shared struct definitions for SkTerminalTab used across the
 * terminal layer implementation files.  NOT part of the public API.
 */

#ifndef SK_TERMINAL_INTERNAL_H
#define SK_TERMINAL_INTERNAL_H

#include "shellkeep/sk_ssh.h"
#include "shellkeep/sk_terminal.h"

#include <gtk/gtk.h>

#include <vte/vte.h>

#include <stdbool.h>

#ifdef __cplusplus
extern "C"
{
#endif

  /* ------------------------------------------------------------------ */
  /* SkTerminalTab internal structure                                    */
  /* ------------------------------------------------------------------ */

  struct _SkTerminalTab
  {
    /* Widgets. */
    GtkWidget *overlay; /**< Top-level overlay container. */
    VteTerminal *vte;   /**< The VTE terminal widget. */

    /* Search bar (owned, lazily created). */
    GtkWidget *search_bar;   /**< Search overlay container. */
    GtkWidget *search_entry; /**< Search text entry. */
    GtkWidget *search_label; /**< Match count label. */
    bool search_visible;     /**< Whether search bar is shown. */

    /* Dead session overlay. */
    GtkWidget *dead_overlay; /**< Dead session banner + button. */
    bool is_dead;            /**< Read-only dead mode active. */
    SkTerminalNewSessionCb new_session_cb;
    gpointer new_session_data;

    /* SSH I/O. */
    SkSshConnection *conn; /**< Borrowed SSH connection. */
    SkSshChannel *channel; /**< Borrowed SSH channel. */
    guint io_watch_id;     /**< GSource ID for IO watch. */
    bool connected;        /**< I/O routing active. */

    /* Config. */
    SkTerminalConfig config; /**< Current configuration (copied). */
    int default_font_size;   /**< Original font size for zoom reset. */
    int current_font_size;   /**< Current font size (zoom-adjusted). */

    /* Resize tracking (FR-TERMINAL-17..18). */
    int last_cols;
    int last_rows;
  };

  /* ------------------------------------------------------------------ */
  /* Legacy SkTerminal wrapper                                           */
  /* ------------------------------------------------------------------ */

  struct _SkTerminal
  {
    SkTerminalTab *tab;
    SkSshChannel *channel;
  };

#ifdef __cplusplus
}
#endif

#endif /* SK_TERMINAL_INTERNAL_H */
