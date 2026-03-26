// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_config.h
 * @brief Configuration management — public API.
 *
 * Loads, validates, and provides access to shellkeep configuration
 * stored in $XDG_CONFIG_HOME/shellkeep/config.ini (FR-CONFIG-01).
 *
 * The application does NOT create this file automatically. All values
 * have internal defaults. The file is purely for user overrides.
 */

#ifndef SK_CONFIG_H
#define SK_CONFIG_H

#include "shellkeep/sk_types.h"

#include <glib.h>

#ifdef __cplusplus
extern "C"
{
#endif

  /* ------------------------------------------------------------------ */
  /* Theme — terminal color scheme                                       */
  /* ------------------------------------------------------------------ */

  /** Terminal color theme (16 ANSI + fg/bg/cursor). FR-CONFIG-02 */
  typedef struct _SkTheme
  {
    char *name;               /**< Theme name (from filename). */
    uint32_t ansi_colors[16]; /**< ANSI colors 0-15 as 0xRRGGBB. */
    uint32_t foreground;      /**< Default foreground 0xRRGGBB. */
    uint32_t background;      /**< Default background 0xRRGGBB. */
    uint32_t cursor_color;    /**< Cursor color 0xRRGGBB. */
    bool has_cursor_color;    /**< Whether cursor color is set. */
  } SkTheme;

  /* ------------------------------------------------------------------ */
  /* Enums for configuration values                                      */
  /* ------------------------------------------------------------------ */

  /** Startup behavior — FR-CONFIG-02 [general] */
  typedef enum
  {
    SK_STARTUP_LAST_SESSION = 0,
    SK_STARTUP_WELCOME_SCREEN,
    SK_STARTUP_MINIMIZED,
  } SkStartupBehavior;

  /** Bell mode — FR-CONFIG-05 [terminal] */
  typedef enum
  {
    SK_BELL_VISUAL = 0,
    SK_BELL_AUDIBLE,
    SK_BELL_NONE,
  } SkBellMode;

  /** Cursor shape — FR-CONFIG-05 [terminal] */
  typedef enum
  {
    SK_CURSOR_BLOCK = 0,
    SK_CURSOR_IBEAM,
    SK_CURSOR_UNDERLINE,
  } SkCursorShape;

  /** Cursor blink — FR-CONFIG-05 [terminal] */
  typedef enum
  {
    SK_CURSOR_BLINK_SYSTEM = 0,
    SK_CURSOR_BLINK_ON,
    SK_CURSOR_BLINK_OFF,
  } SkCursorBlink;

  /* ------------------------------------------------------------------ */
  /* SkConfig — FR-CONFIG-02: all configuration sections                 */
  /* ------------------------------------------------------------------ */

  /** Full configuration store. Opaque in sk_types.h; defined here. */
  struct _SkConfig
  {
    /* --- [general] --- */
    char *client_id;  /**< FR-CONFIG-08, FR-CLI-02 */
    char *theme_name; /**< "system", "dark", "light", or filename */
    SkStartupBehavior startup_behavior;

    /* --- [terminal] FR-CONFIG-05 --- */
    char *font_family;
    int font_size;        /**< 6-72 */
    int scrollback_lines; /**< 0=unlimited, max 1000000 */
    SkBellMode bell;
    SkCursorShape cursor_shape;
    SkCursorBlink cursor_blink;
    char *word_chars;
    bool bold_is_bright;
    bool allow_hyperlinks;

    /* --- [ssh] FR-CONFIG-06 --- */
    int ssh_connect_timeout;     /**< 1-300 seconds */
    int ssh_keepalive_interval;  /**< 0-600 seconds (0=disabled) */
    int ssh_keepalive_count_max; /**< 1-30 */
    char *ssh_known_hosts_file;  /**< empty = libssh default */
    char *ssh_identity_file;     /**< empty = ssh-agent/default */
    bool ssh_use_ssh_config;
    int ssh_reconnect_max_attempts;    /**< 0=infinite, max 100 */
    double ssh_reconnect_backoff_base; /**< 0.5-30.0 seconds */

    /* --- [keybindings] FR-CONFIG-02 --- */
    char *kb_new_tab;
    char *kb_close_tab;
    char *kb_new_window;
    char *kb_rename_tab;
    char *kb_find;
    char *kb_next_tab;
    char *kb_prev_tab;
    char *kb_copy;
    char *kb_paste;
    char *kb_copy_all;
    char *kb_export_scrollback;
    char *kb_zoom_in;
    char *kb_zoom_out;
    char *kb_zoom_reset;

    /* --- [state] FR-CONFIG-02 --- */
    int history_max_size_mb; /**< max MB per session history */
    int history_max_days;    /**< purge older than N days */
    int auto_save_interval;  /**< seconds, 5-600 */

    /* --- [tray] FR-CONFIG-02 --- */
    bool tray_enabled;
    bool close_to_tray;
    bool start_minimized;

    /* --- Internal bookkeeping --- */
    char *config_path; /**< Resolved path to config.ini. */
    SkTheme *theme;    /**< Currently loaded theme (or NULL). */
  };

  /* ------------------------------------------------------------------ */
  /* Config lifecycle                                                    */
  /* ------------------------------------------------------------------ */

  /**
   * Load configuration from an INI file, or use defaults if missing.
   * FR-CONFIG-01, FR-CONFIG-03
   *
   * @param path   Path to config file, or NULL for default XDG location.
   * @param error  Return location for error.
   * @return Config object (always non-NULL on success — defaults on parse error).
   *         NULL only on allocation failure.
   */
  SkConfig *sk_config_load(const char *path, GError **error);

  /**
   * Save current configuration to an INI file.
   * @param config  Config object.
   * @param path    Destination path, or NULL for default.
   * @param error   Return location for error.
   * @return true on success.
   */
  bool sk_config_save(const SkConfig *config, const char *path, GError **error);

  /**
   * Create a new config with all internal default values.
   * FR-CONFIG-05, FR-CONFIG-06
   */
  SkConfig *sk_config_new_defaults(void);

  /**
   * Free config object and all owned strings.
   */
  void sk_config_free(SkConfig *config);

  /* ------------------------------------------------------------------ */
  /* Accessors — NFR-ARCH-08                                             */
  /* ------------------------------------------------------------------ */

  int sk_config_get_keepalive_interval(const SkConfig *config);
  int sk_config_get_keepalive_max_attempts(const SkConfig *config);

  /**
   * Get a string setting by dotted key (e.g. "terminal.font_family").
   * @return Internal string — do NOT free.  NULL if key not found.
   */
  const char *sk_config_get_string(const SkConfig *config, const char *key);

  /**
   * Get an integer setting by dotted key.
   * @return Value, or @p def if key not found.
   */
  int sk_config_get_int(const SkConfig *config, const char *key, int def);

  /**
   * Get a boolean setting by dotted key.
   */
  bool sk_config_get_bool(const SkConfig *config, const char *key, bool def);

  /* ------------------------------------------------------------------ */
  /* Client-ID — FR-CONFIG-08, FR-CLI-02                                 */
  /* ------------------------------------------------------------------ */

  /**
   * Resolve client-id: from config, from file, or generate new UUID v4.
   * Saves generated ID to $XDG_CONFIG_HOME/shellkeep/client-id.
   *
   * @param config  Config object (may have client_id set from config file).
   * @param error   Return location for error.
   * @return Newly-allocated client-id string, or NULL on error.
   */
  char *sk_config_resolve_client_id(const SkConfig *config, GError **error);

  /**
   * Validate a client-id string: [a-zA-Z0-9_-], max 64 chars.
   * @return true if valid.
   */
  bool sk_config_validate_client_id(const char *id);

  /* ------------------------------------------------------------------ */
  /* Hot reload — FR-CONFIG-04                                           */
  /* ------------------------------------------------------------------ */

  /** Callback invoked after config is hot-reloaded. */
  typedef void (*SkConfigReloadCallback)(SkConfig *new_config, void *user_data);

  /** Opaque handle for the config file watcher. */
  typedef struct _SkConfigWatcher SkConfigWatcher;

  /**
   * Start watching config file for changes via inotify.
   * Debounces events by 500ms. Only hot-reloadable settings are applied.
   *
   * @param config_path  Path to config.ini (or NULL for default).
   * @param callback     Function to call with new config on reload.
   * @param user_data    Opaque data passed to callback.
   * @param error        Return location for error.
   * @return Watcher handle, or NULL on error.
   */
  SkConfigWatcher *sk_config_watch_start(const char *config_path, SkConfigReloadCallback callback,
                                         void *user_data, GError **error);

  /**
   * Stop watching and free watcher resources.
   */
  void sk_config_watch_stop(SkConfigWatcher *watcher);

  /* ------------------------------------------------------------------ */
  /* Terminal themes                                                     */
  /* ------------------------------------------------------------------ */

  /**
   * Load a theme from $XDG_CONFIG_HOME/shellkeep/themes/<name>.json.
   * Compatible with Gogh/base16 JSON format.
   *
   * @param name   Theme name (filename without .json).
   * @param error  Return location for error.
   * @return Theme object, or NULL on error.
   */
  SkTheme *sk_theme_load(const char *name, GError **error);

  /**
   * Create the built-in default theme (matching common terminal defaults).
   */
  SkTheme *sk_theme_new_default(void);

  /**
   * Free a theme object.
   */
  void sk_theme_free(SkTheme *theme);

  /**
   * Get the default XDG config directory for shellkeep.
   * @return Newly-allocated path string. Caller must free.
   */
  char *sk_config_get_dir(void);

#ifdef __cplusplus
}
#endif

#endif /* SK_CONFIG_H */
