// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_toast.c
 * @brief Toast notification overlay for shellkeep.
 *
 * Displays brief notification messages at the bottom of the window
 * that auto-dismiss after a timeout.
 *
 * Requirements: FR-UI-08, FR-SESSION-11, FR-TABS-19
 */

#include "shellkeep/sk_i18n.h"
#include "shellkeep/sk_log.h"
#include "shellkeep/sk_ui.h"

#include <gtk/gtk.h>

/* ------------------------------------------------------------------ */
/* Default timeout                                                     */
/* ------------------------------------------------------------------ */

#define SK_TOAST_DEFAULT_TIMEOUT_MS 5000

/* ------------------------------------------------------------------ */
/* Toast auto-dismiss data                                             */
/* ------------------------------------------------------------------ */

typedef struct
{
  GtkWidget *toast_widget;
  guint timeout_id;
} ToastData;

static gboolean
toast_timeout_cb(gpointer user_data)
{
  ToastData *data = user_data;

  if (data->toast_widget != NULL && GTK_IS_WIDGET(data->toast_widget))
  {
    gtk_widget_destroy(data->toast_widget);
  }

  g_free(data);
  return G_SOURCE_REMOVE;
}

static void
on_toast_destroy(GtkWidget *widget G_GNUC_UNUSED, gpointer user_data)
{
  ToastData *data = user_data;
  data->toast_widget = NULL;
  if (data->timeout_id > 0)
  {
    /* Widget destroyed externally (not by timeout).
     * Remove the pending timeout and free the data ourselves. */
    g_source_remove(data->timeout_id);
    data->timeout_id = 0;
    g_free(data);
  }
  /* If timeout_id == 0, the timeout callback is currently running
   * and will free the data after gtk_widget_destroy returns. */
}

/* ------------------------------------------------------------------ */
/* Public API                                                          */
/* ------------------------------------------------------------------ */

void
sk_toast_show(GtkWindow *parent, const char *message, int timeout_ms)
{
  g_return_if_fail(parent != NULL);
  g_return_if_fail(message != NULL);

  if (timeout_ms <= 0)
  {
    timeout_ms = SK_TOAST_DEFAULT_TIMEOUT_MS;
  }

  /* Create toast widget */
  GtkWidget *toast_frame = gtk_frame_new(NULL);
  GtkStyleContext *ctx = gtk_widget_get_style_context(toast_frame);
  gtk_style_context_add_class(ctx, "toast-overlay");
  gtk_widget_set_halign(toast_frame, GTK_ALIGN_CENTER);
  gtk_widget_set_valign(toast_frame, GTK_ALIGN_END);
  gtk_widget_set_margin_bottom(toast_frame, 20);

  GtkWidget *label = gtk_label_new(message);
  gtk_label_set_line_wrap(GTK_LABEL(label), TRUE);
  gtk_container_add(GTK_CONTAINER(toast_frame), label);

  /* Try to find the overlay in the parent window */
  GtkWidget *child = gtk_bin_get_child(GTK_BIN(parent));
  if (child != NULL && GTK_IS_OVERLAY(child))
  {
    gtk_overlay_add_overlay(GTK_OVERLAY(child), toast_frame);
    gtk_widget_show_all(toast_frame);
  }
  else
  {
    /* Fallback: If no overlay, show as a simple temporary popup.
     * This may happen during tests or with non-standard window layouts. */
    SK_LOG_WARN(SK_LOG_COMPONENT_UI, "no overlay container for toast; message: %s", message);
    gtk_widget_destroy(toast_frame);
    return;
  }

  /* Set up auto-dismiss */
  ToastData *data = g_new0(ToastData, 1);
  data->toast_widget = toast_frame;

  g_signal_connect(toast_frame, "destroy", G_CALLBACK(on_toast_destroy), data);

  data->timeout_id = g_timeout_add((guint)timeout_ms, toast_timeout_cb, data);

  SK_LOG_DEBUG(SK_LOG_COMPONENT_UI, "toast shown: '%s' (timeout=%dms)", message, timeout_ms);
}

/* FR-SESSION-11: "Session kept on server" toast */
void
sk_toast_session_kept(GtkWindow *parent)
{
  sk_toast_show(parent,
                _("Session kept on server \xe2\x80\x94 "
                  "you can restore it later"),
                SK_TOAST_DEFAULT_TIMEOUT_MS);
}

/* FR-TABS-19: "continues in tray" notification */
void
sk_toast_continues_in_tray(GtkWindow *parent)
{
  /* FR-UI-06, FR-TABS-19: System notification when last window closes.
   * We use a GNotification for this since the window is being hidden
   * and a toast would not be visible. */
  GApplication *app = g_application_get_default();
  if (app != NULL)
  {
    GNotification *notif = g_notification_new("shellkeep");
    g_notification_set_body(notif, _("shellkeep continues running in the system tray. "
                                     "Your sessions remain active."));
    g_application_send_notification(app, "tray-continue", notif);
    g_object_unref(notif);

    SK_LOG_INFO(SK_LOG_COMPONENT_UI, "sent 'continues in tray' system notification");
  }
  else
  {
    /* Fallback: try as toast (window may still be visible briefly) */
    sk_toast_show(parent,
                  _("shellkeep continues running in the system tray. "
                    "Your sessions remain active."),
                  SK_TOAST_DEFAULT_TIMEOUT_MS);
  }
}
