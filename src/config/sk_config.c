// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_config.c
 * @brief INI configuration loading, validation, and defaults.
 *
 * Uses GLib GKeyFile for parsing. FR-CONFIG-01..03, FR-CONFIG-05..08.
 */

#include "shellkeep/sk_config.h"

#include "shellkeep/sk_log.h"

#include <errno.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

/* ------------------------------------------------------------------ */
/* Helpers                                                             */
/* ------------------------------------------------------------------ */

/** Clamp an integer to [lo, hi] and warn if clamped. */
static int
clamp_int(const char *key, int val, int lo, int hi)
{
  if (val < lo)
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_GENERAL, "config: %s=%d clamped to minimum %d", key, val, lo);
    return lo;
  }
  if (val > hi)
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_GENERAL, "config: %s=%d clamped to maximum %d", key, val, hi);
    return hi;
  }
  return val;
}

/** Clamp a double to [lo, hi] and warn if clamped. */
static double
clamp_double(const char *key, double val, double lo, double hi)
{
  if (val < lo)
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_GENERAL, "config: %s=%.2f clamped to minimum %.2f", key, val, lo);
    return lo;
  }
  if (val > hi)
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_GENERAL, "config: %s=%.2f clamped to maximum %.2f", key, val, hi);
    return hi;
  }
  return val;
}

/** Read a GKeyFile string, returning a g_strdup'd value or NULL. */
static char *
keyfile_string_or_null(GKeyFile *kf, const char *group, const char *key)
{
  GError *err = NULL;
  char *val = g_key_file_get_string(kf, group, key, &err);
  if (err != NULL)
  {
    g_error_free(err);
    return NULL;
  }
  return val;
}

/** Read a GKeyFile int, returning default if not present. */
static int
keyfile_int_or_default(GKeyFile *kf, const char *group, const char *key, int def)
{
  GError *err = NULL;
  int val = g_key_file_get_integer(kf, group, key, &err);
  if (err != NULL)
  {
    g_error_free(err);
    return def;
  }
  return val;
}

/** Read a GKeyFile bool, returning default if not present. */
static bool
keyfile_bool_or_default(GKeyFile *kf, const char *group, const char *key, bool def)
{
  GError *err = NULL;
  gboolean val = g_key_file_get_boolean(kf, group, key, &err);
  if (err != NULL)
  {
    g_error_free(err);
    return def;
  }
  return val != FALSE;
}

/** Read a GKeyFile double, returning default if not present. */
static double
keyfile_double_or_default(GKeyFile *kf, const char *group, const char *key, double def)
{
  GError *err = NULL;
  double val = g_key_file_get_double(kf, group, key, &err);
  if (err != NULL)
  {
    g_error_free(err);
    return def;
  }
  return val;
}

/* ------------------------------------------------------------------ */
/* Enum parsing helpers — FR-CONFIG-03 (fallback on unknown value)     */
/* ------------------------------------------------------------------ */

static SkStartupBehavior
parse_startup_behavior(const char *val)
{
  if (val == NULL)
    return SK_STARTUP_WELCOME_SCREEN;
  if (g_strcmp0(val, "last_session") == 0)
    return SK_STARTUP_LAST_SESSION;
  if (g_strcmp0(val, "minimized") == 0)
    return SK_STARTUP_MINIMIZED;
  if (g_strcmp0(val, "welcome_screen") == 0)
    return SK_STARTUP_WELCOME_SCREEN;
  SK_LOG_WARN(SK_LOG_COMPONENT_GENERAL,
              "config: unknown startup_behavior '%s', using 'welcome_screen'", val);
  return SK_STARTUP_WELCOME_SCREEN;
}

static SkBellMode
parse_bell(const char *val)
{
  if (val == NULL)
    return SK_BELL_VISUAL;
  if (g_strcmp0(val, "visual") == 0)
    return SK_BELL_VISUAL;
  if (g_strcmp0(val, "audible") == 0)
    return SK_BELL_AUDIBLE;
  if (g_strcmp0(val, "none") == 0)
    return SK_BELL_NONE;
  SK_LOG_WARN(SK_LOG_COMPONENT_GENERAL, "config: unknown bell '%s', using 'visual'", val);
  return SK_BELL_VISUAL;
}

static SkCursorShape
parse_cursor_shape(const char *val)
{
  if (val == NULL)
    return SK_CURSOR_BLOCK;
  if (g_strcmp0(val, "block") == 0)
    return SK_CURSOR_BLOCK;
  if (g_strcmp0(val, "ibeam") == 0)
    return SK_CURSOR_IBEAM;
  if (g_strcmp0(val, "underline") == 0)
    return SK_CURSOR_UNDERLINE;
  SK_LOG_WARN(SK_LOG_COMPONENT_GENERAL, "config: unknown cursor_shape '%s', using 'block'", val);
  return SK_CURSOR_BLOCK;
}

static SkCursorBlink
parse_cursor_blink(const char *val)
{
  if (val == NULL)
    return SK_CURSOR_BLINK_SYSTEM;
  if (g_strcmp0(val, "system") == 0)
    return SK_CURSOR_BLINK_SYSTEM;
  if (g_strcmp0(val, "on") == 0)
    return SK_CURSOR_BLINK_ON;
  if (g_strcmp0(val, "off") == 0)
    return SK_CURSOR_BLINK_OFF;
  SK_LOG_WARN(SK_LOG_COMPONENT_GENERAL, "config: unknown cursor_blink '%s', using 'system'", val);
  return SK_CURSOR_BLINK_SYSTEM;
}

/* ------------------------------------------------------------------ */
/* Unknown key detection — FR-CONFIG-03                                */
/* ------------------------------------------------------------------ */

static void
warn_unknown_keys(GKeyFile *kf, const char *section, const char *const *known, int n_known)
{
  gsize n_keys = 0;
  GError *err = NULL;
  gchar **keys = g_key_file_get_keys(kf, section, &n_keys, &err);

  if (err != NULL)
  {
    g_error_free(err);
    return;
  }

  for (gsize i = 0; i < n_keys; i++)
  {
    bool found = false;
    for (int k = 0; k < n_known; k++)
    {
      if (g_strcmp0(keys[i], known[k]) == 0)
      {
        found = true;
        break;
      }
    }
    if (!found)
    {
      SK_LOG_WARN(SK_LOG_COMPONENT_GENERAL, "config: unknown key '%s' in [%s], ignored", keys[i],
                  section);
    }
  }

  g_strfreev(keys);
}

/* ------------------------------------------------------------------ */
/* Known keys per section                                              */
/* ------------------------------------------------------------------ */

/* clang-format off */
static const char *const general_keys[] = {
  "client_id", "theme", "startup_behavior",
};

static const char *const terminal_keys[] = {
  "font_family", "font_size", "scrollback_lines", "bell",
  "cursor_shape", "cursor_blink", "word_chars", "bold_is_bright",
  "allow_hyperlinks",
};

static const char *const ssh_keys[] = {
  "connect_timeout", "keepalive_interval", "keepalive_count_max",
  "known_hosts_file", "identity_file", "use_ssh_config",
  "reconnect_max_attempts", "reconnect_backoff_base",
};

static const char *const keybinding_keys[] = {
  "new_tab", "close_tab", "new_window", "rename_tab", "find",
  "next_tab", "prev_tab", "copy", "paste", "copy_all",
  "export_scrollback", "zoom_in", "zoom_out", "zoom_reset",
};

static const char *const state_keys[] = {
  "history_max_size_mb", "history_max_days", "auto_save_interval",
};

static const char *const tray_keys[] = {
  "enabled", "close_to_tray", "start_minimized",
};
/* clang-format on */

/* ------------------------------------------------------------------ */
/* Default config path                                                 */
/* ------------------------------------------------------------------ */

char *
sk_config_get_dir(void)
{
  const char *xdg = g_get_user_config_dir();
  return g_build_filename(xdg, "shellkeep", NULL);
}

static char *
default_config_path(void)
{
  char *dir = sk_config_get_dir();
  char *path = g_build_filename(dir, "config.ini", NULL);
  g_free(dir);
  return path;
}

/* ------------------------------------------------------------------ */
/* Defaults — FR-CONFIG-05, FR-CONFIG-06                               */
/* ------------------------------------------------------------------ */

SkConfig *
sk_config_new_defaults(void)
{
  SkConfig *c = g_new0(SkConfig, 1);

  /* [general] */
  c->client_id = NULL; /* resolved later via sk_config_resolve_client_id */
  c->theme_name = g_strdup("system");
  c->startup_behavior = SK_STARTUP_WELCOME_SCREEN;

  /* [terminal] FR-CONFIG-05 */
  c->font_family = g_strdup("Monospace");
  c->font_size = 12;
  c->scrollback_lines = 10000;
  c->bell = SK_BELL_VISUAL;
  c->cursor_shape = SK_CURSOR_BLOCK;
  c->cursor_blink = SK_CURSOR_BLINK_SYSTEM;
  c->word_chars = g_strdup("-A-Za-z0-9_./:~");
  c->bold_is_bright = true;
  c->allow_hyperlinks = true;

  /* [ssh] FR-CONFIG-06 */
  c->ssh_connect_timeout = 10;
  c->ssh_keepalive_interval = 15;
  c->ssh_keepalive_count_max = 3;
  c->ssh_known_hosts_file = g_strdup("");
  c->ssh_identity_file = g_strdup("");
  c->ssh_use_ssh_config = true;
  c->ssh_reconnect_max_attempts = 10;
  c->ssh_reconnect_backoff_base = 2.0;

  /* [keybindings] */
  c->kb_new_tab = g_strdup("Ctrl+Shift+T");
  c->kb_close_tab = g_strdup("Ctrl+Shift+W");
  c->kb_new_window = g_strdup("Ctrl+Shift+N");
  c->kb_rename_tab = g_strdup("F2");
  c->kb_find = g_strdup("Ctrl+Shift+F");
  c->kb_next_tab = g_strdup("Ctrl+Tab");
  c->kb_prev_tab = g_strdup("Ctrl+Shift+Tab");
  c->kb_copy = g_strdup("Ctrl+Shift+C");
  c->kb_paste = g_strdup("Ctrl+Shift+V");
  c->kb_copy_all = g_strdup("Ctrl+Shift+A");
  c->kb_export_scrollback = g_strdup("");
  c->kb_zoom_in = g_strdup("Ctrl+Shift+plus");
  c->kb_zoom_out = g_strdup("Ctrl+Shift+minus");
  c->kb_zoom_reset = g_strdup("Ctrl+Shift+0");

  /* [state] */
  c->history_max_size_mb = 50;
  c->history_max_days = 90;
  c->auto_save_interval = 30;

  /* [tray] */
  c->tray_enabled = true;
  c->close_to_tray = true;
  c->start_minimized = false;

  /* internal */
  c->config_path = NULL;
  c->theme = NULL;

  return c;
}

/* ------------------------------------------------------------------ */
/* Free                                                                */
/* ------------------------------------------------------------------ */

void
sk_config_free(SkConfig *config)
{
  if (config == NULL)
    return;

  g_free(config->client_id);
  g_free(config->theme_name);
  g_free(config->font_family);
  g_free(config->word_chars);
  g_free(config->ssh_known_hosts_file);
  g_free(config->ssh_identity_file);
  g_free(config->kb_new_tab);
  g_free(config->kb_close_tab);
  g_free(config->kb_new_window);
  g_free(config->kb_rename_tab);
  g_free(config->kb_find);
  g_free(config->kb_next_tab);
  g_free(config->kb_prev_tab);
  g_free(config->kb_copy);
  g_free(config->kb_paste);
  g_free(config->kb_copy_all);
  g_free(config->kb_export_scrollback);
  g_free(config->kb_zoom_in);
  g_free(config->kb_zoom_out);
  g_free(config->kb_zoom_reset);
  g_free(config->config_path);

  sk_theme_free(config->theme);

  g_free(config);
}

/* ------------------------------------------------------------------ */
/* GKeyFile parsing — FR-CONFIG-01, FR-CONFIG-03                       */
/* ------------------------------------------------------------------ */

/** Parse [general] section. */
static void
parse_general(SkConfig *c, GKeyFile *kf)
{
  char *val;

  if (!g_key_file_has_group(kf, "general"))
    return;

  warn_unknown_keys(kf, "general", general_keys, G_N_ELEMENTS(general_keys));

  val = keyfile_string_or_null(kf, "general", "client_id");
  if (val != NULL)
  {
    g_free(c->client_id);
    c->client_id = val;
  }

  val = keyfile_string_or_null(kf, "general", "theme");
  if (val != NULL)
  {
    g_free(c->theme_name);
    c->theme_name = val;
  }

  val = keyfile_string_or_null(kf, "general", "startup_behavior");
  if (val != NULL)
  {
    c->startup_behavior = parse_startup_behavior(val);
    g_free(val);
  }
}

/** Parse [terminal] section. FR-CONFIG-05 */
static void
parse_terminal(SkConfig *c, GKeyFile *kf)
{
  char *val;

  if (!g_key_file_has_group(kf, "terminal"))
    return;

  warn_unknown_keys(kf, "terminal", terminal_keys, G_N_ELEMENTS(terminal_keys));

  val = keyfile_string_or_null(kf, "terminal", "font_family");
  if (val != NULL)
  {
    g_free(c->font_family);
    c->font_family = val;
  }

  c->font_size = clamp_int("terminal.font_size",
                            keyfile_int_or_default(kf, "terminal", "font_size", c->font_size), 6,
                            72);

  c->scrollback_lines =
      clamp_int("terminal.scrollback_lines",
                keyfile_int_or_default(kf, "terminal", "scrollback_lines", c->scrollback_lines), 0,
                1000000);

  val = keyfile_string_or_null(kf, "terminal", "bell");
  if (val != NULL)
  {
    c->bell = parse_bell(val);
    g_free(val);
  }

  val = keyfile_string_or_null(kf, "terminal", "cursor_shape");
  if (val != NULL)
  {
    c->cursor_shape = parse_cursor_shape(val);
    g_free(val);
  }

  val = keyfile_string_or_null(kf, "terminal", "cursor_blink");
  if (val != NULL)
  {
    c->cursor_blink = parse_cursor_blink(val);
    g_free(val);
  }

  val = keyfile_string_or_null(kf, "terminal", "word_chars");
  if (val != NULL)
  {
    g_free(c->word_chars);
    c->word_chars = val;
  }

  c->bold_is_bright = keyfile_bool_or_default(kf, "terminal", "bold_is_bright", c->bold_is_bright);
  c->allow_hyperlinks =
      keyfile_bool_or_default(kf, "terminal", "allow_hyperlinks", c->allow_hyperlinks);
}

/** Parse [ssh] section. FR-CONFIG-06 */
static void
parse_ssh(SkConfig *c, GKeyFile *kf)
{
  char *val;

  if (!g_key_file_has_group(kf, "ssh"))
    return;

  warn_unknown_keys(kf, "ssh", ssh_keys, G_N_ELEMENTS(ssh_keys));

  c->ssh_connect_timeout =
      clamp_int("ssh.connect_timeout",
                keyfile_int_or_default(kf, "ssh", "connect_timeout", c->ssh_connect_timeout), 1,
                300);

  c->ssh_keepalive_interval = clamp_int(
      "ssh.keepalive_interval",
      keyfile_int_or_default(kf, "ssh", "keepalive_interval", c->ssh_keepalive_interval), 0, 600);

  c->ssh_keepalive_count_max = clamp_int(
      "ssh.keepalive_count_max",
      keyfile_int_or_default(kf, "ssh", "keepalive_count_max", c->ssh_keepalive_count_max), 1, 30);

  val = keyfile_string_or_null(kf, "ssh", "known_hosts_file");
  if (val != NULL)
  {
    g_free(c->ssh_known_hosts_file);
    c->ssh_known_hosts_file = val;
  }

  val = keyfile_string_or_null(kf, "ssh", "identity_file");
  if (val != NULL)
  {
    g_free(c->ssh_identity_file);
    c->ssh_identity_file = val;
  }

  c->ssh_use_ssh_config =
      keyfile_bool_or_default(kf, "ssh", "use_ssh_config", c->ssh_use_ssh_config);

  c->ssh_reconnect_max_attempts =
      clamp_int("ssh.reconnect_max_attempts",
                keyfile_int_or_default(kf, "ssh", "reconnect_max_attempts",
                                       c->ssh_reconnect_max_attempts),
                0, 100);

  c->ssh_reconnect_backoff_base =
      clamp_double("ssh.reconnect_backoff_base",
                   keyfile_double_or_default(kf, "ssh", "reconnect_backoff_base",
                                             c->ssh_reconnect_backoff_base),
                   0.5, 30.0);
}

/** Parse [keybindings] section. */
static void
parse_keybindings(SkConfig *c, GKeyFile *kf)
{
  if (!g_key_file_has_group(kf, "keybindings"))
    return;

  warn_unknown_keys(kf, "keybindings", keybinding_keys, G_N_ELEMENTS(keybinding_keys));

  /* Helper macro: read key or keep default */
#define READ_KB(field, name)                                                                       \
  do                                                                                               \
  {                                                                                                \
    char *v_ = keyfile_string_or_null(kf, "keybindings", (name));                                  \
    if (v_ != NULL)                                                                                \
    {                                                                                              \
      g_free(c->field);                                                                            \
      c->field = v_;                                                                               \
    }                                                                                              \
  } while (0)

  READ_KB(kb_new_tab, "new_tab");
  READ_KB(kb_close_tab, "close_tab");
  READ_KB(kb_new_window, "new_window");
  READ_KB(kb_rename_tab, "rename_tab");
  READ_KB(kb_find, "find");
  READ_KB(kb_next_tab, "next_tab");
  READ_KB(kb_prev_tab, "prev_tab");
  READ_KB(kb_copy, "copy");
  READ_KB(kb_paste, "paste");
  READ_KB(kb_copy_all, "copy_all");
  READ_KB(kb_export_scrollback, "export_scrollback");
  READ_KB(kb_zoom_in, "zoom_in");
  READ_KB(kb_zoom_out, "zoom_out");
  READ_KB(kb_zoom_reset, "zoom_reset");

#undef READ_KB
}

/** Parse [state] section. */
static void
parse_state(SkConfig *c, GKeyFile *kf)
{
  if (!g_key_file_has_group(kf, "state"))
    return;

  warn_unknown_keys(kf, "state", state_keys, G_N_ELEMENTS(state_keys));

  c->history_max_size_mb =
      clamp_int("state.history_max_size_mb",
                keyfile_int_or_default(kf, "state", "history_max_size_mb", c->history_max_size_mb),
                1, 10000);

  c->history_max_days =
      clamp_int("state.history_max_days",
                keyfile_int_or_default(kf, "state", "history_max_days", c->history_max_days), 1,
                3650);

  c->auto_save_interval =
      clamp_int("state.auto_save_interval",
                keyfile_int_or_default(kf, "state", "auto_save_interval", c->auto_save_interval), 5,
                600);
}

/** Parse [tray] section. */
static void
parse_tray(SkConfig *c, GKeyFile *kf)
{
  if (!g_key_file_has_group(kf, "tray"))
    return;

  warn_unknown_keys(kf, "tray", tray_keys, G_N_ELEMENTS(tray_keys));

  c->tray_enabled = keyfile_bool_or_default(kf, "tray", "enabled", c->tray_enabled);
  c->close_to_tray = keyfile_bool_or_default(kf, "tray", "close_to_tray", c->close_to_tray);
  c->start_minimized = keyfile_bool_or_default(kf, "tray", "start_minimized", c->start_minimized);
}

/* ------------------------------------------------------------------ */
/* Load — FR-CONFIG-01                                                 */
/* ------------------------------------------------------------------ */

SkConfig *
sk_config_load(const char *path, GError **error)
{
  SkConfig *c;
  char *resolved;
  GKeyFile *kf;
  GError *local_error = NULL;

  c = sk_config_new_defaults();
  if (c == NULL)
  {
    g_set_error(error, SK_ERROR, SK_ERROR_ALLOC, "Failed to allocate config");
    return NULL;
  }

  /* Resolve path */
  resolved = path != NULL ? g_strdup(path) : default_config_path();
  c->config_path = g_strdup(resolved);

  /* FR-CONFIG-01: file not found = use defaults, no file created */
  if (!g_file_test(resolved, G_FILE_TEST_EXISTS))
  {
    SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "config: file not found at %s, using defaults", resolved);
    g_free(resolved);
    return c;
  }

  /* Open and parse */
  kf = g_key_file_new();

  if (!g_key_file_load_from_file(kf, resolved, G_KEY_FILE_NONE, &local_error))
  {
    /* FR-CONFIG-03: parse error = use defaults + log */
    SK_LOG_ERROR(SK_LOG_COMPONENT_GENERAL, "config: parse error in %s: %s", resolved,
                 local_error->message);
    g_error_free(local_error);
    g_key_file_free(kf);
    g_free(resolved);
    return c; /* use defaults */
  }

  g_free(resolved);

  /* Parse each section */
  parse_general(c, kf);
  parse_terminal(c, kf);
  parse_ssh(c, kf);
  parse_keybindings(c, kf);
  parse_state(c, kf);
  parse_tray(c, kf);

  g_key_file_free(kf);

  SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "config: loaded from %s", c->config_path);

  return c;
}

/* ------------------------------------------------------------------ */
/* Save — write config as INI                                          */
/* ------------------------------------------------------------------ */

static const char *
startup_behavior_to_string(SkStartupBehavior b)
{
  switch (b)
  {
  case SK_STARTUP_LAST_SESSION:
    return "last_session";
  case SK_STARTUP_WELCOME_SCREEN:
    return "welcome_screen";
  case SK_STARTUP_MINIMIZED:
    return "minimized";
  }
  return "welcome_screen";
}

static const char *
bell_to_string(SkBellMode b)
{
  switch (b)
  {
  case SK_BELL_VISUAL:
    return "visual";
  case SK_BELL_AUDIBLE:
    return "audible";
  case SK_BELL_NONE:
    return "none";
  }
  return "visual";
}

static const char *
cursor_shape_to_string(SkCursorShape s)
{
  switch (s)
  {
  case SK_CURSOR_BLOCK:
    return "block";
  case SK_CURSOR_IBEAM:
    return "ibeam";
  case SK_CURSOR_UNDERLINE:
    return "underline";
  }
  return "block";
}

static const char *
cursor_blink_to_string(SkCursorBlink b)
{
  switch (b)
  {
  case SK_CURSOR_BLINK_SYSTEM:
    return "system";
  case SK_CURSOR_BLINK_ON:
    return "on";
  case SK_CURSOR_BLINK_OFF:
    return "off";
  }
  return "system";
}

bool
sk_config_save(const SkConfig *config, const char *path, GError **error)
{
  char *resolved;
  char *dir;
  GString *buf;
  bool ok;

  if (config == NULL)
  {
    g_set_error(error, SK_ERROR, SK_ERROR_GENERIC, "config is NULL");
    return false;
  }

  resolved = path != NULL ? g_strdup(path) : default_config_path();

  /* Ensure directory exists (0700 permissions — security rule) */
  dir = g_path_get_dirname(resolved);
  if (g_mkdir_with_parents(dir, 0700) != 0)
  {
    g_set_error(error, SK_ERROR, SK_ERROR_IO, "Cannot create config directory: %s", dir);
    g_free(dir);
    g_free(resolved);
    return false;
  }
  g_free(dir);

  buf = g_string_new("# shellkeep configuration\n"
                     "# Auto-saved by shellkeep\n\n");

  /* [general] */
  g_string_append(buf, "[general]\n");
  if (config->client_id != NULL)
    g_string_append_printf(buf, "client_id = %s\n", config->client_id);
  g_string_append_printf(buf, "theme = %s\n", config->theme_name);
  g_string_append_printf(buf, "startup_behavior = %s\n",
                         startup_behavior_to_string(config->startup_behavior));

  /* [terminal] */
  g_string_append(buf, "\n[terminal]\n");
  g_string_append_printf(buf, "font_family = %s\n", config->font_family);
  g_string_append_printf(buf, "font_size = %d\n", config->font_size);
  g_string_append_printf(buf, "scrollback_lines = %d\n", config->scrollback_lines);
  g_string_append_printf(buf, "bell = %s\n", bell_to_string(config->bell));
  g_string_append_printf(buf, "cursor_shape = %s\n",
                         cursor_shape_to_string(config->cursor_shape));
  g_string_append_printf(buf, "cursor_blink = %s\n",
                         cursor_blink_to_string(config->cursor_blink));
  g_string_append_printf(buf, "word_chars = %s\n", config->word_chars);
  g_string_append_printf(buf, "bold_is_bright = %s\n", config->bold_is_bright ? "true" : "false");
  g_string_append_printf(buf, "allow_hyperlinks = %s\n",
                         config->allow_hyperlinks ? "true" : "false");

  /* [ssh] */
  g_string_append(buf, "\n[ssh]\n");
  g_string_append_printf(buf, "connect_timeout = %d\n", config->ssh_connect_timeout);
  g_string_append_printf(buf, "keepalive_interval = %d\n", config->ssh_keepalive_interval);
  g_string_append_printf(buf, "keepalive_count_max = %d\n", config->ssh_keepalive_count_max);
  g_string_append_printf(buf, "known_hosts_file = %s\n", config->ssh_known_hosts_file);
  g_string_append_printf(buf, "identity_file = %s\n", config->ssh_identity_file);
  g_string_append_printf(buf, "use_ssh_config = %s\n",
                         config->ssh_use_ssh_config ? "true" : "false");
  g_string_append_printf(buf, "reconnect_max_attempts = %d\n", config->ssh_reconnect_max_attempts);
  g_string_append_printf(buf, "reconnect_backoff_base = %.1f\n",
                         config->ssh_reconnect_backoff_base);

  /* [keybindings] */
  g_string_append(buf, "\n[keybindings]\n");
  g_string_append_printf(buf, "new_tab = %s\n", config->kb_new_tab);
  g_string_append_printf(buf, "close_tab = %s\n", config->kb_close_tab);
  g_string_append_printf(buf, "new_window = %s\n", config->kb_new_window);
  g_string_append_printf(buf, "rename_tab = %s\n", config->kb_rename_tab);
  g_string_append_printf(buf, "find = %s\n", config->kb_find);
  g_string_append_printf(buf, "next_tab = %s\n", config->kb_next_tab);
  g_string_append_printf(buf, "prev_tab = %s\n", config->kb_prev_tab);
  g_string_append_printf(buf, "copy = %s\n", config->kb_copy);
  g_string_append_printf(buf, "paste = %s\n", config->kb_paste);
  g_string_append_printf(buf, "copy_all = %s\n", config->kb_copy_all);
  g_string_append_printf(buf, "export_scrollback = %s\n", config->kb_export_scrollback);
  g_string_append_printf(buf, "zoom_in = %s\n", config->kb_zoom_in);
  g_string_append_printf(buf, "zoom_out = %s\n", config->kb_zoom_out);
  g_string_append_printf(buf, "zoom_reset = %s\n", config->kb_zoom_reset);

  /* [state] */
  g_string_append(buf, "\n[state]\n");
  g_string_append_printf(buf, "history_max_size_mb = %d\n", config->history_max_size_mb);
  g_string_append_printf(buf, "history_max_days = %d\n", config->history_max_days);
  g_string_append_printf(buf, "auto_save_interval = %d\n", config->auto_save_interval);

  /* [tray] */
  g_string_append(buf, "\n[tray]\n");
  g_string_append_printf(buf, "enabled = %s\n", config->tray_enabled ? "true" : "false");
  g_string_append_printf(buf, "close_to_tray = %s\n", config->close_to_tray ? "true" : "false");
  g_string_append_printf(buf, "start_minimized = %s\n", config->start_minimized ? "true" : "false");

  /* Write atomically via temp file — INV-SECURITY-3: 0600.
   * Use g_file_set_contents followed by immediate chmod.
   * Note: g_file_set_contents uses tmp+rename internally which is atomic,
   * and we chmod immediately after to enforce 0600 regardless of umask. */
  ok = g_file_set_contents(resolved, buf->str, (gssize)buf->len, error);
  if (ok)
  {
    chmod(resolved, 0600);
    SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "config: saved to %s", resolved);
  }
  else
  {
    SK_LOG_ERROR(SK_LOG_COMPONENT_GENERAL, "config: failed to save to %s", resolved);
  }

  g_string_free(buf, TRUE);
  g_free(resolved);
  return ok;
}

/* ------------------------------------------------------------------ */
/* Generic accessors                                                   */
/* ------------------------------------------------------------------ */

int
sk_config_get_keepalive_interval(const SkConfig *config)
{
  return config != NULL ? config->ssh_keepalive_interval : 15;
}

int
sk_config_get_keepalive_max_attempts(const SkConfig *config)
{
  return config != NULL ? config->ssh_keepalive_count_max : 3;
}

const char *
sk_config_get_string(const SkConfig *config, const char *key)
{
  if (config == NULL || key == NULL)
    return NULL;

  /* Map dotted keys to struct fields */
  if (g_strcmp0(key, "general.client_id") == 0)
    return config->client_id;
  if (g_strcmp0(key, "general.theme") == 0)
    return config->theme_name;
  if (g_strcmp0(key, "terminal.font_family") == 0)
    return config->font_family;
  if (g_strcmp0(key, "terminal.word_chars") == 0)
    return config->word_chars;
  if (g_strcmp0(key, "ssh.known_hosts_file") == 0)
    return config->ssh_known_hosts_file;
  if (g_strcmp0(key, "ssh.identity_file") == 0)
    return config->ssh_identity_file;

  /* Keybindings */
  if (g_strcmp0(key, "keybindings.new_tab") == 0)
    return config->kb_new_tab;
  if (g_strcmp0(key, "keybindings.close_tab") == 0)
    return config->kb_close_tab;
  if (g_strcmp0(key, "keybindings.new_window") == 0)
    return config->kb_new_window;
  if (g_strcmp0(key, "keybindings.rename_tab") == 0)
    return config->kb_rename_tab;
  if (g_strcmp0(key, "keybindings.find") == 0)
    return config->kb_find;
  if (g_strcmp0(key, "keybindings.next_tab") == 0)
    return config->kb_next_tab;
  if (g_strcmp0(key, "keybindings.prev_tab") == 0)
    return config->kb_prev_tab;
  if (g_strcmp0(key, "keybindings.copy") == 0)
    return config->kb_copy;
  if (g_strcmp0(key, "keybindings.paste") == 0)
    return config->kb_paste;
  if (g_strcmp0(key, "keybindings.copy_all") == 0)
    return config->kb_copy_all;
  if (g_strcmp0(key, "keybindings.export_scrollback") == 0)
    return config->kb_export_scrollback;
  if (g_strcmp0(key, "keybindings.zoom_in") == 0)
    return config->kb_zoom_in;
  if (g_strcmp0(key, "keybindings.zoom_out") == 0)
    return config->kb_zoom_out;
  if (g_strcmp0(key, "keybindings.zoom_reset") == 0)
    return config->kb_zoom_reset;

  return NULL;
}

int
sk_config_get_int(const SkConfig *config, const char *key, int def)
{
  if (config == NULL || key == NULL)
    return def;

  if (g_strcmp0(key, "terminal.font_size") == 0)
    return config->font_size;
  if (g_strcmp0(key, "terminal.scrollback_lines") == 0)
    return config->scrollback_lines;
  if (g_strcmp0(key, "ssh.connect_timeout") == 0)
    return config->ssh_connect_timeout;
  if (g_strcmp0(key, "ssh.keepalive_interval") == 0)
    return config->ssh_keepalive_interval;
  if (g_strcmp0(key, "ssh.keepalive_count_max") == 0)
    return config->ssh_keepalive_count_max;
  if (g_strcmp0(key, "ssh.reconnect_max_attempts") == 0)
    return config->ssh_reconnect_max_attempts;
  if (g_strcmp0(key, "state.history_max_size_mb") == 0)
    return config->history_max_size_mb;
  if (g_strcmp0(key, "state.history_max_days") == 0)
    return config->history_max_days;
  if (g_strcmp0(key, "state.auto_save_interval") == 0)
    return config->auto_save_interval;

  return def;
}

bool
sk_config_get_bool(const SkConfig *config, const char *key, bool def)
{
  if (config == NULL || key == NULL)
    return def;

  if (g_strcmp0(key, "terminal.bold_is_bright") == 0)
    return config->bold_is_bright;
  if (g_strcmp0(key, "terminal.allow_hyperlinks") == 0)
    return config->allow_hyperlinks;
  if (g_strcmp0(key, "ssh.use_ssh_config") == 0)
    return config->ssh_use_ssh_config;
  if (g_strcmp0(key, "tray.enabled") == 0)
    return config->tray_enabled;
  if (g_strcmp0(key, "tray.close_to_tray") == 0)
    return config->close_to_tray;
  if (g_strcmp0(key, "tray.start_minimized") == 0)
    return config->start_minimized;

  return def;
}
