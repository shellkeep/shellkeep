// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_conn_feedback.c
 * @brief Connection feedback overlay -- per-phase progress indicator.
 *
 * Shows a visual overlay during connection with phase messages:
 * "Connecting...", "Authenticating...", "Checking tmux...",
 * "Loading state...", "Restoring sessions (N/M)..."
 *
 * Requirement: FR-CONN-16
 */

#include "shellkeep/sk_i18n.h"
#include "shellkeep/sk_log.h"
#include "shellkeep/sk_ui.h"

#include <gtk/gtk.h>

#include <string.h>

/* ------------------------------------------------------------------ */
/* Internal structure                                                  */
/* ------------------------------------------------------------------ */

struct _SkConnFeedback
{
  GtkWindow *parent;
  GtkWidget *overlay_widget; /**< Overlay container added to parent. */
  GtkWidget *box;            /**< VBox with spinner + label. */
  GtkWidget *spinner;        /**< GtkSpinner. */
  GtkWidget *phase_label;    /**< Phase description text. */
  GtkWidget *progress_label; /**< "Restoring sessions N/M" text. */
  GtkWidget *error_label;    /**< Error message (hidden by default). */
  SkConnPhase current_phase;
};

/* ------------------------------------------------------------------ */
/* Phase text (FR-CONN-16)                                             */
/* ------------------------------------------------------------------ */

static const char *
phase_to_string(SkConnPhase phase)
{
  switch (phase)
  {
  case SK_CONN_PHASE_IDLE:
    return "";
  case SK_CONN_PHASE_CONNECTING:
    return _("Connecting...");
  case SK_CONN_PHASE_AUTHENTICATING:
    return _("Authenticating...");
  case SK_CONN_PHASE_CHECKING_TMUX:
    return _("Checking tmux...");
  case SK_CONN_PHASE_LOADING_STATE:
    return _("Loading state...");
  case SK_CONN_PHASE_RESTORING:
    return _("Restoring sessions...");
  case SK_CONN_PHASE_DONE:
    return _("Connected");
  case SK_CONN_PHASE_ERROR:
    return _("Connection failed");
  }
  return "";
}

/* ------------------------------------------------------------------ */
/* Lifecycle                                                           */
/* ------------------------------------------------------------------ */

SkConnFeedback *
sk_conn_feedback_new(GtkWindow *parent)
{
  g_return_val_if_fail(parent != NULL, NULL);

  SkConnFeedback *fb = g_new0(SkConnFeedback, 1);
  fb->parent = parent;
  fb->current_phase = SK_CONN_PHASE_IDLE;

  /* Create an overlay widget to display on top of the window content */
  fb->overlay_widget = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
  GtkStyleContext *ctx = gtk_widget_get_style_context(fb->overlay_widget);
  gtk_style_context_add_class(ctx, "conn-feedback");
  gtk_widget_set_halign(fb->overlay_widget, GTK_ALIGN_CENTER);
  gtk_widget_set_valign(fb->overlay_widget, GTK_ALIGN_CENTER);

  fb->box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 12);
  gtk_widget_set_margin_start(fb->box, 40);
  gtk_widget_set_margin_end(fb->box, 40);
  gtk_widget_set_margin_top(fb->box, 30);
  gtk_widget_set_margin_bottom(fb->box, 30);
  gtk_container_add(GTK_CONTAINER(fb->overlay_widget), fb->box);

  /* Spinner */
  fb->spinner = gtk_spinner_new();
  gtk_spinner_start(GTK_SPINNER(fb->spinner));
  gtk_widget_set_halign(fb->spinner, GTK_ALIGN_CENTER);
  gtk_container_add(GTK_CONTAINER(fb->box), fb->spinner);

  /* Phase label */
  fb->phase_label = gtk_label_new(_("Connecting..."));
  gtk_label_set_justify(GTK_LABEL(fb->phase_label), GTK_JUSTIFY_CENTER);
  gtk_widget_set_halign(fb->phase_label, GTK_ALIGN_CENTER);
  gtk_container_add(GTK_CONTAINER(fb->box), fb->phase_label);

  /* Progress label (hidden by default) */
  fb->progress_label = gtk_label_new("");
  gtk_widget_set_halign(fb->progress_label, GTK_ALIGN_CENTER);
  gtk_widget_set_no_show_all(fb->progress_label, TRUE);
  gtk_container_add(GTK_CONTAINER(fb->box), fb->progress_label);

  /* Error label (hidden by default) */
  fb->error_label = gtk_label_new("");
  gtk_label_set_line_wrap(GTK_LABEL(fb->error_label), TRUE);
  gtk_widget_set_halign(fb->error_label, GTK_ALIGN_CENTER);
  gtk_widget_set_no_show_all(fb->error_label, TRUE);
  gtk_container_add(GTK_CONTAINER(fb->box), fb->error_label);

  /* Try to add to the window's overlay container if available,
   * otherwise add directly to the window */
  GtkWidget *child = gtk_bin_get_child(GTK_BIN(parent));
  if (child != NULL && GTK_IS_OVERLAY(child))
  {
    gtk_overlay_add_overlay(GTK_OVERLAY(child), fb->overlay_widget);
  }
  else
  {
    /* Fallback: show as a separate popup window */
    SK_LOG_WARN(SK_LOG_COMPONENT_UI, "no overlay container found; feedback may not display");
  }

  gtk_widget_show_all(fb->overlay_widget);

  SK_LOG_DEBUG(SK_LOG_COMPONENT_UI, "connection feedback overlay created");

  return fb;
}

void
sk_conn_feedback_set_phase(SkConnFeedback *fb, SkConnPhase phase)
{
  g_return_if_fail(fb != NULL);

  fb->current_phase = phase;
  gtk_label_set_text(GTK_LABEL(fb->phase_label), phase_to_string(phase));

  /* Show/hide spinner based on phase */
  if (phase == SK_CONN_PHASE_DONE || phase == SK_CONN_PHASE_ERROR)
  {
    gtk_spinner_stop(GTK_SPINNER(fb->spinner));
    gtk_widget_hide(fb->spinner);
  }
  else
  {
    gtk_spinner_start(GTK_SPINNER(fb->spinner));
    gtk_widget_show(fb->spinner);
  }

  /* Hide progress unless restoring */
  if (phase != SK_CONN_PHASE_RESTORING)
  {
    gtk_widget_hide(fb->progress_label);
  }

  /* Show error label only on error */
  if (phase != SK_CONN_PHASE_ERROR)
  {
    gtk_widget_hide(fb->error_label);
  }

  SK_LOG_DEBUG(SK_LOG_COMPONENT_UI, "connection phase: %s", phase_to_string(phase));
}

void
sk_conn_feedback_set_progress(SkConnFeedback *fb, int current, int total)
{
  g_return_if_fail(fb != NULL);

  /* Translators: %1$d is the current session number, %2$d is the total */
  char *text = g_strdup_printf(_("Restoring sessions (%1$d/%2$d)..."), current, total);
  gtk_label_set_text(GTK_LABEL(fb->progress_label), text);
  gtk_widget_show(fb->progress_label);
  g_free(text);
}

void
sk_conn_feedback_set_error(SkConnFeedback *fb, const char *message)
{
  g_return_if_fail(fb != NULL);

  fb->current_phase = SK_CONN_PHASE_ERROR;
  gtk_label_set_text(GTK_LABEL(fb->phase_label), _("Connection failed"));
  gtk_spinner_stop(GTK_SPINNER(fb->spinner));
  gtk_widget_hide(fb->spinner);

  if (message != NULL)
  {
    gtk_label_set_text(GTK_LABEL(fb->error_label), message);
    gtk_widget_show(fb->error_label);
  }

  SK_LOG_WARN(SK_LOG_COMPONENT_UI, "connection feedback error: %s",
              message != NULL ? message : "(no message)");
}

void
sk_conn_feedback_free(SkConnFeedback *fb)
{
  if (fb == NULL)
    return;

  if (fb->overlay_widget != NULL && GTK_IS_WIDGET(fb->overlay_widget))
  {
    gtk_widget_destroy(fb->overlay_widget);
  }

  g_free(fb);

  SK_LOG_DEBUG(SK_LOG_COMPONENT_UI, "connection feedback overlay freed");
}
