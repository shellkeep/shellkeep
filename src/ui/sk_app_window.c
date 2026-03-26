// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_app_window.c
 * @brief SkAppWindow -- GTK window with tabbed terminal notebook.
 *
 * Implements the main application window with a GtkNotebook tab bar,
 * editable tab labels, connection indicators, drag-and-drop between
 * windows, close-window dialog, and geometry save/restore.
 *
 * Requirements: FR-TABS-01..19, FR-UI-04, FR-UI-06
 */

#include "shellkeep/sk_i18n.h"
#include "shellkeep/sk_log.h"
#include "shellkeep/sk_terminal.h"
#include "shellkeep/sk_types.h"
#include "shellkeep/sk_ui.h"

#include <atk/atk.h>
#include <gtk/gtk.h>

#include <string.h>

/* ------------------------------------------------------------------ */
/* Internal structures                                                 */
/* ------------------------------------------------------------------ */

/** Tab label widget with indicator, title, and close button. */
typedef struct _SkTabLabel
{
  GtkWidget *box;          /**< HBox container. */
  GtkWidget *indicator;    /**< Connection indicator (colored dot). */
  GtkWidget *label;        /**< GtkLabel for the title. */
  GtkWidget *entry;        /**< GtkEntry for inline rename (hidden). */
  GtkWidget *warning_icon; /**< Warning icon for dead tabs. */
  GtkWidget *close_button; /**< Close button (x). */
} SkTabLabel;

struct _SkAppTab
{
  SkAppWindow *window;       /**< Parent window. */
  SkTerminalTab *terminal;   /**< Terminal tab widget. */
  SkTabLabel label;          /**< Tab label widgets. */
  char *title;               /**< Current title string. */
  SkConnIndicator indicator; /**< Connection state. */
  bool is_dead;              /**< Whether this tab is dead. */
  int page_num;              /**< Page number in notebook. */
};

struct _SkAppWindow
{
  GtkApplication *app; /**< Application reference. */
  GtkWidget *window;   /**< GtkApplicationWindow. */
  GtkWidget *notebook; /**< GtkNotebook for tabs. */
  GtkWidget *overlay;  /**< GtkOverlay for toasts/feedback. */
  GPtrArray *tabs;     /**< Array of SkAppTab*. */

  /* Callbacks */
  SkTabRenamedCb renamed_cb;
  gpointer renamed_cb_data;
  SkTabClosedCb closed_cb;
  gpointer closed_cb_data;

  /* DnD state */
  bool dnd_enabled;
};

/* ------------------------------------------------------------------ */
/* Tab label helpers                                                   */
/* ------------------------------------------------------------------ */

static const char *
indicator_css_class(SkConnIndicator ind)
{
  switch (ind)
  {
  case SK_CONN_INDICATOR_GREEN:
    return "indicator-green";
  case SK_CONN_INDICATOR_YELLOW:
    return "indicator-yellow";
  case SK_CONN_INDICATOR_RED:
    return "indicator-red";
  default:
    return "indicator-green";
  }
}

static void
update_indicator_css(GtkWidget *widget, SkConnIndicator ind)
{
  GtkStyleContext *ctx = gtk_widget_get_style_context(widget);
  gtk_style_context_remove_class(ctx, "indicator-green");
  gtk_style_context_remove_class(ctx, "indicator-yellow");
  gtk_style_context_remove_class(ctx, "indicator-red");
  gtk_style_context_add_class(ctx, indicator_css_class(ind));
}

/* ------------------------------------------------------------------ */
/* Tab label event handlers                                            */
/* ------------------------------------------------------------------ */

static void
on_tab_close_clicked(GtkButton *button G_GNUC_UNUSED, gpointer user_data)
{
  SkAppTab *tab = user_data;
  if (tab->window->closed_cb != NULL)
  {
    tab->window->closed_cb(tab, tab->window->closed_cb_data);
  }
}

static void
on_tab_rename_activate(GtkEntry *entry, gpointer user_data)
{
  SkAppTab *tab = user_data;
  const char *new_title = gtk_entry_get_text(entry);

  if (new_title != NULL && new_title[0] != '\0')
  {
    g_free(tab->title);
    tab->title = g_strdup(new_title);
    gtk_label_set_text(GTK_LABEL(tab->label.label), new_title);

    if (tab->window->renamed_cb != NULL)
    {
      tab->window->renamed_cb(tab, new_title, tab->window->renamed_cb_data);
    }
  }

  /* Switch back from entry to label */
  gtk_widget_hide(GTK_WIDGET(entry));
  gtk_widget_show(tab->label.label);
}

static gboolean
on_tab_rename_focus_out(GtkWidget *widget, GdkEvent *event G_GNUC_UNUSED, gpointer user_data)
{
  on_tab_rename_activate(GTK_ENTRY(widget), user_data);
  return FALSE;
}

static gboolean
on_tab_rename_key_press(GtkWidget *widget, GdkEventKey *event, gpointer user_data)
{
  if (event->keyval == GDK_KEY_Escape)
  {
    SkAppTab *tab = user_data;
    /* Cancel: revert and hide entry */
    gtk_widget_hide(widget);
    gtk_widget_show(tab->label.label);
    return TRUE;
  }
  return FALSE;
}

static void
on_tab_close_from_menu(SkAppTab *tab)
{
  if (tab->window->closed_cb != NULL)
  {
    tab->window->closed_cb(tab, tab->window->closed_cb_data);
  }
}

static gboolean
on_tab_label_button_press(GtkWidget *widget G_GNUC_UNUSED, GdkEventButton *event,
                          gpointer user_data)
{
  SkAppTab *tab = user_data;

  /* Double-click to rename (FR-SESSION-06) */
  if (event->type == GDK_2BUTTON_PRESS && event->button == 1)
  {
    sk_app_tab_begin_rename(tab);
    return TRUE;
  }

  /* Right-click for context menu */
  if (event->type == GDK_BUTTON_PRESS && event->button == 3)
  {
    /* FR-TABS: context menu with rename, close, close & terminate */
    GtkWidget *menu = gtk_menu_new();

    GtkWidget *item_rename = gtk_menu_item_new_with_label(_("Rename"));
    g_signal_connect_swapped(item_rename, "activate", G_CALLBACK(sk_app_tab_begin_rename), tab);
    gtk_menu_shell_append(GTK_MENU_SHELL(menu), item_rename);

    GtkWidget *item_close = gtk_menu_item_new_with_label(_("Close (keep session)"));
    g_signal_connect_swapped(item_close, "activate", G_CALLBACK(on_tab_close_from_menu), tab);
    gtk_menu_shell_append(GTK_MENU_SHELL(menu), item_close);

    /* Fix: destroy menu on deactivate to prevent widget accumulation. */
    g_signal_connect(menu, "deactivate", G_CALLBACK(gtk_widget_destroy), NULL);

    gtk_widget_show_all(menu);
    gtk_menu_popup_at_pointer(GTK_MENU(menu), (GdkEvent *)event);
    return TRUE;
  }

  return FALSE;
}

/* ------------------------------------------------------------------ */
/* Tab label construction                                              */
/* ------------------------------------------------------------------ */

static void
init_tab_label(SkAppTab *tab, const char *title)
{
  SkTabLabel *lbl = &tab->label;

  lbl->box = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 4);

  /* Connection indicator dot -- FR-UI-04 */
  lbl->indicator = gtk_drawing_area_new();
  gtk_widget_set_size_request(lbl->indicator, 8, 8);
  gtk_widget_set_tooltip_text(lbl->indicator, _("Connection status"));
  update_indicator_css(lbl->indicator, SK_CONN_INDICATOR_GREEN);
  /* Green = invisible by default (FR-UI-04: green = no icon visible) */
  gtk_widget_set_no_show_all(lbl->indicator, TRUE);
  gtk_box_pack_start(GTK_BOX(lbl->box), lbl->indicator, FALSE, FALSE, 0);

  /* Warning icon for dead tabs (hidden by default) */
  lbl->warning_icon = gtk_image_new_from_icon_name("dialog-warning", GTK_ICON_SIZE_MENU);
  gtk_widget_set_tooltip_text(lbl->warning_icon, _("Session ended"));
  gtk_widget_set_no_show_all(lbl->warning_icon, TRUE);
  gtk_box_pack_start(GTK_BOX(lbl->box), lbl->warning_icon, FALSE, FALSE, 0);

  /* Title label */
  lbl->label = gtk_label_new(title);
  gtk_label_set_ellipsize(GTK_LABEL(lbl->label), PANGO_ELLIPSIZE_END);
  gtk_label_set_max_width_chars(GTK_LABEL(lbl->label), 30);
  gtk_box_pack_start(GTK_BOX(lbl->box), lbl->label, TRUE, TRUE, 0);

  /* Inline rename entry (hidden by default) */
  lbl->entry = gtk_entry_new();
  gtk_entry_set_text(GTK_ENTRY(lbl->entry), title);
  gtk_widget_set_no_show_all(lbl->entry, TRUE);
  g_signal_connect(lbl->entry, "activate", G_CALLBACK(on_tab_rename_activate), tab);
  g_signal_connect(lbl->entry, "focus-out-event", G_CALLBACK(on_tab_rename_focus_out), tab);
  g_signal_connect(lbl->entry, "key-press-event", G_CALLBACK(on_tab_rename_key_press), tab);
  gtk_box_pack_start(GTK_BOX(lbl->box), lbl->entry, TRUE, TRUE, 0);

  /* Close button */
  lbl->close_button = gtk_button_new_from_icon_name("window-close-symbolic", GTK_ICON_SIZE_MENU);
  gtk_button_set_relief(GTK_BUTTON(lbl->close_button), GTK_RELIEF_NONE);
  gtk_widget_set_focus_on_click(lbl->close_button, FALSE);
  gtk_widget_set_tooltip_text(lbl->close_button, _("Close tab"));
  /* Accessibility: label for close button */
  AtkObject *atk_close = gtk_widget_get_accessible(lbl->close_button);
  if (atk_close != NULL)
  {
    atk_object_set_name(atk_close, _("Close tab"));
  }
  g_signal_connect(lbl->close_button, "clicked", G_CALLBACK(on_tab_close_clicked), tab);
  gtk_box_pack_end(GTK_BOX(lbl->box), lbl->close_button, FALSE, FALSE, 0);

  /* Event box for double-click rename and right-click context menu */
  GtkWidget *evbox = gtk_event_box_new();
  gtk_event_box_set_visible_window(GTK_EVENT_BOX(evbox), FALSE);
  g_signal_connect(evbox, "button-press-event", G_CALLBACK(on_tab_label_button_press), tab);

  /* Reparent label box into event box */
  gtk_container_add(GTK_CONTAINER(evbox), lbl->box);

  /* Store the event box as the box for notebook use */
  lbl->box = evbox;

  gtk_widget_show_all(lbl->box);
}

/* ------------------------------------------------------------------ */
/* Window close handler (FR-TABS-17..19)                               */
/* ------------------------------------------------------------------ */

static int
count_active_tabs(SkAppWindow *win)
{
  int active = 0;
  for (guint i = 0; i < win->tabs->len; i++)
  {
    SkAppTab *tab = g_ptr_array_index(win->tabs, i);
    if (!tab->is_dead)
    {
      active++;
    }
  }
  return active;
}

static gboolean
on_window_delete_event(GtkWidget *widget G_GNUC_UNUSED, GdkEvent *event G_GNUC_UNUSED,
                       gpointer user_data)
{
  SkAppWindow *win = user_data;
  int n_active = count_active_tabs(win);

  /* FR-TABS-18: Only dead tabs -- close directly */
  if (n_active == 0)
  {
    return FALSE; /* Allow close */
  }

  /* FR-TABS-17: Show close window dialog */
  SkCloseResult result = sk_dialog_close_window(GTK_WINDOW(win->window), n_active);

  switch (result)
  {
  case SK_CLOSE_RESULT_HIDE:
    /* FR-TABS-17: Hide window (default) */
    gtk_widget_hide(win->window);

    /* FR-TABS-19: If last visible window, show tray toast */
    {
      GList *windows = gtk_application_get_windows(win->app);
      bool any_visible = false;
      for (GList *l = windows; l != NULL; l = l->next)
      {
        GtkWidget *w = l->data;
        if (w != win->window && gtk_widget_get_visible(w))
        {
          any_visible = true;
          break;
        }
      }
      if (!any_visible)
      {
        sk_toast_continues_in_tray(GTK_WINDOW(win->window));
      }
    }
    return TRUE; /* Prevent destroy */

  case SK_CLOSE_RESULT_TERMINATE:
    /* Allow the window to be destroyed -- caller handles session kill */
    return FALSE;

  case SK_CLOSE_RESULT_CANCEL:
  default:
    return TRUE; /* Prevent close */
  }
}

/* ------------------------------------------------------------------ */
/* CSS provider for tab indicators and dead tabs                       */
/* ------------------------------------------------------------------ */

static void
load_ui_css(void)
{
  static bool loaded = false;
  if (loaded)
    return;
  loaded = true;

  const char *css = ".indicator-green  { color: #4caf50; }\n"
                    ".indicator-yellow { color: #ff9800; }\n"
                    ".indicator-red    { color: #f44336; }\n"
                    ".tab-dead .tab-title { color: #f44336; }\n"
                    ".toast-overlay {\n"
                    "  background-color: rgba(50, 50, 50, 0.9);\n"
                    "  color: white;\n"
                    "  border-radius: 8px;\n"
                    "  padding: 12px 24px;\n"
                    "  margin: 12px;\n"
                    "}\n"
                    ".conn-feedback {\n"
                    "  background-color: rgba(0, 0, 0, 0.7);\n"
                    "  color: white;\n"
                    "  padding: 20px;\n"
                    "}\n"
                    ".welcome-screen {\n"
                    "  padding: 40px;\n"
                    "}\n"
                    ".welcome-screen .host-entry {\n"
                    "  font-size: 16px;\n"
                    "  padding: 8px;\n"
                    "}\n";

  GtkCssProvider *provider = gtk_css_provider_new();
  gtk_css_provider_load_from_data(provider, css, -1, NULL);
  gtk_style_context_add_provider_for_screen(gdk_screen_get_default(), GTK_STYLE_PROVIDER(provider),
                                            GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
  g_object_unref(provider);
}

/* ------------------------------------------------------------------ */
/* SkAppWindow lifecycle                                               */
/* ------------------------------------------------------------------ */

SkAppWindow *
sk_app_window_new(GtkApplication *app)
{
  return sk_app_window_new_from_state(app, NULL, -1, -1, 0, 0);
}

SkAppWindow *
sk_app_window_new_from_state(GtkApplication *app, const char *title, int x, int y, int width,
                             int height)
{
  g_return_val_if_fail(app != NULL, NULL);

  load_ui_css();

  SkAppWindow *win = g_new0(SkAppWindow, 1);
  win->app = app;
  win->tabs = g_ptr_array_new();

  /* Create window */
  win->window = gtk_application_window_new(app);
  gtk_window_set_title(GTK_WINDOW(win->window), title != NULL ? title : _("shellkeep"));

  /* Geometry -- FR-TABS: save/restore geometry */
  int w = (width > 0) ? width : 800;
  int h = (height > 0) ? height : 600;
  gtk_window_set_default_size(GTK_WINDOW(win->window), w, h);

  /* Position: X11 only (FR-TABS: position X11 only) */
  if (x >= 0 && y >= 0)
  {
    GdkDisplay *display = gdk_display_get_default();
    if (GDK_IS_DISPLAY(display))
    {
      /* Check if running under X11 by examining the display name */
      const char *name = gdk_display_get_name(display);
      if (name != NULL && name[0] == ':')
      {
        gtk_window_move(GTK_WINDOW(win->window), x, y);
      }
    }
  }

  /* Overlay for toasts and connection feedback */
  win->overlay = gtk_overlay_new();
  gtk_container_add(GTK_CONTAINER(win->window), win->overlay);

  /* Notebook for tabs */
  win->notebook = gtk_notebook_new();
  gtk_notebook_set_scrollable(GTK_NOTEBOOK(win->notebook), TRUE);
  gtk_notebook_set_show_tabs(GTK_NOTEBOOK(win->notebook), TRUE);
  gtk_notebook_set_show_border(GTK_NOTEBOOK(win->notebook), FALSE);
  gtk_container_add(GTK_CONTAINER(win->overlay), win->notebook);

  /* Close event handler */
  g_signal_connect(win->window, "delete-event", G_CALLBACK(on_window_delete_event), win);

  SK_LOG_INFO(SK_LOG_COMPONENT_UI, "created app window: %dx%d", w, h);

  return win;
}

GtkWidget *
sk_app_window_get_widget(SkAppWindow *win)
{
  g_return_val_if_fail(win != NULL, NULL);
  return win->window;
}

GtkWindow *
sk_app_window_get_gtk_window(SkAppWindow *win)
{
  g_return_val_if_fail(win != NULL, NULL);
  return GTK_WINDOW(win->window);
}

void
sk_app_window_show(SkAppWindow *win)
{
  g_return_if_fail(win != NULL);
  gtk_widget_show_all(win->window);
}

void
sk_app_window_hide(SkAppWindow *win)
{
  g_return_if_fail(win != NULL);
  gtk_widget_hide(win->window);
}

void
sk_app_window_free(SkAppWindow *win)
{
  if (win == NULL)
    return;

  /* Free all tabs */
  for (guint i = 0; i < win->tabs->len; i++)
  {
    SkAppTab *tab = g_ptr_array_index(win->tabs, i);
    g_free(tab->title);
    g_free(tab);
  }
  g_ptr_array_free(win->tabs, TRUE);

  if (win->window != NULL && GTK_IS_WIDGET(win->window))
  {
    gtk_widget_destroy(win->window);
  }

  g_free(win);
}

/* ------------------------------------------------------------------ */
/* Tab management                                                      */
/* ------------------------------------------------------------------ */

SkAppTab *
sk_app_window_add_tab(SkAppWindow *win, SkTerminalTab *terminal, const char *title)
{
  g_return_val_if_fail(win != NULL, NULL);
  g_return_val_if_fail(terminal != NULL, NULL);

  SkAppTab *tab = g_new0(SkAppTab, 1);
  tab->window = win;
  tab->terminal = terminal;
  tab->title = g_strdup(title != NULL ? title : _("New Tab"));
  tab->indicator = SK_CONN_INDICATOR_GREEN;
  tab->is_dead = false;

  /* Build the tab label widget */
  init_tab_label(tab, tab->title);

  /* Get the terminal widget to add as notebook page */
  GtkWidget *term_widget = sk_terminal_tab_get_widget(terminal);

  /* Add page to notebook */
  int page_num = gtk_notebook_append_page(GTK_NOTEBOOK(win->notebook), term_widget, tab->label.box);
  tab->page_num = page_num;

  /* Make tab reorderable within the notebook */
  gtk_notebook_set_tab_reorderable(GTK_NOTEBOOK(win->notebook), term_widget, TRUE);

  /* Track the tab */
  g_ptr_array_add(win->tabs, tab);

  /* Switch to the new tab */
  gtk_notebook_set_current_page(GTK_NOTEBOOK(win->notebook), page_num);

  gtk_widget_show_all(term_widget);

  SK_LOG_DEBUG(SK_LOG_COMPONENT_UI, "added tab '%s' at position %d", tab->title, page_num);

  return tab;
}

void
sk_app_window_remove_tab(SkAppWindow *win, SkAppTab *tab)
{
  g_return_if_fail(win != NULL);
  g_return_if_fail(tab != NULL);

  GtkWidget *term_widget = sk_terminal_tab_get_widget(tab->terminal);
  int page = gtk_notebook_page_num(GTK_NOTEBOOK(win->notebook), term_widget);
  if (page >= 0)
  {
    gtk_notebook_remove_page(GTK_NOTEBOOK(win->notebook), page);
  }

  g_ptr_array_remove(win->tabs, tab);

  SK_LOG_DEBUG(SK_LOG_COMPONENT_UI, "removed tab '%s'", tab->title);

  g_free(tab->title);
  g_free(tab);
}

int
sk_app_window_get_tab_count(const SkAppWindow *win)
{
  g_return_val_if_fail(win != NULL, 0);
  return (int)win->tabs->len;
}

SkAppTab *
sk_app_window_get_active_tab(SkAppWindow *win)
{
  g_return_val_if_fail(win != NULL, NULL);

  int current = gtk_notebook_get_current_page(GTK_NOTEBOOK(win->notebook));
  if (current < 0)
    return NULL;

  for (guint i = 0; i < win->tabs->len; i++)
  {
    SkAppTab *tab = g_ptr_array_index(win->tabs, i);
    GtkWidget *w = sk_terminal_tab_get_widget(tab->terminal);
    int pn = gtk_notebook_page_num(GTK_NOTEBOOK(win->notebook), w);
    if (pn == current)
      return tab;
  }
  return NULL;
}

void
sk_app_window_set_active_tab(SkAppWindow *win, int index)
{
  g_return_if_fail(win != NULL);
  gtk_notebook_set_current_page(GTK_NOTEBOOK(win->notebook), index);
}

void
sk_app_window_get_geometry(const SkAppWindow *win, int *x, int *y, int *width, int *height)
{
  g_return_if_fail(win != NULL);
  gtk_window_get_position(GTK_WINDOW(win->window), x, y);
  gtk_window_get_size(GTK_WINDOW(win->window), width, height);
}

bool
sk_app_window_is_visible(const SkAppWindow *win)
{
  g_return_val_if_fail(win != NULL, false);
  return gtk_widget_get_visible(win->window);
}

/* ------------------------------------------------------------------ */
/* Tab properties                                                      */
/* ------------------------------------------------------------------ */

const char *
sk_app_tab_get_title(const SkAppTab *tab)
{
  g_return_val_if_fail(tab != NULL, NULL);
  return tab->title;
}

void
sk_app_tab_set_title(SkAppTab *tab, const char *title)
{
  g_return_if_fail(tab != NULL);
  g_return_if_fail(title != NULL);

  g_free(tab->title);
  tab->title = g_strdup(title);
  gtk_label_set_text(GTK_LABEL(tab->label.label), title);
}

void
sk_app_tab_set_indicator(SkAppTab *tab, SkConnIndicator indicator)
{
  g_return_if_fail(tab != NULL);
  tab->indicator = indicator;
  update_indicator_css(tab->label.indicator, indicator);

  /* FR-UI-04: Green = no icon visible */
  if (indicator == SK_CONN_INDICATOR_GREEN)
  {
    gtk_widget_hide(tab->label.indicator);
  }
  else
  {
    gtk_widget_show(tab->label.indicator);
  }
}

void
sk_app_tab_set_dead(SkAppTab *tab, bool dead)
{
  g_return_if_fail(tab != NULL);
  tab->is_dead = dead;

  if (dead)
  {
    /* FR-UI-07: Red title + warning icon */
    GtkStyleContext *ctx = gtk_widget_get_style_context(tab->label.label);
    gtk_style_context_add_class(ctx, "tab-dead");
    gtk_widget_show(tab->label.warning_icon);

    /* Set red indicator */
    sk_app_tab_set_indicator(tab, SK_CONN_INDICATOR_RED);
  }
  else
  {
    GtkStyleContext *ctx = gtk_widget_get_style_context(tab->label.label);
    gtk_style_context_remove_class(ctx, "tab-dead");
    gtk_widget_hide(tab->label.warning_icon);
  }
}

bool
sk_app_tab_is_dead(const SkAppTab *tab)
{
  g_return_val_if_fail(tab != NULL, false);
  return tab->is_dead;
}

SkTerminalTab *
sk_app_tab_get_terminal(SkAppTab *tab)
{
  g_return_val_if_fail(tab != NULL, NULL);
  return tab->terminal;
}

void
sk_app_tab_begin_rename(SkAppTab *tab)
{
  g_return_if_fail(tab != NULL);

  /* Show entry, hide label -- FR-TABS-08, FR-SESSION-06 */
  gtk_entry_set_text(GTK_ENTRY(tab->label.entry), tab->title);
  gtk_widget_hide(tab->label.label);
  gtk_widget_show(tab->label.entry);
  gtk_widget_grab_focus(tab->label.entry);
  gtk_editable_select_region(GTK_EDITABLE(tab->label.entry), 0, -1);
}

/* ------------------------------------------------------------------ */
/* Callbacks                                                           */
/* ------------------------------------------------------------------ */

void
sk_app_window_set_tab_renamed_cb(SkAppWindow *win, SkTabRenamedCb cb, gpointer user_data)
{
  g_return_if_fail(win != NULL);
  win->renamed_cb = cb;
  win->renamed_cb_data = user_data;
}

void
sk_app_window_set_tab_closed_cb(SkAppWindow *win, SkTabClosedCb cb, gpointer user_data)
{
  g_return_if_fail(win != NULL);
  win->closed_cb = cb;
  win->closed_cb_data = user_data;
}

/* ------------------------------------------------------------------ */
/* Drag and drop (FR-TABS-03)                                          */
/* ------------------------------------------------------------------ */

void
sk_app_window_enable_tab_dnd(SkAppWindow *win)
{
  g_return_if_fail(win != NULL);
  if (win->dnd_enabled)
    return;

  /* GtkNotebook has built-in DnD support for tab reordering.
   * For cross-window DnD, we need to set the notebook group name
   * so tabs can be dragged between notebooks. */
  gtk_notebook_set_group_name(GTK_NOTEBOOK(win->notebook), "shellkeep-tabs");
  win->dnd_enabled = true;

  SK_LOG_DEBUG(SK_LOG_COMPONENT_UI, "enabled tab DnD for window");
}
