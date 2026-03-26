// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_welcome.c
 * @brief Welcome screen shown on launch without arguments.
 *
 * Displays a user@host input field, recent connections list, empty
 * state for first-use with onboarding, and friendly client-id naming.
 *
 * Requirements: FR-UI-01..04
 */

#include "shellkeep/sk_i18n.h"
#include "shellkeep/sk_log.h"
#include "shellkeep/sk_types.h"
#include "shellkeep/sk_ui.h"

#include <atk/atk.h>
#include <gtk/gtk.h>

#include <string.h>

/* ------------------------------------------------------------------ */
/* Internal data for the welcome screen dialog                         */
/* ------------------------------------------------------------------ */

typedef struct
{
  GtkWidget *dialog;
  GtkWidget *host_entry;
  GtkWidget *client_name_entry;
  GtkWidget *listbox;
  const char **recent_hosts;
  const char **recent_users;
  const int *recent_ports;
  int n_recent;
  bool first_use;
} WelcomeData;

/* ------------------------------------------------------------------ */
/* Helpers                                                             */
/* ------------------------------------------------------------------ */

/**
 * Parse "user@host:port" or "user@host" or "host" into components.
 */
static void
parse_host_input(const char *input, char **out_user, char **out_host, int *out_port)
{
  *out_user = NULL;
  *out_host = NULL;
  *out_port = 0;

  if (input == NULL || input[0] == '\0')
    return;

  const char *at = strchr(input, '@');
  const char *host_start = input;

  if (at != NULL)
  {
    *out_user = g_strndup(input, (gsize)(at - input));
    host_start = at + 1;
  }

  const char *colon = strchr(host_start, ':');
  if (colon != NULL)
  {
    *out_host = g_strndup(host_start, (gsize)(colon - host_start));
    *out_port = atoi(colon + 1);
  }
  else
  {
    *out_host = g_strdup(host_start);
  }
}

/* ------------------------------------------------------------------ */
/* Signal handlers                                                     */
/* ------------------------------------------------------------------ */

static void
on_host_entry_activate(GtkEntry *entry G_GNUC_UNUSED, gpointer user_data)
{
  GtkDialog *dialog = GTK_DIALOG(user_data);
  gtk_dialog_response(dialog, GTK_RESPONSE_OK);
}

static void
on_recent_row_activated(GtkListBox *box G_GNUC_UNUSED, GtkListBoxRow *row, gpointer user_data)
{
  WelcomeData *data = user_data;
  int idx = gtk_list_box_row_get_index(row);

  if (idx >= 0 && idx < data->n_recent)
  {
    /* FR-UI-01: Click on recent connection connects immediately */
    const char *host = data->recent_hosts[idx];
    const char *user = data->recent_users[idx];
    int port = data->recent_ports[idx];

    char *text;
    if (user != NULL && user[0] != '\0')
    {
      if (port > 0 && port != 22)
      {
        text = g_strdup_printf("%s@%s:%d", user, host, port);
      }
      else
      {
        text = g_strdup_printf("%s@%s", user, host);
      }
    }
    else
    {
      if (port > 0 && port != 22)
      {
        text = g_strdup_printf("%s:%d", host, port);
      }
      else
      {
        text = g_strdup(host);
      }
    }

    gtk_entry_set_text(GTK_ENTRY(data->host_entry), text);
    g_free(text);

    gtk_dialog_response(GTK_DIALOG(data->dialog), GTK_RESPONSE_OK);
  }
}

/* ------------------------------------------------------------------ */
/* Welcome screen implementation                                       */
/* ------------------------------------------------------------------ */

SkWelcomeResult *
sk_welcome_screen_show(GtkWindow *parent, const char **recent, const char **recent_hosts,
                       const char **recent_users, const int *recent_ports, int n_recent,
                       bool first_use)
{
  GtkWidget *dialog = gtk_dialog_new_with_buttons(
      _("shellkeep"), parent, GTK_DIALOG_MODAL | GTK_DIALOG_DESTROY_WITH_PARENT, _("Connect"),
      GTK_RESPONSE_OK, _("Quit"), GTK_RESPONSE_CANCEL, NULL);

  gtk_dialog_set_default_response(GTK_DIALOG(dialog), GTK_RESPONSE_OK);
  gtk_window_set_default_size(GTK_WINDOW(dialog), 500, 450);

  GtkWidget *content = gtk_dialog_get_content_area(GTK_DIALOG(dialog));
  GtkStyleContext *ctx = gtk_widget_get_style_context(content);
  gtk_style_context_add_class(ctx, "welcome-screen");
  gtk_box_set_spacing(GTK_BOX(content), 12);

  /* App title */
  GtkWidget *title_label = gtk_label_new(NULL);
  /* Translators: This is the app subtitle shown on the welcome screen */
  char *title_markup = g_strdup_printf("<span size='xx-large' weight='bold'>shellkeep</span>\n"
                                       "<span size='small'>%s</span>",
                                       _("Persistent terminal sessions over SSH"));
  gtk_label_set_markup(GTK_LABEL(title_label), title_markup);
  g_free(title_markup);
  gtk_label_set_justify(GTK_LABEL(title_label), GTK_JUSTIFY_CENTER);
  gtk_widget_set_margin_top(title_label, 12);
  gtk_container_add(GTK_CONTAINER(content), title_label);

  /* First-use onboarding (FR-UI-03) */
  if (first_use)
  {
    GtkWidget *onboard = gtk_label_new(NULL);
    /* Translators: Onboarding steps shown on first use */
    char *onboard_markup =
        g_strdup_printf("<span size='small'>%s</span>",
                        _("1. Connect to a server\n"
                          "2. Your sessions persist even if you disconnect\n"
                          "3. Reconnect and everything is right where you left it"));
    gtk_label_set_markup(GTK_LABEL(onboard), onboard_markup);
    g_free(onboard_markup);
    gtk_label_set_line_wrap(GTK_LABEL(onboard), TRUE);
    gtk_widget_set_halign(onboard, GTK_ALIGN_CENTER);
    gtk_widget_set_margin_top(onboard, 4);
    gtk_widget_set_margin_bottom(onboard, 4);
    gtk_container_add(GTK_CONTAINER(content), onboard);

    /* FR-UI-03: Friendly client-id naming */
    GtkWidget *name_box = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 8);
    GtkWidget *name_label = gtk_label_new(_("Name this device:"));
    gtk_container_add(GTK_CONTAINER(name_box), name_label);

    GtkWidget *name_entry = gtk_entry_new();
    gtk_entry_set_placeholder_text(GTK_ENTRY(name_entry), _("e.g., My Desktop, Work Laptop"));
    gtk_widget_set_hexpand(name_entry, TRUE);
    gtk_container_add(GTK_CONTAINER(name_box), name_entry);

    gtk_widget_set_margin_start(name_box, 20);
    gtk_widget_set_margin_end(name_box, 20);
    gtk_container_add(GTK_CONTAINER(content), name_box);

    /* Store for later retrieval */
    g_object_set_data(G_OBJECT(dialog), "client-name-entry", name_entry);
  }

  /* Connection input (FR-UI-01) */
  GtkWidget *host_entry = gtk_entry_new();
  gtk_entry_set_placeholder_text(GTK_ENTRY(host_entry), _("user@hostname or IP address"));
  gtk_entry_set_activates_default(GTK_ENTRY(host_entry), TRUE);
  /* Accessibility: label for host entry */
  AtkObject *atk_host = gtk_widget_get_accessible(host_entry);
  if (atk_host != NULL)
  {
    atk_object_set_name(atk_host, _("SSH connection address"));
  }
  GtkStyleContext *entry_ctx = gtk_widget_get_style_context(host_entry);
  gtk_style_context_add_class(entry_ctx, "host-entry");
  gtk_widget_set_margin_start(host_entry, 40);
  gtk_widget_set_margin_end(host_entry, 40);
  g_signal_connect(host_entry, "activate", G_CALLBACK(on_host_entry_activate), dialog);
  gtk_container_add(GTK_CONTAINER(content), host_entry);

  /* Recent connections list (FR-UI-01, FR-UI-02) */
  GtkWidget *listbox = NULL;
  WelcomeData wdata = {
    .dialog = dialog,
    .host_entry = host_entry,
    .recent_hosts = recent_hosts,
    .recent_users = recent_users,
    .recent_ports = recent_ports,
    .n_recent = n_recent,
    .first_use = first_use,
  };

  if (n_recent > 0 && recent != NULL)
  {
    GtkWidget *recent_label = gtk_label_new(NULL);
    char *recent_markup = g_strdup_printf("<span weight='bold'>%s</span>", _("Recent connections"));
    gtk_label_set_markup(GTK_LABEL(recent_label), recent_markup);
    g_free(recent_markup);
    gtk_widget_set_halign(recent_label, GTK_ALIGN_START);
    gtk_widget_set_margin_start(recent_label, 40);
    gtk_widget_set_margin_top(recent_label, 8);
    gtk_container_add(GTK_CONTAINER(content), recent_label);

    GtkWidget *scrolled = gtk_scrolled_window_new(NULL, NULL);
    gtk_scrolled_window_set_policy(GTK_SCROLLED_WINDOW(scrolled), GTK_POLICY_NEVER,
                                   GTK_POLICY_AUTOMATIC);
    gtk_widget_set_vexpand(scrolled, TRUE);
    gtk_widget_set_margin_start(scrolled, 40);
    gtk_widget_set_margin_end(scrolled, 40);
    gtk_widget_set_margin_bottom(scrolled, 8);
    gtk_container_add(GTK_CONTAINER(content), scrolled);

    listbox = gtk_list_box_new();
    gtk_list_box_set_selection_mode(GTK_LIST_BOX(listbox), GTK_SELECTION_SINGLE);
    g_signal_connect(listbox, "row-activated", G_CALLBACK(on_recent_row_activated), &wdata);
    gtk_container_add(GTK_CONTAINER(scrolled), listbox);

    for (int i = 0; i < n_recent; i++)
    {
      GtkWidget *row_label = gtk_label_new(recent[i]);
      gtk_widget_set_halign(row_label, GTK_ALIGN_START);
      gtk_widget_set_margin_start(row_label, 8);
      gtk_widget_set_margin_end(row_label, 8);
      gtk_widget_set_margin_top(row_label, 6);
      gtk_widget_set_margin_bottom(row_label, 6);
      gtk_list_box_insert(GTK_LIST_BOX(listbox), row_label, -1);
    }
  }
  else if (!first_use)
  {
    /* Empty state but not first use */
    GtkWidget *empty = gtk_label_new(_("No recent connections.\n"
                                       "Enter a hostname above to get started."));
    gtk_widget_set_halign(empty, GTK_ALIGN_CENTER);
    gtk_widget_set_margin_top(empty, 20);
    gtk_container_add(GTK_CONTAINER(content), empty);
  }

  gtk_widget_show_all(dialog);
  gtk_widget_grab_focus(host_entry);

  int response = gtk_dialog_run(GTK_DIALOG(dialog));

  SkWelcomeResult *result = NULL;

  if (response == GTK_RESPONSE_OK)
  {
    const char *input = gtk_entry_get_text(GTK_ENTRY(host_entry));

    if (input != NULL && input[0] != '\0')
    {
      result = g_new0(SkWelcomeResult, 1);
      parse_host_input(input, &result->user, &result->host, &result->port);

      /* Retrieve client name if first use */
      if (first_use)
      {
        GtkWidget *name_entry = g_object_get_data(G_OBJECT(dialog), "client-name-entry");
        if (name_entry != NULL)
        {
          const char *name = gtk_entry_get_text(GTK_ENTRY(name_entry));
          if (name != NULL && name[0] != '\0')
          {
            result->client_name = g_strdup(name);
          }
        }
      }

      SK_LOG_INFO(SK_LOG_COMPONENT_UI, "welcome: connecting to %s%s%s",
                  result->user ? result->user : "", result->user ? "@" : "",
                  result->host ? result->host : "(null)");
    }
  }

  gtk_widget_destroy(dialog);

  return result;
}

void
sk_welcome_result_free(SkWelcomeResult *result)
{
  if (result == NULL)
    return;
  g_free(result->host);
  g_free(result->user);
  g_free(result->client_name);
  g_free(result);
}
