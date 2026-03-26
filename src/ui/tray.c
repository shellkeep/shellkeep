// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file tray.c
 * @brief System tray icon using libayatana-appindicator3.
 *
 * Implements FR-TRAY-01..06: AppIndicator-based tray icon with dynamic
 * menu for environment management, window toggling, badges, and status
 * icon changes.
 *
 * The tray menu is rebuilt each time it is opened (FR-TRAY-05: badges
 * updated on menu open, not in real-time).
 *
 * Thread safety: all functions MUST be called from the GTK main thread.
 */

#include "shellkeep/sk_i18n.h"
#include "shellkeep/sk_log.h"
#include "shellkeep/sk_state.h"

/*
 * NOTE: sk_types.h is intentionally NOT included here because it
 * declares SkWindow and SkTab as opaque forward declarations
 * (struct _SkWindow / struct _SkTab) which conflict with the concrete
 * struct definitions in sk_state.h.  The tray module needs the full
 * state-layer structs (SkWindow.id, .title, .visible, etc.).
 *
 * We pull SK_APPLICATION_ID and SK_APPLICATION_NAME directly.
 */
#ifndef SK_APPLICATION_ID
#define SK_APPLICATION_ID "org.shellkeep.ShellKeep"
#endif
#ifndef SK_APPLICATION_NAME
#define SK_APPLICATION_NAME "shellkeep"
#endif

#include <gtk/gtk.h>

#include <glib.h>

#include <libayatana-appindicator/app-indicator.h>
#include <stdbool.h>
#include <string.h>

/* ------------------------------------------------------------------ */
/* Constants                                                           */
/* ------------------------------------------------------------------ */

/** Maximum windows shown directly in menu before grouping (FR-TRAY-03). */
#define SK_TRAY_MAX_INLINE_WINDOWS 10

/** Icon names for normal and attention states (FR-TRAY-04). */
#define SK_TRAY_ICON_NORMAL "shellkeep-symbolic"
#define SK_TRAY_ICON_ATTENTION "shellkeep-attention-symbolic"

/* ------------------------------------------------------------------ */
/* Tray context                                                        */
/* ------------------------------------------------------------------ */

/**
 * SkTray — manages the AppIndicator lifecycle, menu, and callbacks.
 *
 * The tray does not own the state or the GtkApplication; it holds weak
 * references and queries them when building the menu.
 */
typedef struct _SkTray
{
  AppIndicator *indicator; /**< AppIndicator handle (FR-TRAY-01). */
  GtkApplication *app;     /**< Weak ref to the GtkApplication. */

  /* Callback hooks — set by the UI layer to wire actions to real logic.
   * If NULL, the corresponding menu item is greyed out. */

  /** Called when the user clicks a window entry to toggle visibility. */
  void (*on_window_toggle)(const char *window_id, void *user_data);

  /** Called when the user clicks "Show all". */
  void (*on_show_all)(void *user_data);

  /** Called when the user selects "Switch to" an environment. */
  void (*on_env_switch)(const char *env_name, void *user_data);

  /** Called when the user clicks "Rename environment". */
  void (*on_env_rename)(const char *env_name, void *user_data);

  /** Called when the user clicks "New environment". */
  void (*on_env_new)(void *user_data);

  /** Called when the user clicks "Delete environment". */
  void (*on_env_delete)(const char *env_name, void *user_data);

  /** Called when the user clicks "Quit". */
  void (*on_quit)(void *user_data);

  void *callback_data; /**< Opaque data passed to all callbacks. */

  /* State snapshot used for menu building.  Owned by the caller;
   * the tray reads it only inside sk_tray_rebuild_menu(). */
  const SkStateFile *state;

  /* Badge tracking: set of window IDs that have new output while hidden.
   * Keys are g_strdup'd window IDs; values are GINT_TO_POINTER(1).
   * FR-TRAY-05 */
  GHashTable *badged_windows;

  /* Whether there are active sessions (used for attention icon). */
  bool has_active_sessions;

} SkTray;

/* ------------------------------------------------------------------ */
/* Forward declarations                                                */
/* ------------------------------------------------------------------ */

static void tray_build_menu(SkTray *tray);
static void tray_update_status(SkTray *tray);
static gboolean tray_all_windows_hidden(const SkStateFile *state);

/* Menu item callbacks */
static void on_menu_window_toggle(GtkMenuItem *item, gpointer data);
static void on_menu_show_all(GtkMenuItem *item, gpointer data);
static void on_menu_env_switch(GtkMenuItem *item, gpointer data);
static void on_menu_env_rename(GtkMenuItem *item, gpointer data);
static void on_menu_env_new(GtkMenuItem *item, gpointer data);
static void on_menu_env_delete(GtkMenuItem *item, gpointer data);
static void on_menu_quit(GtkMenuItem *item, gpointer data);

/* ------------------------------------------------------------------ */
/* Lifecycle                                                           */
/* ------------------------------------------------------------------ */

/**
 * Create and initialise the system tray icon.
 *
 * FR-TRAY-01: Uses libayatana-appindicator3 (StatusNotifierItem via
 * D-Bus).  Works on X11 and Wayland.
 *
 * FR-TRAY-06: Limitation — GNOME vanilla requires AppIndicator
 * extension (pre-installed on Ubuntu).  This is documented but not
 * enforced here.
 *
 * @param app  The GtkApplication (not owned).
 * @return New tray context, or NULL on failure.
 */
SkTray *
sk_tray_new(GtkApplication *app)
{
  g_return_val_if_fail(app != NULL, NULL);

  SkTray *tray = g_new0(SkTray, 1);
  tray->app = app;

  /* FR-TRAY-01: create the AppIndicator */
  tray->indicator = app_indicator_new(SK_APPLICATION_ID, SK_TRAY_ICON_NORMAL,
                                      APP_INDICATOR_CATEGORY_APPLICATION_STATUS);

  if (tray->indicator == NULL)
  {
    SK_LOG_ERROR(SK_LOG_COMPONENT_UI, "failed to create AppIndicator for tray icon");
    g_free(tray);
    return NULL;
  }

  app_indicator_set_status(tray->indicator, APP_INDICATOR_STATUS_ACTIVE);
  app_indicator_set_title(tray->indicator, SK_APPLICATION_NAME);

  /* FR-TRAY-04: attention icon for when sessions are active but hidden */
  app_indicator_set_attention_icon(tray->indicator, SK_TRAY_ICON_ATTENTION);

  /* Badge tracking hash table (FR-TRAY-05) */
  tray->badged_windows = g_hash_table_new_full(g_str_hash, g_str_equal, g_free, NULL);

  tray->has_active_sessions = false;
  tray->state = NULL;

  /* Build an initial empty menu so the indicator is usable */
  tray_build_menu(tray);

  SK_LOG_INFO(SK_LOG_COMPONENT_UI, "tray icon initialised");
  return tray;
}

/**
 * Destroy the tray icon and free all resources.
 */
void
sk_tray_free(SkTray *tray)
{
  if (tray == NULL)
    return;

  SK_LOG_DEBUG(SK_LOG_COMPONENT_UI, "destroying tray icon");

  if (tray->indicator != NULL)
    g_object_unref(tray->indicator);

  if (tray->badged_windows != NULL)
    g_hash_table_destroy(tray->badged_windows);

  g_free(tray);
}

/* ------------------------------------------------------------------ */
/* Public setters                                                      */
/* ------------------------------------------------------------------ */

/**
 * Set the callback hooks for tray menu actions.
 *
 * All callbacks are optional (NULL means the menu item is insensitive).
 */
void
sk_tray_set_callbacks(SkTray *tray, void (*on_window_toggle)(const char *, void *),
                      void (*on_show_all)(void *), void (*on_env_switch)(const char *, void *),
                      void (*on_env_rename)(const char *, void *), void (*on_env_new)(void *),
                      void (*on_env_delete)(const char *, void *), void (*on_quit)(void *),
                      void *user_data)
{
  g_return_if_fail(tray != NULL);

  tray->on_window_toggle = on_window_toggle;
  tray->on_show_all = on_show_all;
  tray->on_env_switch = on_env_switch;
  tray->on_env_rename = on_env_rename;
  tray->on_env_new = on_env_new;
  tray->on_env_delete = on_env_delete;
  tray->on_quit = on_quit;
  tray->callback_data = user_data;
}

/**
 * Update the tray with new application state and rebuild the menu.
 *
 * This is the primary entry point for keeping the tray in sync with
 * the rest of the application.  The state pointer is not owned — it
 * must remain valid until the next call to sk_tray_update_state() or
 * sk_tray_free().
 *
 * @param tray               Tray context.
 * @param state              Current state snapshot (not owned).
 * @param has_active_sessions Whether any tmux sessions are alive.
 */
void
sk_tray_update_state(SkTray *tray, const SkStateFile *state, bool has_active_sessions)
{
  g_return_if_fail(tray != NULL);

  tray->state = state;
  tray->has_active_sessions = has_active_sessions;

  tray_build_menu(tray);
  tray_update_status(tray);
}

/**
 * Mark a window as having new output while hidden (FR-TRAY-05).
 *
 * The badge is shown the next time the menu is built (i.e., opened).
 *
 * @param tray       Tray context.
 * @param window_id  The window UUID.
 */
void
sk_tray_badge_window(SkTray *tray, const char *window_id)
{
  g_return_if_fail(tray != NULL);
  g_return_if_fail(window_id != NULL);

  /* FR-TRAY-05: badges updated on menu open, not real-time */
  g_hash_table_replace(tray->badged_windows, g_strdup(window_id), GINT_TO_POINTER(1));

  SK_LOG_TRACE(SK_LOG_COMPONENT_UI, "tray: badged window %s", window_id);
}

/**
 * Clear the badge for a window (e.g., when it becomes visible).
 */
void
sk_tray_clear_badge(SkTray *tray, const char *window_id)
{
  g_return_if_fail(tray != NULL);
  g_return_if_fail(window_id != NULL);

  g_hash_table_remove(tray->badged_windows, window_id);
}

/**
 * Clear all badges (e.g., on "Show all").
 */
void
sk_tray_clear_all_badges(SkTray *tray)
{
  g_return_if_fail(tray != NULL);

  g_hash_table_remove_all(tray->badged_windows);
}

/* ------------------------------------------------------------------ */
/* Menu construction (FR-TRAY-02)                                      */
/* ------------------------------------------------------------------ */

/**
 * Helper: find the active environment in the state file.
 *
 * @return Pointer to the active SkEnvironment, or NULL.
 */
static const SkEnvironment *
tray_find_active_env(const SkStateFile *state)
{
  if (state == NULL || state->last_environment == NULL)
    return NULL;

  for (int i = 0; i < state->n_environments; i++)
  {
    if (state->environments[i] != NULL &&
        g_strcmp0(state->environments[i]->name, state->last_environment) == 0)
    {
      return state->environments[i];
    }
  }

  return NULL;
}

/**
 * Context passed through g_object_set_data to menu item callbacks.
 */
typedef struct
{
  SkTray *tray;
  char *id; /**< Window ID or environment name (owned, freed on destroy). */
} SkTrayItemData;

/**
 * GDestroyNotify for SkTrayItemData attached to menu items.
 */
static void
tray_item_data_free(gpointer data)
{
  SkTrayItemData *d = data;
  if (d == NULL)
    return;
  g_free(d->id);
  g_free(d);
}

/**
 * Allocate and attach callback context to a menu item.
 */
static SkTrayItemData *
tray_item_data_new(SkTray *tray, const char *id, GtkWidget *item)
{
  SkTrayItemData *d = g_new0(SkTrayItemData, 1);
  d->tray = tray;
  d->id = g_strdup(id);
  g_object_set_data_full(G_OBJECT(item), "sk-tray-data", d, tray_item_data_free);
  return d;
}

/**
 * Build (or rebuild) the tray menu from current state.
 *
 * FR-TRAY-02: menu structure
 * FR-TRAY-03: group windows > 10
 * FR-TRAY-05: badges updated here (on menu build)
 *
 * The menu is rebuilt each time because AppIndicator caches a static
 * menu — we replace the whole menu widget.
 */
static void
tray_build_menu(SkTray *tray)
{
  GtkWidget *menu = gtk_menu_new();

  const SkStateFile *state = tray->state;
  const SkEnvironment *active_env = tray_find_active_env(state);

  /* ---- Active environment header (FR-TRAY-02) ---- */
  if (active_env != NULL)
  {
    /* Translators: %s is the environment name */
    char *label_text = g_strdup_printf(_("Environment: %s"), active_env->name);
    GtkWidget *header = gtk_menu_item_new_with_label(label_text);
    g_free(label_text);
    gtk_widget_set_sensitive(header, FALSE);
    gtk_menu_shell_append(GTK_MENU_SHELL(menu), header);
  }
  else
  {
    GtkWidget *header = gtk_menu_item_new_with_label(_("No environment"));
    gtk_widget_set_sensitive(header, FALSE);
    gtk_menu_shell_append(GTK_MENU_SHELL(menu), header);
  }

  /* ---- "Switch to" submenu (FR-TRAY-02, FR-ENV-10) ---- */
  if (state != NULL && state->n_environments > 1)
  {
    GtkWidget *switch_item = gtk_menu_item_new_with_label(_("Switch to"));
    GtkWidget *switch_menu = gtk_menu_new();
    gtk_menu_item_set_submenu(GTK_MENU_ITEM(switch_item), switch_menu);

    for (int i = 0; i < state->n_environments; i++)
    {
      const SkEnvironment *env = state->environments[i];
      if (env == NULL)
        continue;

      /* Skip the active environment */
      if (active_env != NULL && g_strcmp0(env->name, active_env->name) == 0)
      {
        continue;
      }

      GtkWidget *env_item = gtk_menu_item_new_with_label(env->name);
      tray_item_data_new(tray, env->name, env_item);
      g_signal_connect(env_item, "activate", G_CALLBACK(on_menu_env_switch), NULL);
      gtk_widget_set_sensitive(env_item, tray->on_env_switch != NULL);
      gtk_menu_shell_append(GTK_MENU_SHELL(switch_menu), env_item);
    }

    gtk_menu_shell_append(GTK_MENU_SHELL(menu), switch_item);
  }

  /* ---- Environment actions (FR-TRAY-02, FR-ENV-08, FR-ENV-09) ---- */
  {
    GtkWidget *rename_item = gtk_menu_item_new_with_label(_("Rename environment"));
    if (active_env != NULL)
      tray_item_data_new(tray, active_env->name, rename_item);
    g_signal_connect(rename_item, "activate", G_CALLBACK(on_menu_env_rename), NULL);
    gtk_widget_set_sensitive(rename_item, active_env != NULL && tray->on_env_rename != NULL);
    gtk_menu_shell_append(GTK_MENU_SHELL(menu), rename_item);

    GtkWidget *new_item = gtk_menu_item_new_with_label(_("New environment"));
    tray_item_data_new(tray, "", new_item);
    g_signal_connect(new_item, "activate", G_CALLBACK(on_menu_env_new), NULL);
    gtk_widget_set_sensitive(new_item, tray->on_env_new != NULL);
    gtk_menu_shell_append(GTK_MENU_SHELL(menu), new_item);

    GtkWidget *delete_item = gtk_menu_item_new_with_label(_("Delete environment"));
    if (active_env != NULL)
      tray_item_data_new(tray, active_env->name, delete_item);
    g_signal_connect(delete_item, "activate", G_CALLBACK(on_menu_env_delete), NULL);
    gtk_widget_set_sensitive(delete_item, active_env != NULL && tray->on_env_delete != NULL);
    gtk_menu_shell_append(GTK_MENU_SHELL(menu), delete_item);
  }

  /* ---- Separator ---- */
  gtk_menu_shell_append(GTK_MENU_SHELL(menu), gtk_separator_menu_item_new());

  /* ---- Windows list (FR-TRAY-02, FR-TRAY-03, FR-TRAY-05) ---- */
  if (active_env != NULL && active_env->n_windows > 0)
  {
    int n_windows = active_env->n_windows;
    int inline_count =
        (n_windows <= SK_TRAY_MAX_INLINE_WINDOWS) ? n_windows : SK_TRAY_MAX_INLINE_WINDOWS;
    GtkWidget *overflow_menu = NULL;

    /* FR-TRAY-03: if > 10, show first 10 inline, rest in submenu */
    if (n_windows > SK_TRAY_MAX_INLINE_WINDOWS)
    {
      GtkWidget *more_item = gtk_menu_item_new_with_label(_("More windows..."));
      overflow_menu = gtk_menu_new();
      gtk_menu_item_set_submenu(GTK_MENU_ITEM(more_item), overflow_menu);
      /* We'll append this item after the inline windows */
      g_object_set_data(G_OBJECT(menu), "sk-overflow-item", more_item);
    }

    for (int i = 0; i < n_windows; i++)
    {
      const SkWindow *win = active_env->windows[i];
      if (win == NULL)
        continue;

      /* Build label: title + visibility + badge */
      const char *title =
          (win->title != NULL && win->title[0] != '\0') ? win->title : _("Untitled");

      bool has_badge = g_hash_table_contains(tray->badged_windows, win->id ? win->id : "");

      char *label;
      if (has_badge && !win->visible)
      {
        /* FR-TRAY-05: badge for hidden windows with new output */
        /* Translators: %s is the window title */
        label = g_strdup_printf(_("[*] %s (hidden)"), title);
      }
      else if (!win->visible)
      {
        /* Translators: %s is the window title */
        label = g_strdup_printf(_("    %s (hidden)"), title);
      }
      else if (has_badge)
      {
        /* Translators: %s is the window title */
        label = g_strdup_printf(_("[*] %s"), title);
      }
      else
      {
        /* Translators: %s is the window title */
        label = g_strdup_printf(_("    %s"), title);
      }

      GtkWidget *win_item = gtk_menu_item_new_with_label(label);
      g_free(label);

      tray_item_data_new(tray, win->id, win_item);
      g_signal_connect(win_item, "activate", G_CALLBACK(on_menu_window_toggle), NULL);
      gtk_widget_set_sensitive(win_item, tray->on_window_toggle != NULL);

      /* Decide where to append: inline or overflow */
      if (i < inline_count)
      {
        gtk_menu_shell_append(GTK_MENU_SHELL(menu), win_item);
      }
      else if (overflow_menu != NULL)
      {
        gtk_menu_shell_append(GTK_MENU_SHELL(overflow_menu), win_item);
      }
    }

    /* Append overflow submenu item if present */
    GtkWidget *overflow_item = g_object_get_data(G_OBJECT(menu), "sk-overflow-item");
    if (overflow_item != NULL)
    {
      gtk_menu_shell_append(GTK_MENU_SHELL(menu), overflow_item);
    }

    /* ---- "Show all" (FR-TRAY-02) ---- */
    GtkWidget *show_all_item = gtk_menu_item_new_with_label(_("Show all"));
    tray_item_data_new(tray, "", show_all_item);
    g_signal_connect(show_all_item, "activate", G_CALLBACK(on_menu_show_all), NULL);
    gtk_widget_set_sensitive(show_all_item, tray->on_show_all != NULL);
    gtk_menu_shell_append(GTK_MENU_SHELL(menu), show_all_item);
  }
  else
  {
    GtkWidget *no_windows = gtk_menu_item_new_with_label(_("No windows"));
    gtk_widget_set_sensitive(no_windows, FALSE);
    gtk_menu_shell_append(GTK_MENU_SHELL(menu), no_windows);
  }

  /* ---- Separator ---- */
  gtk_menu_shell_append(GTK_MENU_SHELL(menu), gtk_separator_menu_item_new());

  /* ---- Quit (FR-TRAY-02) ---- */
  {
    const char *quit_label = tray->has_active_sessions ? _("Quit (sessions active)") : _("Quit");
    GtkWidget *quit_item = gtk_menu_item_new_with_label(quit_label);
    tray_item_data_new(tray, "", quit_item);
    g_signal_connect(quit_item, "activate", G_CALLBACK(on_menu_quit), NULL);
    gtk_widget_set_sensitive(quit_item, tray->on_quit != NULL);
    gtk_menu_shell_append(GTK_MENU_SHELL(menu), quit_item);
  }

  gtk_widget_show_all(menu);

  /* Replace the indicator's menu */
  app_indicator_set_menu(tray->indicator, GTK_MENU(menu));

  SK_LOG_TRACE(SK_LOG_COMPONENT_UI, "tray menu rebuilt");
}

/* ------------------------------------------------------------------ */
/* Status icon management (FR-TRAY-04)                                 */
/* ------------------------------------------------------------------ */

/**
 * Update the tray icon status based on session state and window
 * visibility.
 *
 * FR-TRAY-04: icon changes appearance when sessions are active but
 * all windows are hidden.
 */
static void
tray_update_status(SkTray *tray)
{
  if (tray->has_active_sessions && tray_all_windows_hidden(tray->state))
  {
    /* Sessions running but no visible windows — draw attention */
    app_indicator_set_status(tray->indicator, APP_INDICATOR_STATUS_ATTENTION);
    SK_LOG_TRACE(SK_LOG_COMPONENT_UI, "tray status: ATTENTION (sessions active, "
                                      "all windows hidden)");
  }
  else
  {
    app_indicator_set_status(tray->indicator, APP_INDICATOR_STATUS_ACTIVE);
  }
}

/**
 * Check whether all windows in the active environment are hidden.
 */
static gboolean
tray_all_windows_hidden(const SkStateFile *state)
{
  if (state == NULL)
    return FALSE;

  const SkEnvironment *env = tray_find_active_env(state);
  if (env == NULL || env->n_windows == 0)
    return FALSE;

  for (int i = 0; i < env->n_windows; i++)
  {
    if (env->windows[i] != NULL && env->windows[i]->visible)
      return FALSE;
  }

  return TRUE;
}

/* ------------------------------------------------------------------ */
/* Menu item callbacks                                                 */
/* ------------------------------------------------------------------ */

/**
 * Retrieve the SkTrayItemData from a menu item's attached data.
 */
static SkTrayItemData *
tray_get_item_data(GtkMenuItem *item)
{
  return g_object_get_data(G_OBJECT(item), "sk-tray-data");
}

/** Toggle window visibility (FR-TRAY-02). */
static void
on_menu_window_toggle(GtkMenuItem *item, gpointer data G_GNUC_UNUSED)
{
  SkTrayItemData *d = tray_get_item_data(item);
  if (d == NULL || d->tray == NULL)
    return;

  SkTray *tray = d->tray;

  /* Clear badge when user interacts with the window (FR-TRAY-05) */
  if (d->id != NULL)
    sk_tray_clear_badge(tray, d->id);

  if (tray->on_window_toggle != NULL)
    tray->on_window_toggle(d->id, tray->callback_data);
}

/** Show all windows (FR-TRAY-02). */
static void
on_menu_show_all(GtkMenuItem *item, gpointer data G_GNUC_UNUSED)
{
  SkTrayItemData *d = tray_get_item_data(item);
  if (d == NULL || d->tray == NULL)
    return;

  SkTray *tray = d->tray;

  /* Clear all badges when showing all */
  sk_tray_clear_all_badges(tray);

  if (tray->on_show_all != NULL)
    tray->on_show_all(tray->callback_data);
}

/** Switch environment (FR-ENV-10). */
static void
on_menu_env_switch(GtkMenuItem *item, gpointer data G_GNUC_UNUSED)
{
  SkTrayItemData *d = tray_get_item_data(item);
  if (d == NULL || d->tray == NULL)
    return;

  SkTray *tray = d->tray;
  if (tray->on_env_switch != NULL)
    tray->on_env_switch(d->id, tray->callback_data);
}

/** Rename environment (FR-ENV-09). */
static void
on_menu_env_rename(GtkMenuItem *item, gpointer data G_GNUC_UNUSED)
{
  SkTrayItemData *d = tray_get_item_data(item);
  if (d == NULL || d->tray == NULL)
    return;

  SkTray *tray = d->tray;
  if (tray->on_env_rename != NULL)
    tray->on_env_rename(d->id, tray->callback_data);
}

/** New environment (FR-ENV-08). */
static void
on_menu_env_new(GtkMenuItem *item, gpointer data G_GNUC_UNUSED)
{
  SkTrayItemData *d = tray_get_item_data(item);
  if (d == NULL || d->tray == NULL)
    return;

  SkTray *tray = d->tray;
  if (tray->on_env_new != NULL)
    tray->on_env_new(tray->callback_data);
}

/** Delete environment (FR-ENV-07). */
static void
on_menu_env_delete(GtkMenuItem *item, gpointer data G_GNUC_UNUSED)
{
  SkTrayItemData *d = tray_get_item_data(item);
  if (d == NULL || d->tray == NULL)
    return;

  SkTray *tray = d->tray;
  if (tray->on_env_delete != NULL)
    tray->on_env_delete(d->id, tray->callback_data);
}

/**
 * Quit the application (FR-TRAY-02).
 *
 * The actual confirmation dialog (if sessions are active) is handled
 * by the callback — the tray just invokes the hook.
 */
static void
on_menu_quit(GtkMenuItem *item, gpointer data G_GNUC_UNUSED)
{
  SkTrayItemData *d = tray_get_item_data(item);
  if (d == NULL || d->tray == NULL)
    return;

  SkTray *tray = d->tray;
  if (tray->on_quit != NULL)
    tray->on_quit(tray->callback_data);
}
