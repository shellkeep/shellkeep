// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_terminal_search.c
 * @brief Scrollback search overlay for the terminal tab.
 *
 * FR-TERMINAL-07: Ctrl+Shift+F opens overlay search bar on current tab.
 * Search is local scrollback (no server round trip). Results highlighted
 * inline. Enter/Shift+Enter navigates matches. Esc closes search.
 */

#include "shellkeep/sk_i18n.h"
#include "shellkeep/sk_log.h"

#include "sk_terminal_internal.h"
#include <atk/atk.h>
#include <string.h>

/* PCRE2 flags used by vte_regex_new_for_search().
 * Defined here in case vte/vte.h does not transitively include pcre2.h. */
#ifndef PCRE2_CASELESS
#define PCRE2_CASELESS 0x00000008u
#endif
#ifndef PCRE2_MULTILINE
#define PCRE2_MULTILINE 0x00000400u
#endif

/* Forward declarations. */
static void on_search_entry_changed(GtkSearchEntry *entry, gpointer user_data);
static gboolean on_search_key_press(GtkWidget *widget, GdkEventKey *event, gpointer user_data);
static void on_search_next(GtkButton *button, gpointer user_data);
static void on_search_prev(GtkButton *button, gpointer user_data);
static void on_search_close(GtkButton *button, gpointer user_data);
static void update_search(SkTerminalTab *tab);

/* ------------------------------------------------------------------ */
/* Search bar creation                                                 */
/* ------------------------------------------------------------------ */

/**
 * Lazily create the search bar overlay.
 */
static void
ensure_search_bar(SkTerminalTab *tab)
{
  if (tab->search_bar != NULL)
  {
    return;
  }

  /* Horizontal box containing search widgets. */
  GtkWidget *hbox = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 4);
  gtk_widget_set_margin_start(hbox, 8);
  gtk_widget_set_margin_end(hbox, 8);
  gtk_widget_set_margin_top(hbox, 4);
  gtk_widget_set_margin_bottom(hbox, 4);

  /* Search entry. */
  tab->search_entry = gtk_search_entry_new();
  gtk_widget_set_hexpand(tab->search_entry, TRUE);
  gtk_entry_set_placeholder_text(GTK_ENTRY(tab->search_entry), _("Search scrollback..."));
  /* Accessibility: label for search entry */
  AtkObject *atk_search = gtk_widget_get_accessible(tab->search_entry);
  if (atk_search != NULL)
  {
    atk_object_set_name(atk_search, _("Search scrollback"));
  }
  gtk_box_pack_start(GTK_BOX(hbox), tab->search_entry, TRUE, TRUE, 0);

  /* Match count label. */
  tab->search_label = gtk_label_new("");
  gtk_widget_set_margin_start(tab->search_label, 4);
  gtk_widget_set_margin_end(tab->search_label, 4);
  gtk_box_pack_start(GTK_BOX(hbox), tab->search_label, FALSE, FALSE, 0);

  /* Previous match button. */
  GtkWidget *btn_prev =
      gtk_button_new_from_icon_name("go-up-symbolic", GTK_ICON_SIZE_SMALL_TOOLBAR);
  gtk_widget_set_tooltip_text(btn_prev, _("Previous match (Shift+Enter)"));
  g_signal_connect(btn_prev, "clicked", G_CALLBACK(on_search_prev), tab);
  gtk_box_pack_start(GTK_BOX(hbox), btn_prev, FALSE, FALSE, 0);

  /* Next match button. */
  GtkWidget *btn_next =
      gtk_button_new_from_icon_name("go-down-symbolic", GTK_ICON_SIZE_SMALL_TOOLBAR);
  gtk_widget_set_tooltip_text(btn_next, _("Next match (Enter)"));
  g_signal_connect(btn_next, "clicked", G_CALLBACK(on_search_next), tab);
  gtk_box_pack_start(GTK_BOX(hbox), btn_next, FALSE, FALSE, 0);

  /* Close button. */
  GtkWidget *btn_close =
      gtk_button_new_from_icon_name("window-close-symbolic", GTK_ICON_SIZE_SMALL_TOOLBAR);
  gtk_widget_set_tooltip_text(btn_close, _("Close search (Esc)"));
  g_signal_connect(btn_close, "clicked", G_CALLBACK(on_search_close), tab);
  gtk_box_pack_start(GTK_BOX(hbox), btn_close, FALSE, FALSE, 0);

  /* Style the search bar with a frame. */
  GtkWidget *frame = gtk_frame_new(NULL);
  gtk_container_add(GTK_CONTAINER(frame), hbox);

  /* CSS styling for the search bar background. */
  GtkCssProvider *css = gtk_css_provider_new();
  gtk_css_provider_load_from_data(css,
                                  "frame { background: @theme_bg_color; "
                                  "border-radius: 0 0 6px 6px; "
                                  "padding: 2px; }",
                                  -1, NULL);
  gtk_style_context_add_provider(gtk_widget_get_style_context(frame), GTK_STYLE_PROVIDER(css),
                                 GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
  g_object_unref(css);

  /* Position at top of overlay. */
  gtk_widget_set_halign(frame, GTK_ALIGN_FILL);
  gtk_widget_set_valign(frame, GTK_ALIGN_START);

  tab->search_bar = frame;
  gtk_overlay_add_overlay(GTK_OVERLAY(tab->overlay), frame);

  /* Connect signals. */
  g_signal_connect(tab->search_entry, "search-changed", G_CALLBACK(on_search_entry_changed), tab);
  g_signal_connect(tab->search_entry, "key-press-event", G_CALLBACK(on_search_key_press), tab);

  gtk_widget_show_all(frame);
  gtk_widget_set_visible(frame, FALSE);
}

/* ------------------------------------------------------------------ */
/* Public: toggle search                                               */
/* ------------------------------------------------------------------ */

void
sk_terminal_tab_toggle_search(SkTerminalTab *tab)
{
  g_return_if_fail(tab != NULL);

  ensure_search_bar(tab);

  tab->search_visible = !tab->search_visible;
  gtk_widget_set_visible(tab->search_bar, tab->search_visible);

  if (tab->search_visible)
  {
    gtk_widget_grab_focus(tab->search_entry);
    update_search(tab);
  }
  else
  {
    /* Clear search highlighting when closing. */
    vte_terminal_search_set_regex(tab->vte, NULL, 0);
    gtk_label_set_text(GTK_LABEL(tab->search_label), "");
    gtk_widget_grab_focus(GTK_WIDGET(tab->vte));
  }
}

bool
sk_terminal_tab_search_is_visible(const SkTerminalTab *tab)
{
  g_return_val_if_fail(tab != NULL, false);
  return tab->search_visible;
}

/* ------------------------------------------------------------------ */
/* Search logic                                                        */
/* ------------------------------------------------------------------ */

static void
update_search(SkTerminalTab *tab)
{
  const char *text = gtk_entry_get_text(GTK_ENTRY(tab->search_entry));

  if (text == NULL || text[0] == '\0')
  {
    vte_terminal_search_set_regex(tab->vte, NULL, 0);
    gtk_label_set_text(GTK_LABEL(tab->search_label), "");
    return;
  }

  /* Escape the search text for use as a regex literal. */
  g_autofree char *escaped = g_regex_escape_string(text, -1);

  GError *err = NULL;
  VteRegex *regex = vte_regex_new_for_search(escaped, -1, PCRE2_CASELESS | PCRE2_MULTILINE, &err);
  if (regex == NULL)
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_TERMINAL, "Search regex error: %s",
                err ? err->message : "unknown");
    g_clear_error(&err);
    gtk_label_set_text(GTK_LABEL(tab->search_label), _("Invalid"));
    return;
  }

  vte_terminal_search_set_regex(tab->vte, regex, 0);
  vte_terminal_search_set_wrap_around(tab->vte, TRUE);
  vte_regex_unref(regex);

  /* Try to find first match. */
  gboolean found = vte_terminal_search_find_next(tab->vte);
  if (found)
  {
    gtk_label_set_text(GTK_LABEL(tab->search_label), _("Found"));
  }
  else
  {
    gtk_label_set_text(GTK_LABEL(tab->search_label), _("No matches"));
  }
}

/* ------------------------------------------------------------------ */
/* Search callbacks                                                    */
/* ------------------------------------------------------------------ */

static void
on_search_entry_changed(GtkSearchEntry *entry, gpointer user_data)
{
  (void)entry;
  SkTerminalTab *tab = user_data;
  update_search(tab);
}

static gboolean
on_search_key_press(GtkWidget *widget, GdkEventKey *event, gpointer user_data)
{
  (void)widget;
  SkTerminalTab *tab = user_data;

  switch (event->keyval)
  {
  case GDK_KEY_Escape:
    /* Esc: Close search. */
    sk_terminal_tab_toggle_search(tab);
    return TRUE;

  case GDK_KEY_Return:
  case GDK_KEY_KP_Enter:
    if (event->state & GDK_SHIFT_MASK)
    {
      /* Shift+Enter: Previous match. */
      vte_terminal_search_find_previous(tab->vte);
    }
    else
    {
      /* Enter: Next match. */
      vte_terminal_search_find_next(tab->vte);
    }
    return TRUE;

  default:
    return FALSE;
  }
}

static void
on_search_next(GtkButton *button, gpointer user_data)
{
  (void)button;
  SkTerminalTab *tab = user_data;
  vte_terminal_search_find_next(tab->vte);
}

static void
on_search_prev(GtkButton *button, gpointer user_data)
{
  (void)button;
  SkTerminalTab *tab = user_data;
  vte_terminal_search_find_previous(tab->vte);
}

static void
on_search_close(GtkButton *button, gpointer user_data)
{
  (void)button;
  SkTerminalTab *tab = user_data;
  if (tab->search_visible)
  {
    sk_terminal_tab_toggle_search(tab);
  }
}
