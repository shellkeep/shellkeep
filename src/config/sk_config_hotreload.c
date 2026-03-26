// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_config_hotreload.c
 * @brief Config file hot-reload via inotify with 500ms debounce.
 *
 * FR-CONFIG-04: hot-reloadable settings are [terminal] (except scrollback),
 * [keybindings], theme, auto_save_interval, close_to_tray.
 *
 * Uses Linux inotify watching for IN_CLOSE_WRITE events, integrated
 * into the GLib main loop via GIOChannel. A 500ms debounce timer
 * coalesces rapid successive writes.
 */

#include "shellkeep/sk_config.h"
#include "shellkeep/sk_log.h"

#include <errno.h>
#include <string.h>
#include <sys/inotify.h>
#include <unistd.h>

/* ------------------------------------------------------------------ */
/* Internal watcher state                                              */
/* ------------------------------------------------------------------ */

struct _SkConfigWatcher
{
  int inotify_fd;        /**< inotify file descriptor. */
  int watch_fd;          /**< Watch descriptor for config dir. */
  GIOChannel *channel;   /**< GLib IO channel for inotify fd. */
  guint source_id;       /**< GLib source ID for the channel. */
  guint debounce_id;     /**< GLib timeout source for debounce. */
  char *config_path;     /**< Full path to config.ini. */
  char *config_dir;      /**< Directory containing config.ini. */
  char *config_basename; /**< Basename of config file. */
  SkConfigReloadCallback callback;
  void *user_data;
};

/* ------------------------------------------------------------------ */
/* Debounce timeout — FR-CONFIG-04: 500ms                              */
/* ------------------------------------------------------------------ */

#define SK_CONFIG_DEBOUNCE_MS 500

static gboolean
on_debounce_timeout(gpointer data)
{
  SkConfigWatcher *w = data;
  GError *error = NULL;
  SkConfig *new_config;

  w->debounce_id = 0; /* one-shot */

  SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "config: hot-reloading %s", w->config_path);

  new_config = sk_config_load(w->config_path, &error);
  if (new_config == NULL)
  {
    SK_LOG_ERROR(SK_LOG_COMPONENT_GENERAL, "config: hot-reload failed: %s",
                 error ? error->message : "unknown error");
    g_clear_error(&error);
    return G_SOURCE_REMOVE;
  }

  if (w->callback != NULL)
    w->callback(new_config, w->user_data);
  else
    sk_config_free(new_config);

  return G_SOURCE_REMOVE;
}

/* ------------------------------------------------------------------ */
/* inotify event handler                                               */
/* ------------------------------------------------------------------ */

static gboolean
on_inotify_event(GIOChannel *source G_GNUC_UNUSED, GIOCondition condition G_GNUC_UNUSED,
                 gpointer data)
{
  SkConfigWatcher *w = data;
  char buf[4096] __attribute__((aligned(__alignof__(struct inotify_event))));
  ssize_t len;
  bool config_changed = false;
  const char *ptr;

  /* Read all pending events */
  len = read(w->inotify_fd, buf, sizeof(buf));
  if (len <= 0)
  {
    if (errno == EAGAIN || errno == EWOULDBLOCK)
      return G_SOURCE_CONTINUE;
    SK_LOG_ERROR(SK_LOG_COMPONENT_GENERAL, "config: inotify read error: %s", strerror(errno));
    return G_SOURCE_REMOVE;
  }

  /* Scan events for our config file */
  ptr = buf;
  while (ptr < buf + len)
  {
    const struct inotify_event *event = (const struct inotify_event *)(const void *)ptr;

    if (event->len > 0)
    {
      /* Check if the modified file matches our config basename */
      if (g_strcmp0(event->name, w->config_basename) == 0)
      {
        if (event->mask & IN_CLOSE_WRITE)
          config_changed = true;
      }
    }

    ptr += sizeof(struct inotify_event) + event->len;
  }

  if (config_changed)
  {
    /* Reset debounce timer */
    if (w->debounce_id != 0)
      g_source_remove(w->debounce_id);

    w->debounce_id = g_timeout_add(SK_CONFIG_DEBOUNCE_MS, on_debounce_timeout, w);
  }

  return G_SOURCE_CONTINUE;
}

/* ------------------------------------------------------------------ */
/* Public API — FR-CONFIG-04                                           */
/* ------------------------------------------------------------------ */

SkConfigWatcher *
sk_config_watch_start(const char *config_path, SkConfigReloadCallback callback, void *user_data,
                      GError **error)
{
  char *resolved;
  SkConfigWatcher *w;

  resolved = config_path != NULL
                 ? g_strdup(config_path)
                 : g_build_filename(g_get_user_config_dir(), "shellkeep", "config.ini", NULL);

  w = g_new0(SkConfigWatcher, 1);
  w->config_path = resolved;
  w->config_dir = g_path_get_dirname(resolved);
  w->config_basename = g_path_get_basename(resolved);
  w->callback = callback;
  w->user_data = user_data;
  w->inotify_fd = -1;
  w->watch_fd = -1;
  w->source_id = 0;
  w->debounce_id = 0;

  /* Create inotify instance */
  w->inotify_fd = inotify_init1(IN_NONBLOCK | IN_CLOEXEC);
  if (w->inotify_fd < 0)
  {
    g_set_error(error, SK_ERROR, SK_ERROR_IO, "inotify_init1 failed: %s", strerror(errno));
    sk_config_watch_stop(w);
    return NULL;
  }

  /* Ensure config directory exists (it may not yet) */
  if (g_mkdir_with_parents(w->config_dir, 0700) != 0)
  {
    g_set_error(error, SK_ERROR, SK_ERROR_IO, "Cannot create config directory: %s: %s",
                w->config_dir, strerror(errno));
    sk_config_watch_stop(w);
    return NULL;
  }

  /* Watch the directory for IN_CLOSE_WRITE events.
   * We watch the directory rather than the file so that we catch
   * editors that write-to-temp-then-rename (which deletes the inode). */
  w->watch_fd = inotify_add_watch(w->inotify_fd, w->config_dir, IN_CLOSE_WRITE | IN_MOVED_TO);
  if (w->watch_fd < 0)
  {
    g_set_error(error, SK_ERROR, SK_ERROR_IO, "inotify_add_watch failed for %s: %s", w->config_dir,
                strerror(errno));
    sk_config_watch_stop(w);
    return NULL;
  }

  /* Integrate with GLib main loop via GIOChannel */
  w->channel = g_io_channel_unix_new(w->inotify_fd);
  g_io_channel_set_close_on_unref(w->channel, FALSE); /* We close fd manually */
  g_io_channel_set_encoding(w->channel, NULL, NULL);
  g_io_channel_set_buffered(w->channel, FALSE);

  w->source_id = g_io_add_watch(w->channel, G_IO_IN | G_IO_HUP | G_IO_ERR, on_inotify_event, w);

  SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "config: watching %s for changes (debounce=%dms)",
              w->config_path, SK_CONFIG_DEBOUNCE_MS);

  return w;
}

void
sk_config_watch_stop(SkConfigWatcher *watcher)
{
  if (watcher == NULL)
    return;

  /* Remove debounce timer */
  if (watcher->debounce_id != 0)
  {
    g_source_remove(watcher->debounce_id);
    watcher->debounce_id = 0;
  }

  /* Remove GLib IO source */
  if (watcher->source_id != 0)
  {
    g_source_remove(watcher->source_id);
    watcher->source_id = 0;
  }

  /* Close GIOChannel (does not close fd if we unref properly) */
  if (watcher->channel != NULL)
  {
    g_io_channel_unref(watcher->channel);
    watcher->channel = NULL;
  }

  /* Remove inotify watch */
  if (watcher->inotify_fd >= 0 && watcher->watch_fd >= 0)
  {
    inotify_rm_watch(watcher->inotify_fd, watcher->watch_fd);
    watcher->watch_fd = -1;
  }

  /* Close inotify fd */
  if (watcher->inotify_fd >= 0)
  {
    close(watcher->inotify_fd);
    watcher->inotify_fd = -1;
  }

  g_free(watcher->config_path);
  g_free(watcher->config_dir);
  g_free(watcher->config_basename);
  g_free(watcher);

  SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "config: file watcher stopped");
}
