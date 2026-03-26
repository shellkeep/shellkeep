// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_terminal_dead.c
 * @brief Dead session rendering -- overlay banner and read-only mode.
 *
 * FR-HISTORY-05..08: Feed raw history into VTE via vte_terminal_feed().
 * Display banner overlay "This session has ended on the server." with
 * a "Create new session" button. Terminal becomes read-only (ignores input).
 */

#include "shellkeep/sk_i18n.h"
#include "shellkeep/sk_log.h"

#include "sk_terminal_internal.h"
#include <string.h>

/* Forward declaration. */
static void on_new_session_clicked(GtkButton *button, gpointer user_data);

/* ------------------------------------------------------------------ */
/* Dead session overlay creation                                       */
/* ------------------------------------------------------------------ */

/**
 * Create the dead session overlay banner.
 */
static GtkWidget *
create_dead_overlay(SkTerminalTab *tab, const char *message)
{
  /* Semi-transparent overlay at bottom of terminal. */
  GtkWidget *box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 8);
  gtk_widget_set_halign(box, GTK_ALIGN_CENTER);
  gtk_widget_set_valign(box, GTK_ALIGN_END);
  gtk_widget_set_margin_bottom(box, 20);
  gtk_widget_set_margin_start(box, 20);
  gtk_widget_set_margin_end(box, 20);

  /* Style with semi-transparent background. */
  GtkCssProvider *css = gtk_css_provider_new();
  gtk_css_provider_load_from_data(css,
                                  "box.dead-overlay {"
                                  "  background-color: rgba(40, 40, 40, 0.92);"
                                  "  border-radius: 8px;"
                                  "  padding: 16px 24px;"
                                  "  border: 1px solid rgba(255, 255, 255, 0.15);"
                                  "}"
                                  "box.dead-overlay label.dead-message {"
                                  "  color: #ff8c00;"
                                  "  font-weight: bold;"
                                  "  font-size: 14px;"
                                  "}"
                                  "box.dead-overlay label.dead-info {"
                                  "  color: rgba(255, 255, 255, 0.7);"
                                  "  font-size: 12px;"
                                  "}",
                                  -1, NULL);

  GtkStyleContext *ctx = gtk_widget_get_style_context(box);
  gtk_style_context_add_provider(ctx, GTK_STYLE_PROVIDER(css),
                                 GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
  gtk_style_context_add_class(ctx, "dead-overlay");

  /* Banner message. */
  GtkWidget *msg_label =
      gtk_label_new(message ? message : _("This session has ended on the server."));
  gtk_style_context_add_class(gtk_widget_get_style_context(msg_label), "dead-message");
  gtk_box_pack_start(GTK_BOX(box), msg_label, FALSE, FALSE, 0);

  /* Info text. */
  GtkWidget *info_label =
      gtk_label_new(_("The terminal content above is preserved from the session history.\n"
                      "You can scroll through it and copy text."));
  gtk_style_context_add_class(gtk_widget_get_style_context(info_label), "dead-info");
  gtk_label_set_justify(GTK_LABEL(info_label), GTK_JUSTIFY_CENTER);
  gtk_box_pack_start(GTK_BOX(box), info_label, FALSE, FALSE, 0);

  /* Propagate CSS provider to child labels. */
  gtk_style_context_add_provider(gtk_widget_get_style_context(msg_label), GTK_STYLE_PROVIDER(css),
                                 GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
  gtk_style_context_add_provider(gtk_widget_get_style_context(info_label), GTK_STYLE_PROVIDER(css),
                                 GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);

  g_object_unref(css);

  /* "Create new session" button. */
  GtkWidget *button = gtk_button_new_with_label(_("Create New Session"));
  gtk_widget_set_halign(button, GTK_ALIGN_CENTER);
  gtk_widget_set_tooltip_text(button, _("Start a new terminal session on the server"));
  g_signal_connect(button, "clicked", G_CALLBACK(on_new_session_clicked), tab);
  gtk_box_pack_start(GTK_BOX(box), button, FALSE, FALSE, 4);

  return box;
}

/* ------------------------------------------------------------------ */
/* Public: set dead mode                                               */
/* ------------------------------------------------------------------ */

void
sk_terminal_tab_set_dead(SkTerminalTab *tab, const char *history_data, gssize history_len,
                         const char *message)
{
  g_return_if_fail(tab != NULL);

  /* Disconnect if connected. */
  if (tab->connected)
  {
    sk_terminal_tab_disconnect(tab);
  }

  /* FR-HISTORY-05..08: Feed raw history into VTE to replay session. */
  if (history_data != NULL && history_len != 0)
  {
    if (history_len < 0)
    {
      history_len = (gssize)strlen(history_data);
    }

    /* Reset the terminal before feeding history. */
    vte_terminal_reset(tab->vte, TRUE, TRUE);

    /* Feed in chunks to avoid issues with very large buffers. */
    const gssize chunk_size = 65536;
    gssize offset = 0;
    while (offset < history_len)
    {
      gssize remaining = history_len - offset;
      gssize this_chunk = remaining < chunk_size ? remaining : chunk_size;
      vte_terminal_feed(tab->vte, history_data + offset, this_chunk);
      offset += this_chunk;
    }

    SK_LOG_INFO(SK_LOG_COMPONENT_TERMINAL, "Fed %zd bytes of history into dead session VTE",
                (ssize_t)history_len);
  }

  /* Set read-only mode. */
  tab->is_dead = true;

  /* Remove existing dead overlay if any. */
  if (tab->dead_overlay != NULL)
  {
    gtk_widget_destroy(tab->dead_overlay);
    tab->dead_overlay = NULL;
  }

  /* Create and show the dead session overlay banner. */
  tab->dead_overlay = create_dead_overlay(tab, message);
  gtk_overlay_add_overlay(GTK_OVERLAY(tab->overlay), tab->dead_overlay);
  gtk_widget_show_all(tab->dead_overlay);

  SK_LOG_INFO(SK_LOG_COMPONENT_TERMINAL, "Terminal tab set to dead session mode");
}

void
sk_terminal_tab_set_new_session_cb(SkTerminalTab *tab, SkTerminalNewSessionCb callback,
                                   gpointer user_data)
{
  g_return_if_fail(tab != NULL);
  tab->new_session_cb = callback;
  tab->new_session_data = user_data;
}

/* ------------------------------------------------------------------ */
/* Callbacks                                                           */
/* ------------------------------------------------------------------ */

static void
on_new_session_clicked(GtkButton *button, gpointer user_data)
{
  (void)button;
  SkTerminalTab *tab = user_data;

  if (tab->new_session_cb != NULL)
  {
    tab->new_session_cb(tab, tab->new_session_data);
  }
}
