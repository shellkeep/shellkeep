// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_dialogs.c
 * @brief GTK dialog implementations for shellkeep.
 *
 * Implements all modal dialogs: host key verification, authentication,
 * lock conflict, environment selection, close window, and general
 * error/info dialogs.
 *
 * Requirements: FR-CONN-01..05, FR-CONN-09..10, FR-LOCK-05,
 *               FR-ENV-03..05, FR-TABS-17
 */

#include "shellkeep/sk_i18n.h"
#include "shellkeep/sk_log.h"
#include "shellkeep/sk_ui.h"

#include <atk/atk.h>
#include <gtk/gtk.h>

#include <string.h>

/* ------------------------------------------------------------------ */
/* Host key dialogs (FR-CONN-01..05)                                   */
/* ------------------------------------------------------------------ */

SkHostKeyDialogResult
sk_dialog_host_key_unknown(GtkWindow *parent, const char *hostname, const char *fingerprint,
                           const char *key_type)
{
  /* FR-CONN-03: TOFU dialog with fingerprint and options */
  /* Translators: %1$s is hostname, %2$s is key type, %3$s is fingerprint */
  char *message = g_strdup_printf(_("The authenticity of host '%1$s' cannot be established.\n\n"
                                    "Key type: %2$s\n"
                                    "Fingerprint:\n  %3$s\n\n"
                                    "Are you sure you want to continue connecting?"),
                                  hostname, key_type != NULL ? key_type : _("unknown"),
                                  fingerprint != NULL ? fingerprint : _("unknown"));

  GtkWidget *dialog = gtk_dialog_new_with_buttons(
      _("Unknown Host Key"), parent, GTK_DIALOG_MODAL | GTK_DIALOG_DESTROY_WITH_PARENT,
      _("Accept and save"), 1, _("Connect once"), 2, _("Cancel"), GTK_RESPONSE_CANCEL, NULL);

  /* Default to Cancel (safe action) */
  gtk_dialog_set_default_response(GTK_DIALOG(dialog), GTK_RESPONSE_CANCEL);

  GtkWidget *content = gtk_dialog_get_content_area(GTK_DIALOG(dialog));
  GtkWidget *label = gtk_label_new(message);
  gtk_label_set_selectable(GTK_LABEL(label), TRUE);
  gtk_label_set_line_wrap(GTK_LABEL(label), TRUE);
  gtk_widget_set_margin_start(label, 20);
  gtk_widget_set_margin_end(label, 20);
  gtk_widget_set_margin_top(label, 20);
  gtk_widget_set_margin_bottom(label, 20);

  /* Accessibility: set accessible description for host key fingerprint */
  AtkObject *atk_label = gtk_widget_get_accessible(label);
  if (atk_label != NULL)
  {
    atk_object_set_name(atk_label, _("Host key fingerprint details"));
  }

  gtk_container_add(GTK_CONTAINER(content), label);
  gtk_widget_show_all(dialog);

  int response = gtk_dialog_run(GTK_DIALOG(dialog));
  gtk_widget_destroy(dialog);
  g_free(message);

  SK_LOG_INFO(SK_LOG_COMPONENT_UI, "host key unknown dialog for '%s': response=%d", hostname,
              response);

  switch (response)
  {
  case 1:
    return SK_HOST_KEY_ACCEPT_SAVE;
  case 2:
    return SK_HOST_KEY_CONNECT_ONCE;
  default:
    return SK_HOST_KEY_REJECT;
  }
}

void
sk_dialog_host_key_changed(GtkWindow *parent, const char *hostname, const char *old_fingerprint,
                           const char *new_fingerprint, const char *key_type)
{
  /* FR-CONN-02: Block, no override button */
  /* Translators: %1$s is hostname, %2$s is key type, %3$s is old fingerprint,
   * %4$s is new fingerprint, %5$s is hostname (repeated for command) */
  char *message =
      g_strdup_printf(_("WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!\n\n"
                        "Someone could be eavesdropping on you right now "
                        "(man-in-the-middle attack)!\n\n"
                        "Host: %1$s\n"
                        "Key type: %2$s\n\n"
                        "Previous fingerprint:\n  %3$s\n\n"
                        "Current fingerprint:\n  %4$s\n\n"
                        "The connection has been blocked for your security.\n"
                        "To resolve this, remove the old key from your known_hosts file:\n\n"
                        "  ssh-keygen -R %5$s"),
                      hostname, key_type != NULL ? key_type : _("unknown"),
                      old_fingerprint != NULL ? old_fingerprint : _("(not available)"),
                      new_fingerprint != NULL ? new_fingerprint : _("unknown"), hostname);

  GtkWidget *dialog =
      gtk_message_dialog_new(parent, GTK_DIALOG_MODAL | GTK_DIALOG_DESTROY_WITH_PARENT,
                             GTK_MESSAGE_ERROR, GTK_BUTTONS_OK, "%s", _("Host Key Has Changed"));

  gtk_message_dialog_format_secondary_text(GTK_MESSAGE_DIALOG(dialog), "%s", message);

  SK_LOG_WARN(SK_LOG_COMPONENT_UI, "host key CHANGED for '%s' -- connection blocked", hostname);

  gtk_dialog_run(GTK_DIALOG(dialog));
  gtk_widget_destroy(dialog);
  g_free(message);
}

/* ------------------------------------------------------------------ */
/* Authentication dialogs (FR-CONN-09..10)                             */
/* ------------------------------------------------------------------ */

char *
sk_dialog_auth_password(GtkWindow *parent, const char *prompt)
{
  /* FR-CONN-09: Password dialog with masked field */
  GtkWidget *dialog = gtk_dialog_new_with_buttons(
      _("Authentication Required"), parent, GTK_DIALOG_MODAL | GTK_DIALOG_DESTROY_WITH_PARENT,
      _("OK"), GTK_RESPONSE_OK, _("Cancel"), GTK_RESPONSE_CANCEL, NULL);

  gtk_dialog_set_default_response(GTK_DIALOG(dialog), GTK_RESPONSE_OK);

  GtkWidget *content = gtk_dialog_get_content_area(GTK_DIALOG(dialog));
  gtk_box_set_spacing(GTK_BOX(content), 8);

  GtkWidget *label = gtk_label_new(prompt != NULL ? prompt : _("Password:"));
  gtk_widget_set_halign(label, GTK_ALIGN_START);
  gtk_widget_set_margin_start(label, 20);
  gtk_widget_set_margin_end(label, 20);
  gtk_widget_set_margin_top(label, 16);
  gtk_container_add(GTK_CONTAINER(content), label);

  GtkWidget *entry = gtk_entry_new();
  gtk_entry_set_visibility(GTK_ENTRY(entry), FALSE);
  gtk_entry_set_input_purpose(GTK_ENTRY(entry), GTK_INPUT_PURPOSE_PASSWORD);
  gtk_entry_set_activates_default(GTK_ENTRY(entry), TRUE);
  /* Accessibility: label for password entry */
  AtkObject *atk_entry = gtk_widget_get_accessible(entry);
  if (atk_entry != NULL)
  {
    atk_object_set_name(atk_entry, _("Password"));
  }
  gtk_widget_set_margin_start(entry, 20);
  gtk_widget_set_margin_end(entry, 20);
  gtk_widget_set_margin_bottom(entry, 16);
  gtk_container_add(GTK_CONTAINER(content), entry);

  gtk_widget_show_all(dialog);
  gtk_widget_grab_focus(entry);

  int response = gtk_dialog_run(GTK_DIALOG(dialog));
  char *password = NULL;

  if (response == GTK_RESPONSE_OK)
  {
    const char *text = gtk_entry_get_text(GTK_ENTRY(entry));
    if (text != NULL)
    {
      password = g_strdup(text);
    }
  }

  /* Clear the entry before destroying */
  gtk_entry_set_text(GTK_ENTRY(entry), "");
  gtk_widget_destroy(dialog);

  SK_LOG_DEBUG(SK_LOG_COMPONENT_UI, "password dialog: %s",
               password != NULL ? "provided" : "cancelled");

  return password;
}

char **
sk_dialog_auth_mfa(GtkWindow *parent, const char *name, const char *instruction,
                   const char **prompts, const gboolean *show_input, int n_prompts)
{
  /* FR-CONN-10: Keyboard-interactive MFA dialog */
  g_return_val_if_fail(n_prompts > 0, NULL);
  g_return_val_if_fail(prompts != NULL, NULL);

  /* Translators: %s is the authentication method name */
  char *title =
      g_strdup_printf(_("Authentication: %s"),
                      (name != NULL && name[0] != '\0') ? name : _("Verification Required"));

  GtkWidget *dialog =
      gtk_dialog_new_with_buttons(title, parent, GTK_DIALOG_MODAL | GTK_DIALOG_DESTROY_WITH_PARENT,
                                  _("OK"), GTK_RESPONSE_OK, _("Cancel"), GTK_RESPONSE_CANCEL, NULL);
  g_free(title);

  gtk_dialog_set_default_response(GTK_DIALOG(dialog), GTK_RESPONSE_OK);

  GtkWidget *content = gtk_dialog_get_content_area(GTK_DIALOG(dialog));
  gtk_box_set_spacing(GTK_BOX(content), 8);

  /* Instruction text */
  if (instruction != NULL && instruction[0] != '\0')
  {
    GtkWidget *instr_label = gtk_label_new(instruction);
    gtk_label_set_line_wrap(GTK_LABEL(instr_label), TRUE);
    gtk_widget_set_halign(instr_label, GTK_ALIGN_START);
    gtk_widget_set_margin_start(instr_label, 20);
    gtk_widget_set_margin_end(instr_label, 20);
    gtk_widget_set_margin_top(instr_label, 12);
    gtk_container_add(GTK_CONTAINER(content), instr_label);
  }

  /* Create entry fields for each prompt */
  GtkWidget **entries = g_new(GtkWidget *, n_prompts);

  for (int i = 0; i < n_prompts; i++)
  {
    GtkWidget *label = gtk_label_new(prompts[i] != NULL ? prompts[i] : _("Response:"));
    gtk_widget_set_halign(label, GTK_ALIGN_START);
    gtk_widget_set_margin_start(label, 20);
    gtk_widget_set_margin_end(label, 20);
    gtk_widget_set_margin_top(label, (i == 0 && instruction == NULL) ? 12 : 4);
    gtk_container_add(GTK_CONTAINER(content), label);

    entries[i] = gtk_entry_new();
    if (show_input != NULL && !show_input[i])
    {
      gtk_entry_set_visibility(GTK_ENTRY(entries[i]), FALSE);
      gtk_entry_set_input_purpose(GTK_ENTRY(entries[i]), GTK_INPUT_PURPOSE_PASSWORD);
    }
    /* Last entry activates default */
    if (i == n_prompts - 1)
    {
      gtk_entry_set_activates_default(GTK_ENTRY(entries[i]), TRUE);
    }
    gtk_widget_set_margin_start(entries[i], 20);
    gtk_widget_set_margin_end(entries[i], 20);
    gtk_widget_set_margin_bottom(entries[i], (i == n_prompts - 1) ? 16 : 4);
    gtk_container_add(GTK_CONTAINER(content), entries[i]);
  }

  gtk_widget_show_all(dialog);
  if (n_prompts > 0)
  {
    gtk_widget_grab_focus(entries[0]);
  }

  int response = gtk_dialog_run(GTK_DIALOG(dialog));
  char **results = NULL;

  if (response == GTK_RESPONSE_OK)
  {
    results = g_new0(char *, n_prompts + 1);
    for (int i = 0; i < n_prompts; i++)
    {
      const char *text = gtk_entry_get_text(GTK_ENTRY(entries[i]));
      results[i] = g_strdup(text != NULL ? text : "");
    }
  }

  /* Clear entries before destroying */
  for (int i = 0; i < n_prompts; i++)
  {
    gtk_entry_set_text(GTK_ENTRY(entries[i]), "");
  }
  g_free(entries);
  gtk_widget_destroy(dialog);

  SK_LOG_DEBUG(SK_LOG_COMPONENT_UI, "MFA dialog (%d prompts): %s", n_prompts,
               results != NULL ? "provided" : "cancelled");

  return results;
}

/* ------------------------------------------------------------------ */
/* Conflict dialog (FR-LOCK-05)                                        */
/* ------------------------------------------------------------------ */

bool
sk_dialog_conflict(GtkWindow *parent, const char *hostname, const char *connected_at)
{
  /* FR-LOCK-05: User-friendly language, no "client-id" term */
  /* Translators: %1$s is the hostname, %2$s is the connection timestamp */
  char *message =
      g_strdup_printf(_("Another shellkeep instance is currently connected to this server.\n\n"
                        "Connected from: %1$s\n"
                        "Since: %2$s\n\n"
                        "Only one connection per device identity is allowed at a time.\n\n"
                        "Would you like to disconnect the other instance and connect here?"),
                      hostname != NULL ? hostname : _("(unknown)"),
                      connected_at != NULL ? connected_at : _("(unknown)"));

  GtkWidget *dialog = gtk_dialog_new_with_buttons(
      _("Active Connection Detected"), parent, GTK_DIALOG_MODAL | GTK_DIALOG_DESTROY_WITH_PARENT,
      _("Disconnect and connect here"), GTK_RESPONSE_YES, _("Cancel"), GTK_RESPONSE_CANCEL, NULL);

  /* Default to Cancel (safe action) */
  gtk_dialog_set_default_response(GTK_DIALOG(dialog), GTK_RESPONSE_CANCEL);

  GtkWidget *content = gtk_dialog_get_content_area(GTK_DIALOG(dialog));
  GtkWidget *label = gtk_label_new(message);
  gtk_label_set_line_wrap(GTK_LABEL(label), TRUE);
  gtk_widget_set_margin_start(label, 20);
  gtk_widget_set_margin_end(label, 20);
  gtk_widget_set_margin_top(label, 20);
  gtk_widget_set_margin_bottom(label, 20);
  gtk_container_add(GTK_CONTAINER(content), label);
  gtk_widget_show_all(dialog);

  int response = gtk_dialog_run(GTK_DIALOG(dialog));
  gtk_widget_destroy(dialog);
  g_free(message);

  SK_LOG_INFO(SK_LOG_COMPONENT_UI, "conflict dialog: %s",
              response == GTK_RESPONSE_YES ? "takeover" : "cancel");

  return (response == GTK_RESPONSE_YES);
}

/* ------------------------------------------------------------------ */
/* Environment selection dialog (FR-ENV-03..05)                        */
/* ------------------------------------------------------------------ */

typedef struct
{
  GtkWidget *listbox;
  GtkWidget *search_entry;
  const char **envs;
  int n_envs;
} EnvDialogData;

static gboolean
env_filter_func(GtkListBoxRow *row, gpointer user_data)
{
  EnvDialogData *data = user_data;
  const char *filter = gtk_entry_get_text(GTK_ENTRY(data->search_entry));

  if (filter == NULL || filter[0] == '\0')
  {
    return TRUE;
  }

  GtkWidget *child = gtk_bin_get_child(GTK_BIN(row));
  if (!GTK_IS_LABEL(child))
    return TRUE;

  const char *text = gtk_label_get_text(GTK_LABEL(child));
  if (text == NULL)
    return FALSE;

  /* Case-insensitive substring match */
  char *lower_text = g_utf8_strdown(text, -1);
  char *lower_filter = g_utf8_strdown(filter, -1);
  gboolean match = (strstr(lower_text, lower_filter) != NULL);
  g_free(lower_text);
  g_free(lower_filter);

  return match;
}

static void
on_env_search_changed(GtkEditable *editable G_GNUC_UNUSED, gpointer user_data)
{
  EnvDialogData *data = user_data;
  gtk_list_box_invalidate_filter(GTK_LIST_BOX(data->listbox));
}

static void
on_env_row_activated(GtkListBox *box G_GNUC_UNUSED, GtkListBoxRow *row G_GNUC_UNUSED,
                     gpointer user_data)
{
  GtkDialog *dialog = GTK_DIALOG(user_data);
  gtk_dialog_response(dialog, GTK_RESPONSE_OK);
}

char *
sk_dialog_environment_select(GtkWindow *parent, const char **envs, int n_envs, const char *last_env)
{
  g_return_val_if_fail(envs != NULL && n_envs > 0, NULL);

  GtkWidget *dialog = gtk_dialog_new_with_buttons(
      _("Select Environment"), parent, GTK_DIALOG_MODAL | GTK_DIALOG_DESTROY_WITH_PARENT, _("Open"),
      GTK_RESPONSE_OK, _("Cancel"), GTK_RESPONSE_CANCEL, NULL);

  gtk_dialog_set_default_response(GTK_DIALOG(dialog), GTK_RESPONSE_OK);
  gtk_window_set_default_size(GTK_WINDOW(dialog), 400, 350);

  GtkWidget *content = gtk_dialog_get_content_area(GTK_DIALOG(dialog));
  gtk_box_set_spacing(GTK_BOX(content), 8);

  /* Search/filter field */
  GtkWidget *search_entry = gtk_search_entry_new();
  gtk_entry_set_placeholder_text(GTK_ENTRY(search_entry), _("Filter environments..."));
  /* Accessibility: label for environment filter */
  AtkObject *atk_search = gtk_widget_get_accessible(search_entry);
  if (atk_search != NULL)
  {
    atk_object_set_name(atk_search, _("Filter environments"));
  }
  gtk_widget_set_margin_start(search_entry, 12);
  gtk_widget_set_margin_end(search_entry, 12);
  gtk_widget_set_margin_top(search_entry, 12);
  gtk_container_add(GTK_CONTAINER(content), search_entry);

  /* Scrolled listbox */
  GtkWidget *scrolled = gtk_scrolled_window_new(NULL, NULL);
  gtk_scrolled_window_set_policy(GTK_SCROLLED_WINDOW(scrolled), GTK_POLICY_NEVER,
                                 GTK_POLICY_AUTOMATIC);
  gtk_widget_set_vexpand(scrolled, TRUE);
  gtk_widget_set_margin_start(scrolled, 12);
  gtk_widget_set_margin_end(scrolled, 12);
  gtk_widget_set_margin_bottom(scrolled, 12);
  gtk_container_add(GTK_CONTAINER(content), scrolled);

  GtkWidget *listbox = gtk_list_box_new();
  gtk_list_box_set_selection_mode(GTK_LIST_BOX(listbox), GTK_SELECTION_SINGLE);
  gtk_container_add(GTK_CONTAINER(scrolled), listbox);

  /* Populate */
  int select_idx = 0;
  for (int i = 0; i < n_envs; i++)
  {
    GtkWidget *label = gtk_label_new(envs[i]);
    gtk_widget_set_halign(label, GTK_ALIGN_START);
    gtk_widget_set_margin_start(label, 8);
    gtk_widget_set_margin_end(label, 8);
    gtk_widget_set_margin_top(label, 6);
    gtk_widget_set_margin_bottom(label, 6);
    gtk_list_box_insert(GTK_LIST_BOX(listbox), label, -1);

    if (last_env != NULL && g_strcmp0(envs[i], last_env) == 0)
    {
      select_idx = i;
    }
  }

  /* Pre-select last-used environment */
  GtkListBoxRow *row = gtk_list_box_get_row_at_index(GTK_LIST_BOX(listbox), select_idx);
  if (row != NULL)
  {
    gtk_list_box_select_row(GTK_LIST_BOX(listbox), row);
  }

  /* Filter setup */
  EnvDialogData filter_data = {
    .listbox = listbox,
    .search_entry = search_entry,
    .envs = envs,
    .n_envs = n_envs,
  };
  gtk_list_box_set_filter_func(GTK_LIST_BOX(listbox), env_filter_func, &filter_data, NULL);
  g_signal_connect(search_entry, "changed", G_CALLBACK(on_env_search_changed), &filter_data);

  /* Double-click to confirm */
  g_signal_connect(listbox, "row-activated", G_CALLBACK(on_env_row_activated), dialog);

  gtk_widget_show_all(dialog);

  int response = gtk_dialog_run(GTK_DIALOG(dialog));
  char *selected = NULL;

  if (response == GTK_RESPONSE_OK)
  {
    GtkListBoxRow *sel = gtk_list_box_get_selected_row(GTK_LIST_BOX(listbox));
    if (sel != NULL)
    {
      GtkWidget *child = gtk_bin_get_child(GTK_BIN(sel));
      if (GTK_IS_LABEL(child))
      {
        selected = g_strdup(gtk_label_get_text(GTK_LABEL(child)));
      }
    }
  }

  gtk_widget_destroy(dialog);

  SK_LOG_INFO(SK_LOG_COMPONENT_UI, "environment select: '%s'",
              selected != NULL ? selected : "(cancelled)");

  return selected;
}

/* ------------------------------------------------------------------ */
/* Close window dialog (FR-TABS-17)                                    */
/* ------------------------------------------------------------------ */

SkCloseResult
sk_dialog_close_window(GtkWindow *parent, int n_active)
{
  /* NFR-I18N-03: Use ngettext for plural handling */
  /* Translators: %d is the number of active sessions */
  char *message =
      g_strdup_printf(ngettext("This window has %d active session.\n\n"
                               "Sessions will continue running on the server even if you "
                               "hide or close this window.",
                               "This window has %d active sessions.\n\n"
                               "Sessions will continue running on the server even if you "
                               "hide or close this window.",
                               n_active),
                      n_active);

  GtkWidget *dialog = gtk_dialog_new_with_buttons(
      _("Close Window"), parent, GTK_DIALOG_MODAL | GTK_DIALOG_DESTROY_WITH_PARENT, _("Hide"),
      1, /* Default / Enter */
      _("Terminate sessions"), 2, _("Cancel"), GTK_RESPONSE_CANCEL, NULL);

  /* FR-TABS-17: "Hide" is default, activated with Enter */
  gtk_dialog_set_default_response(GTK_DIALOG(dialog), 1);

  GtkWidget *content = gtk_dialog_get_content_area(GTK_DIALOG(dialog));
  GtkWidget *label = gtk_label_new(message);
  gtk_label_set_line_wrap(GTK_LABEL(label), TRUE);
  gtk_widget_set_margin_start(label, 20);
  gtk_widget_set_margin_end(label, 20);
  gtk_widget_set_margin_top(label, 20);
  gtk_widget_set_margin_bottom(label, 20);
  gtk_container_add(GTK_CONTAINER(content), label);
  gtk_widget_show_all(dialog);

  int response = gtk_dialog_run(GTK_DIALOG(dialog));
  gtk_widget_destroy(dialog);
  g_free(message);

  SK_LOG_INFO(SK_LOG_COMPONENT_UI, "close window dialog (%d active): response=%d", n_active,
              response);

  switch (response)
  {
  case 1:
    return SK_CLOSE_RESULT_HIDE;
  case 2:
    return SK_CLOSE_RESULT_TERMINATE;
  default:
    return SK_CLOSE_RESULT_CANCEL;
  }
}

/* ------------------------------------------------------------------ */
/* General dialogs                                                     */
/* ------------------------------------------------------------------ */

void
sk_dialog_error(GtkWindow *parent, const char *title, const char *message)
{
  GtkWidget *dialog = gtk_message_dialog_new(
      parent, GTK_DIALOG_MODAL | GTK_DIALOG_DESTROY_WITH_PARENT, GTK_MESSAGE_ERROR, GTK_BUTTONS_OK,
      "%s", title != NULL ? title : _("Error"));

  if (message != NULL)
  {
    gtk_message_dialog_format_secondary_text(GTK_MESSAGE_DIALOG(dialog), "%s", message);
  }

  gtk_dialog_run(GTK_DIALOG(dialog));
  gtk_widget_destroy(dialog);
}

void
sk_dialog_info(GtkWindow *parent, const char *title, const char *message)
{
  GtkWidget *dialog = gtk_message_dialog_new(
      parent, GTK_DIALOG_MODAL | GTK_DIALOG_DESTROY_WITH_PARENT, GTK_MESSAGE_INFO, GTK_BUTTONS_OK,
      "%s", title != NULL ? title : _("Information"));

  if (message != NULL)
  {
    gtk_message_dialog_format_secondary_text(GTK_MESSAGE_DIALOG(dialog), "%s", message);
  }

  gtk_dialog_run(GTK_DIALOG(dialog));
  gtk_widget_destroy(dialog);
}

/* ------------------------------------------------------------------ */
/* Legacy compatibility wrappers                                       */
/* ------------------------------------------------------------------ */

bool
sk_ui_dialog_host_key(GtkWindow *parent, const char *host, const char *fingerprint, bool changed)
{
  if (changed)
  {
    sk_dialog_host_key_changed(parent, host, NULL, fingerprint, NULL);
    return false;
  }
  SkHostKeyDialogResult r = sk_dialog_host_key_unknown(parent, host, fingerprint, NULL);
  return (r == SK_HOST_KEY_ACCEPT_SAVE || r == SK_HOST_KEY_CONNECT_ONCE);
}

char *
sk_ui_dialog_password(GtkWindow *parent, const char *prompt)
{
  return sk_dialog_auth_password(parent, prompt);
}

void
sk_ui_dialog_error(GtkWindow *parent, const char *title, const char *message)
{
  sk_dialog_error(parent, title, message);
}

void
sk_ui_dialog_info(GtkWindow *parent, const char *title, const char *message)
{
  sk_dialog_info(parent, title, message);
}
