// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_terminal_tab.c
 * @brief VTE terminal wrapper -- core implementation.
 *
 * Implements SkTerminalTab: VTE widget creation, SSH channel I/O routing,
 * input interception (Ctrl+Shift shortcuts), resize handling, config/theme
 * application, selection & copy, and zoom.
 *
 * FR-TERMINAL-01..06: Input routing -- only Ctrl+Shift shortcuts intercepted.
 * FR-TERMINAL-07..10: Scrollback configuration and search.
 * FR-TERMINAL-11..16: Config application (font, colors, cursor).
 * FR-TERMINAL-17..18: Resize propagation to SSH channel.
 * FR-TERMINAL-19: Independent VteTerminal per tab.
 */

#include "shellkeep/sk_i18n.h"
#include "shellkeep/sk_log.h"

#include "sk_terminal_internal.h"
#include <gdk/gdk.h>
#include <string.h>
#include <sys/stat.h>

/* Forward declarations. */
static gboolean on_ssh_data_available(GIOChannel *source, GIOCondition condition,
                                      gpointer user_data);
static void on_vte_commit(VteTerminal *vte, gchar *text, guint size, gpointer user_data);
static gboolean on_key_press(GtkWidget *widget, GdkEventKey *event, gpointer user_data);
static void on_size_allocate(GtkWidget *widget, GdkRectangle *allocation, gpointer user_data);
static void on_selection_changed(VteTerminal *vte, gpointer user_data);
static void apply_font(SkTerminalTab *tab);

/* ------------------------------------------------------------------ */
/* Default configuration                                               */
/* ------------------------------------------------------------------ */

static const SkTerminalConfig DEFAULT_CONFIG = {
  .font_family = "Monospace",
  .font_size = 12,
  .scrollback_lines = 10000, /* FR-TERMINAL-14 */
  .cursor_shape = SK_CURSOR_SHAPE_BLOCK,
  .cursor_blink = true,
  .bold_is_bright = false,
  .allow_hyperlinks = true,
  .word_chars = NULL,
  .audible_bell = false,
};

/* ------------------------------------------------------------------ */
/* Terminal tab lifecycle                                               */
/* ------------------------------------------------------------------ */

SkTerminalTab *
sk_terminal_tab_new(const SkTerminalConfig *config)
{
  SkTerminalTab *tab = g_new0(SkTerminalTab, 1);

  /* Copy configuration, using defaults for NULLs. */
  if (config != NULL)
  {
    tab->config = *config;
  }
  else
  {
    tab->config = DEFAULT_CONFIG;
  }
  if (tab->config.font_family == NULL)
  {
    tab->config.font_family = g_strdup(DEFAULT_CONFIG.font_family);
  }
  else
  {
    tab->config.font_family = g_strdup(tab->config.font_family);
  }
  /* Deep-copy word_chars if present. */
  tab->config.word_chars = g_strdup(tab->config.word_chars);
  if (tab->config.font_size <= 0)
  {
    tab->config.font_size = DEFAULT_CONFIG.font_size;
  }
  if (tab->config.scrollback_lines < 0)
  {
    tab->config.scrollback_lines = DEFAULT_CONFIG.scrollback_lines;
  }

  tab->default_font_size = tab->config.font_size;
  tab->current_font_size = tab->config.font_size;

  /* Create overlay container. */
  tab->overlay = gtk_overlay_new();
  g_object_ref_sink(tab->overlay);

  /* Create VTE terminal widget (FR-TERMINAL-19). */
  tab->vte = VTE_TERMINAL(vte_terminal_new());
  gtk_container_add(GTK_CONTAINER(tab->overlay), GTK_WIDGET(tab->vte));
  gtk_widget_show(GTK_WIDGET(tab->vte));

  /* Apply initial configuration. */
  sk_terminal_tab_apply_config(tab, &tab->config);

  /* Connect key-press for input routing (FR-TERMINAL-01..03). */
  g_signal_connect(tab->vte, "key-press-event", G_CALLBACK(on_key_press), tab);

  /* Connect size-allocate for resize handling (FR-TERMINAL-17..18). */
  g_signal_connect(tab->vte, "size-allocate", G_CALLBACK(on_size_allocate), tab);

  /* Connect selection-changed for primary selection (FR-TERMINAL-03). */
  g_signal_connect(tab->vte, "selection-changed", G_CALLBACK(on_selection_changed), tab);

  gtk_widget_set_can_focus(GTK_WIDGET(tab->vte), TRUE);
  gtk_widget_show(tab->overlay);

  SK_LOG_DEBUG(SK_LOG_COMPONENT_TERMINAL, "Terminal tab created (scrollback=%d, font=%s %d)",
               tab->config.scrollback_lines, tab->config.font_family, tab->config.font_size);

  return tab;
}

bool
sk_terminal_tab_connect(SkTerminalTab *tab, SkSshConnection *conn, SkSshChannel *channel,
                        GError **error)
{
  g_return_val_if_fail(tab != NULL, false);
  g_return_val_if_fail(conn != NULL, false);
  g_return_val_if_fail(channel != NULL, false);

  if (tab->connected)
  {
    sk_terminal_tab_disconnect(tab);
  }

  tab->conn = conn;
  tab->channel = channel;

  /* Set up IO watch on SSH fd for incoming data (INV-IO-1). */
  tab->io_watch_id = sk_ssh_connection_add_io_watch(conn, on_ssh_data_available, tab);
  if (tab->io_watch_id == 0)
  {
    g_set_error(error, SK_ERROR, SK_ERROR_IO, "Failed to set up I/O watch on SSH connection");
    tab->conn = NULL;
    tab->channel = NULL;
    return false;
  }

  /* Connect VTE commit signal for outgoing data. */
  g_signal_connect(tab->vte, "commit", G_CALLBACK(on_vte_commit), tab);

  tab->connected = true;

  /* Send initial PTY size. */
  int cols = vte_terminal_get_column_count(tab->vte);
  int rows = vte_terminal_get_row_count(tab->vte);
  tab->last_cols = cols;
  tab->last_rows = rows;

  GError *resize_err = NULL;
  if (!sk_ssh_channel_resize_pty(channel, cols, rows, &resize_err))
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_TERMINAL, "Initial PTY resize failed: %s",
                resize_err ? resize_err->message : "unknown");
    g_clear_error(&resize_err);
  }

  SK_LOG_INFO(SK_LOG_COMPONENT_TERMINAL, "Terminal tab connected (%dx%d)", cols, rows);

  return true;
}

void
sk_terminal_tab_disconnect(SkTerminalTab *tab)
{
  g_return_if_fail(tab != NULL);

  if (!tab->connected)
  {
    return;
  }

  /* Remove IO watch. */
  if (tab->io_watch_id > 0)
  {
    g_source_remove(tab->io_watch_id);
    tab->io_watch_id = 0;
  }

  /* Disconnect VTE commit handler. */
  g_signal_handlers_disconnect_by_func(tab->vte, G_CALLBACK(on_vte_commit), tab);

  tab->conn = NULL;
  tab->channel = NULL;
  tab->connected = false;

  SK_LOG_INFO(SK_LOG_COMPONENT_TERMINAL, "Terminal tab disconnected");
}

void
sk_terminal_tab_feed(SkTerminalTab *tab, const char *data, gssize len)
{
  g_return_if_fail(tab != NULL);
  g_return_if_fail(data != NULL);

  if (len < 0)
  {
    len = (gssize)strlen(data);
  }

  vte_terminal_feed(tab->vte, data, len);
}

void
sk_terminal_tab_free(SkTerminalTab *tab)
{
  if (tab == NULL)
  {
    return;
  }

  sk_terminal_tab_disconnect(tab);

  /* Free deep-copied config strings. */
  g_free((char *)tab->config.font_family);
  g_free((char *)tab->config.word_chars);
  tab->config.font_family = NULL;
  tab->config.word_chars = NULL;

  if (tab->overlay != NULL)
  {
    g_object_unref(tab->overlay);
    tab->overlay = NULL;
  }

  g_free(tab);

  SK_LOG_DEBUG(SK_LOG_COMPONENT_TERMINAL, "Terminal tab freed");
}

/* ------------------------------------------------------------------ */
/* Widget access                                                       */
/* ------------------------------------------------------------------ */

GtkWidget *
sk_terminal_tab_get_widget(SkTerminalTab *tab)
{
  g_return_val_if_fail(tab != NULL, NULL);
  return tab->overlay;
}

GtkWidget *
sk_terminal_tab_get_vte(SkTerminalTab *tab)
{
  g_return_val_if_fail(tab != NULL, NULL);
  return GTK_WIDGET(tab->vte);
}

/* ------------------------------------------------------------------ */
/* Properties                                                          */
/* ------------------------------------------------------------------ */

void
sk_terminal_tab_get_size(const SkTerminalTab *tab, int *cols, int *rows)
{
  g_return_if_fail(tab != NULL);

  if (cols != NULL)
  {
    *cols = (int)vte_terminal_get_column_count(tab->vte);
  }
  if (rows != NULL)
  {
    *rows = (int)vte_terminal_get_row_count(tab->vte);
  }
}

bool
sk_terminal_tab_is_connected(const SkTerminalTab *tab)
{
  g_return_val_if_fail(tab != NULL, false);
  return tab->connected;
}

bool
sk_terminal_tab_is_dead(const SkTerminalTab *tab)
{
  g_return_val_if_fail(tab != NULL, false);
  return tab->is_dead;
}

/* ------------------------------------------------------------------ */
/* Config application (FR-TERMINAL-10..16)                             */
/* ------------------------------------------------------------------ */

void
sk_terminal_tab_apply_config(SkTerminalTab *tab, const SkTerminalConfig *config)
{
  g_return_if_fail(tab != NULL);
  g_return_if_fail(config != NULL);

  /* Free previous deep-copied strings before overwriting. */
  g_free((char *)tab->config.font_family);
  g_free((char *)tab->config.word_chars);

  /* Copy config with deep-copied strings for clear ownership. */
  tab->config = *config;
  if (tab->config.font_family == NULL)
  {
    tab->config.font_family = g_strdup(DEFAULT_CONFIG.font_family);
  }
  else
  {
    tab->config.font_family = g_strdup(tab->config.font_family);
  }
  tab->config.word_chars = g_strdup(tab->config.word_chars);
  if (tab->config.font_size <= 0)
  {
    tab->config.font_size = DEFAULT_CONFIG.font_size;
  }
  tab->default_font_size = tab->config.font_size;
  tab->current_font_size = tab->config.font_size;

  /* Font (FR-TERMINAL-10). */
  apply_font(tab);

  /* Scrollback (FR-TERMINAL-14). */
  vte_terminal_set_scrollback_lines(tab->vte, (glong)config->scrollback_lines);

  /* Cursor shape (FR-TERMINAL-13). */
  VteCursorShape vte_shape;
  switch (config->cursor_shape)
  {
  case SK_CURSOR_SHAPE_IBEAM:
    vte_shape = VTE_CURSOR_SHAPE_IBEAM;
    break;
  case SK_CURSOR_SHAPE_UNDERLINE:
    vte_shape = VTE_CURSOR_SHAPE_UNDERLINE;
    break;
  default:
    vte_shape = VTE_CURSOR_SHAPE_BLOCK;
    break;
  }
  vte_terminal_set_cursor_shape(tab->vte, vte_shape);

  /* Cursor blink (FR-TERMINAL-13). */
  vte_terminal_set_cursor_blink_mode(tab->vte, config->cursor_blink ? VTE_CURSOR_BLINK_ON
                                                                    : VTE_CURSOR_BLINK_OFF);

  /* Bold is bright. */
  vte_terminal_set_bold_is_bright(tab->vte, config->bold_is_bright);

  /* Allow hyperlinks (OSC 8). */
  vte_terminal_set_allow_hyperlink(tab->vte, config->allow_hyperlinks);

  /* Word chars for double-click selection. */
  if (config->word_chars != NULL)
  {
    vte_terminal_set_word_char_exceptions(tab->vte, config->word_chars);
  }

  /* Audible bell. */
  vte_terminal_set_audible_bell(tab->vte, config->audible_bell);

  /* Mouse autohide. */
  vte_terminal_set_mouse_autohide(tab->vte, TRUE);

  /* Scroll on output / keystroke. */
  vte_terminal_set_scroll_on_output(tab->vte, FALSE);
  vte_terminal_set_scroll_on_keystroke(tab->vte, TRUE);

  SK_LOG_DEBUG(SK_LOG_COMPONENT_TERMINAL, "Config applied: font=%s %d, scrollback=%d, cursor=%d",
               tab->config.font_family, tab->config.font_size, config->scrollback_lines,
               (int)config->cursor_shape);
}

void
sk_terminal_tab_apply_theme(SkTerminalTab *tab, const SkTerminalTheme *theme)
{
  g_return_if_fail(tab != NULL);
  g_return_if_fail(theme != NULL);

  /* FR-TERMINAL-11: Apply 16-color palette + fg/bg. */
  vte_terminal_set_colors(tab->vte, &theme->foreground, &theme->background, theme->palette, 16);

  /* Cursor color. */
  if (theme->has_cursor_color)
  {
    vte_terminal_set_color_cursor(tab->vte, &theme->cursor_color);
  }
  if (theme->has_cursor_fg)
  {
    vte_terminal_set_color_cursor_foreground(tab->vte, &theme->cursor_fg);
  }

  /* Highlight (selection) colors. */
  if (theme->has_highlight_bg)
  {
    vte_terminal_set_color_highlight(tab->vte, &theme->highlight_bg);
  }
  if (theme->has_highlight_fg)
  {
    vte_terminal_set_color_highlight_foreground(tab->vte, &theme->highlight_fg);
  }

  SK_LOG_DEBUG(SK_LOG_COMPONENT_TERMINAL, "Theme applied: %s",
               theme->name ? theme->name : "(unnamed)");
}

static void
apply_font(SkTerminalTab *tab)
{
  g_autofree char *font_desc_str =
      g_strdup_printf("%s %d", tab->config.font_family, tab->current_font_size);
  PangoFontDescription *font_desc = pango_font_description_from_string(font_desc_str);
  vte_terminal_set_font(tab->vte, font_desc);
  pango_font_description_free(font_desc);
}

/* ------------------------------------------------------------------ */
/* Selection & copy (FR-TERMINAL-03..05, FR-TABS-11..12)               */
/* ------------------------------------------------------------------ */

void
sk_terminal_tab_copy_clipboard(SkTerminalTab *tab)
{
  g_return_if_fail(tab != NULL);

  /* FR-TERMINAL-04: Copy contains only terminal content, no chrome. */
  vte_terminal_copy_clipboard_format(tab->vte, VTE_FORMAT_TEXT);

  SK_LOG_DEBUG(SK_LOG_COMPONENT_TERMINAL, "Copied selection to clipboard");
}

void
sk_terminal_tab_paste_clipboard(SkTerminalTab *tab)
{
  g_return_if_fail(tab != NULL);

  if (tab->is_dead)
  {
    return; /* Read-only in dead mode. */
  }

  vte_terminal_paste_clipboard(tab->vte);
}

void
sk_terminal_tab_copy_all(SkTerminalTab *tab)
{
  g_return_if_fail(tab != NULL);

  /* FR-TABS-12: Copy entire scrollback to clipboard. */
  /* Get all text from the visible area plus scrollback.
   * VTE stores scrollback above the visible area (negative row indices). */
  glong visible_rows = vte_terminal_get_row_count(tab->vte);
  glong cols = vte_terminal_get_column_count(tab->vte);
  glong scroll_rows = (glong)tab->config.scrollback_lines;

  char *text = vte_terminal_get_text_range(
      tab->vte, -(glong)scroll_rows, 0,                     /* Start from top of scrollback. */
      (glong)visible_rows - 1, (glong)cols - 1, NULL, NULL, /* No callback filter. */
      NULL);                                                /* No attributes. */

  if (text != NULL)
  {
    GtkClipboard *clipboard = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    gtk_clipboard_set_text(clipboard, text, -1);
    g_free(text);

    SK_LOG_DEBUG(SK_LOG_COMPONENT_TERMINAL, "Copied entire scrollback to clipboard");
  }
}

bool
sk_terminal_tab_export_scrollback(SkTerminalTab *tab, GtkWindow *parent)
{
  g_return_val_if_fail(tab != NULL, false);

  /* FR-TERMINAL-18: Export scrollback to file. */
  GtkWidget *dialog = gtk_file_chooser_dialog_new(
      _("Export Scrollback"), parent, GTK_FILE_CHOOSER_ACTION_SAVE, _("_Cancel"),
      GTK_RESPONSE_CANCEL, _("_Save"), GTK_RESPONSE_ACCEPT, NULL);

  gtk_file_chooser_set_do_overwrite_confirmation(GTK_FILE_CHOOSER(dialog), TRUE);
  gtk_file_chooser_set_current_name(GTK_FILE_CHOOSER(dialog), _("scrollback.txt"));

  gint response = gtk_dialog_run(GTK_DIALOG(dialog));

  if (response != GTK_RESPONSE_ACCEPT)
  {
    gtk_widget_destroy(dialog);
    return false;
  }

  g_autofree char *filename = gtk_file_chooser_get_filename(GTK_FILE_CHOOSER(dialog));
  gtk_widget_destroy(dialog);

  /* Get scrollback text. */
  glong scroll_rows = (glong)tab->config.scrollback_lines;
  glong visible_rows = vte_terminal_get_row_count(tab->vte);
  glong cols = vte_terminal_get_column_count(tab->vte);

  char *text = vte_terminal_get_text_range(
      tab->vte, -(glong)scroll_rows, 0, (glong)visible_rows - 1, (glong)cols - 1, NULL, NULL, NULL);

  if (text == NULL)
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_TERMINAL, "No scrollback text to export");
    return false;
  }

  GError *err = NULL;
  gboolean ok = g_file_set_contents(filename, text, -1, &err);
  g_free(text);

  if (!ok)
  {
    SK_LOG_ERROR(SK_LOG_COMPONENT_TERMINAL, "Failed to export scrollback: %s",
                 err ? err->message : "unknown");
    g_clear_error(&err);
    return false;
  }

  /* INV-SECURITY-3: scrollback may contain sensitive data; enforce 0600. */
  chmod(filename, 0600);

  SK_LOG_INFO(SK_LOG_COMPONENT_TERMINAL, "Scrollback exported to file");
  return true;
}

/* ------------------------------------------------------------------ */
/* Zoom (FR-TABS-13)                                                   */
/* ------------------------------------------------------------------ */

void
sk_terminal_tab_zoom_in(SkTerminalTab *tab)
{
  g_return_if_fail(tab != NULL);

  if (tab->current_font_size < 72)
  {
    tab->current_font_size++;
    apply_font(tab);
  }
}

void
sk_terminal_tab_zoom_out(SkTerminalTab *tab)
{
  g_return_if_fail(tab != NULL);

  if (tab->current_font_size > 4)
  {
    tab->current_font_size--;
    apply_font(tab);
  }
}

void
sk_terminal_tab_zoom_reset(SkTerminalTab *tab)
{
  g_return_if_fail(tab != NULL);

  tab->current_font_size = tab->default_font_size;
  apply_font(tab);
}

/* ------------------------------------------------------------------ */
/* I/O routing callbacks (FR-TERMINAL-01..03)                          */
/* ------------------------------------------------------------------ */

/**
 * Called by g_io_add_watch() when data is available on the SSH fd.
 * Reads from SSH channel and feeds into VTE.
 */
static gboolean
on_ssh_data_available(GIOChannel *source, GIOCondition condition, gpointer user_data)
{
  (void)source;
  SkTerminalTab *tab = user_data;

  if (!tab->connected || tab->channel == NULL)
  {
    return G_SOURCE_REMOVE;
  }

  if (condition & (G_IO_ERR | G_IO_HUP | G_IO_NVAL))
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_TERMINAL, "SSH IO error condition: 0x%x", (unsigned)condition);
    tab->connected = false;
    tab->io_watch_id = 0;
    return G_SOURCE_REMOVE;
  }

  /* Read data from SSH channel. */
  char buf[16384];
  int nbytes = sk_ssh_channel_read_nonblocking(tab->channel, buf, sizeof(buf));

  if (nbytes > 0)
  {
    /* Feed data into VTE terminal. */
    vte_terminal_feed(tab->vte, buf, (gssize)nbytes);
  }
  else if (nbytes < 0)
  {
    /* EOF or error -- channel closed. */
    SK_LOG_INFO(SK_LOG_COMPONENT_TERMINAL, "SSH channel EOF/error during read");
    tab->connected = false;
    tab->io_watch_id = 0;
    return G_SOURCE_REMOVE;
  }
  /* nbytes == 0: no data available right now, keep watching. */

  return G_SOURCE_CONTINUE;
}

/**
 * Called when VTE has output from user input to send to the remote.
 * FR-TERMINAL-01: Everything except intercepted shortcuts goes to SSH.
 */
static void
on_vte_commit(VteTerminal *vte, gchar *text, guint size, gpointer user_data)
{
  (void)vte;
  SkTerminalTab *tab = user_data;

  if (!tab->connected || tab->channel == NULL)
  {
    return;
  }

  if (tab->is_dead)
  {
    return; /* Read-only in dead mode. */
  }

  /* Write user input to SSH channel. */
  int written = sk_ssh_channel_write(tab->channel, text, (size_t)size);
  if (written < 0)
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_TERMINAL, "Failed to write to SSH channel");
  }
}

/* ------------------------------------------------------------------ */
/* Input routing -- key press handler (FR-TERMINAL-01..03, FR-TABS-14) */
/* ------------------------------------------------------------------ */

/**
 * Intercept only Ctrl+Shift shortcuts. Everything else passes through
 * to the remote session via VTE's normal key handling.
 *
 * FR-TABS-14: All shortcuts use Ctrl+Shift prefix to avoid conflicts
 * with remote applications (vim, nano, bash).
 * FR-TABS-16: All non-reserved shortcuts pass directly to remote.
 */
static gboolean
on_key_press(GtkWidget *widget, GdkEventKey *event, gpointer user_data)
{
  (void)widget;
  SkTerminalTab *tab = user_data;

  /* Only intercept Ctrl+Shift combinations. */
  guint state = event->state & (GDK_CONTROL_MASK | GDK_SHIFT_MASK | GDK_MOD1_MASK);

  if (state != (GDK_CONTROL_MASK | GDK_SHIFT_MASK))
  {
    /* Not Ctrl+Shift -- pass through to VTE/remote (FR-TERMINAL-01). */
    return FALSE;
  }

  guint key = gdk_keyval_to_upper(event->keyval);

  switch (key)
  {
  case GDK_KEY_C:
    /* Ctrl+Shift+C: Copy to clipboard (FR-TABS-11). */
    sk_terminal_tab_copy_clipboard(tab);
    return TRUE;

  case GDK_KEY_V:
    /* Ctrl+Shift+V: Paste from clipboard (FR-TABS-11). */
    sk_terminal_tab_paste_clipboard(tab);
    return TRUE;

  case GDK_KEY_A:
    /* Ctrl+Shift+A: Copy all scrollback (FR-TABS-12). */
    sk_terminal_tab_copy_all(tab);
    return TRUE;

  case GDK_KEY_F:
    /* Ctrl+Shift+F: Toggle search bar (FR-TERMINAL-07, FR-TABS-09). */
    sk_terminal_tab_toggle_search(tab);
    return TRUE;

  case GDK_KEY_plus:
  case GDK_KEY_equal:
    /* Ctrl+Shift+Plus: Zoom in (FR-TABS-13). */
    sk_terminal_tab_zoom_in(tab);
    return TRUE;

  case GDK_KEY_minus:
  case GDK_KEY_underscore:
    /* Ctrl+Shift+Minus: Zoom out (FR-TABS-13). */
    sk_terminal_tab_zoom_out(tab);
    return TRUE;

  case GDK_KEY_0:
  case GDK_KEY_parenright:
    /* Ctrl+Shift+0: Zoom reset (FR-TABS-13). */
    sk_terminal_tab_zoom_reset(tab);
    return TRUE;

  default:
    /* Other Ctrl+Shift combos: pass through.
     * Tab management shortcuts (T, W, N, Tab) are handled by the
     * window/UI layer, not the terminal layer. */
    return FALSE;
  }
}

/* ------------------------------------------------------------------ */
/* Resize handling (FR-TERMINAL-17..18)                                */
/* ------------------------------------------------------------------ */

/**
 * Called when VTE widget is resized. Propagate new dimensions to the
 * SSH PTY so the remote gets SIGWINCH (FR-TERMINAL-16).
 */
static void
on_size_allocate(GtkWidget *widget, GdkRectangle *allocation, gpointer user_data)
{
  (void)allocation;
  SkTerminalTab *tab = user_data;

  int cols = (int)vte_terminal_get_column_count(VTE_TERMINAL(widget));
  int rows = (int)vte_terminal_get_row_count(VTE_TERMINAL(widget));

  /* Only send resize if dimensions actually changed. */
  if (cols == tab->last_cols && rows == tab->last_rows)
  {
    return;
  }

  tab->last_cols = cols;
  tab->last_rows = rows;

  if (!tab->connected || tab->channel == NULL)
  {
    return;
  }

  /* FR-TERMINAL-16: Explicit PTY resize via SSH channel. */
  GError *err = NULL;
  if (!sk_ssh_channel_resize_pty(tab->channel, cols, rows, &err))
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_TERMINAL, "PTY resize failed (%dx%d): %s", cols, rows,
                err ? err->message : "unknown");
    g_clear_error(&err);
  }
  else
  {
    SK_LOG_TRACE(SK_LOG_COMPONENT_TERMINAL, "PTY resized to %dx%d", cols, rows);
  }
}

/* ------------------------------------------------------------------ */
/* Selection (FR-TERMINAL-03..05)                                      */
/* ------------------------------------------------------------------ */

/**
 * FR-TERMINAL-03: Selection = primary copy (VTE default behavior).
 * VTE automatically copies to PRIMARY selection on mouse select.
 * This handler is a hook point for future customization.
 */
static void
on_selection_changed(VteTerminal *vte, gpointer user_data)
{
  (void)vte;
  (void)user_data;
  /* VTE handles primary selection automatically (FR-TERMINAL-03).
   * FR-TERMINAL-05: VTE preserves selection on scroll by default. */
}

/* ------------------------------------------------------------------ */
/* Legacy API (kept for backward compatibility with sk_types.h)        */
/* ------------------------------------------------------------------ */

SkTerminal *
sk_terminal_new(SkSshChannel *channel)
{
  SkTerminal *term = g_new0(SkTerminal, 1);
  term->tab = sk_terminal_tab_new(NULL);
  term->channel = channel;
  return term;
}

GtkWidget *
sk_terminal_get_widget(SkTerminal *term)
{
  g_return_val_if_fail(term != NULL, NULL);
  return sk_terminal_tab_get_widget(term->tab);
}

bool
sk_terminal_start(SkTerminal *term, GError **error)
{
  (void)term;
  (void)error;
  /* Legacy: connection must be set up via sk_terminal_tab_connect(). */
  SK_LOG_WARN(SK_LOG_COMPONENT_TERMINAL, "sk_terminal_start() is deprecated; "
                                         "use sk_terminal_tab_connect()");
  return false;
}

void
sk_terminal_stop(SkTerminal *term)
{
  if (term != NULL && term->tab != NULL)
  {
    sk_terminal_tab_disconnect(term->tab);
  }
}

void
sk_terminal_free(SkTerminal *term)
{
  if (term == NULL)
  {
    return;
  }
  sk_terminal_tab_free(term->tab);
  g_free(term);
}

void
sk_terminal_get_size(const SkTerminal *term, int *cols, int *rows)
{
  if (term != NULL && term->tab != NULL)
  {
    sk_terminal_tab_get_size(term->tab, cols, rows);
  }
}

bool
sk_terminal_is_active(const SkTerminal *term)
{
  if (term != NULL && term->tab != NULL)
  {
    return sk_terminal_tab_is_connected(term->tab);
  }
  return false;
}
