// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_log.h
 * @brief ShellKeep logging subsystem — public API.
 *
 * Provides structured logging with five severity levels, async ring-buffer
 * writing, file rotation, crash handling, optional journald output, and
 * environment-variable / CLI overrides.
 *
 * ## Prohibited data (NFR-OBS-08)
 * Logs must NEVER contain:
 *   - Terminal content (input or output)
 *   - Private keys, passphrases, passwords
 *   - Environment variables (may contain secrets)
 *   - SFTP file content
 *   - Clipboard content
 *   - Scrollback / history JSONL content
 * Violation of this policy is treated as a **security bug**.
 */

#ifndef SK_LOG_H
#define SK_LOG_H

#include <glib.h>

#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C"
{
#endif

  /* ------------------------------------------------------------------ */
  /* Log levels — NFR-OBS-01                                            */
  /* ------------------------------------------------------------------ */

  typedef enum
  {
    SK_LOG_LEVEL_ERROR = 0,
    SK_LOG_LEVEL_WARN = 1,
    SK_LOG_LEVEL_INFO = 2, /* default */
    SK_LOG_LEVEL_DEBUG = 3,
    SK_LOG_LEVEL_TRACE = 4,
  } SkLogLevel;

  /* ------------------------------------------------------------------ */
  /* Components — NFR-OBS-06                                            */
  /* ------------------------------------------------------------------ */

  typedef enum
  {
    SK_LOG_COMPONENT_SSH = 0,
    SK_LOG_COMPONENT_TERMINAL = 1,
    SK_LOG_COMPONENT_STATE = 2,
    SK_LOG_COMPONENT_UI = 3,
    SK_LOG_COMPONENT_TMUX = 4,
    SK_LOG_COMPONENT_SFTP = 5,
    SK_LOG_COMPONENT_GENERAL = 6, /* catch-all */
    SK_LOG_COMPONENT_COUNT = 7,
  } SkLogComponent;

  /* ------------------------------------------------------------------ */
  /* Initialisation / shutdown                                          */
  /* ------------------------------------------------------------------ */

  /**
   * Initialise the logging subsystem.
   *
   * Reads env vars: SHELLKEEP_LOG_LEVEL, SHELLKEEP_LOG_COMPONENT,
   *                 SHELLKEEP_LOG_FILE, SHELLKEEP_LOG_JOURNALD.
   *
   * Spawns the async writer thread and installs crash-signal handlers.
   *
   * @param debug_mode   true if --debug was given on CLI (sets DEBUG level + stderr)
   * @param trace_mode   true if --trace was given on CLI (sets TRACE level + stderr)
   * @param debug_components  comma-separated component list (e.g. "ssh,tmux"),
   *                          or NULL for all.  Only meaningful when debug_mode is true.
   * @return 0 on success, -1 on failure.
   */
  int sk_log_init(bool debug_mode, bool trace_mode, const char *debug_components);

  /**
   * Flush pending log entries and shut down the writer thread.
   * Safe to call multiple times.
   */
  void sk_log_shutdown(void);

  /* ------------------------------------------------------------------ */
  /* Core logging function (prefer macros below)                        */
  /* ------------------------------------------------------------------ */

  void sk_log_write(SkLogLevel level, SkLogComponent component, const char *file, int line,
                    const char *fmt, ...) __attribute__((format(printf, 5, 6)));

  /* ------------------------------------------------------------------ */
  /* Convenience macros — NFR-OBS-01, NFR-OBS-03                        */
  /* ------------------------------------------------------------------ */

#define SK_LOG_ERROR(comp, ...)                                                                    \
  sk_log_write(SK_LOG_LEVEL_ERROR, (comp), __FILE__, __LINE__, __VA_ARGS__)

#define SK_LOG_WARN(comp, ...)                                                                     \
  sk_log_write(SK_LOG_LEVEL_WARN, (comp), __FILE__, __LINE__, __VA_ARGS__)

#define SK_LOG_INFO(comp, ...)                                                                     \
  sk_log_write(SK_LOG_LEVEL_INFO, (comp), __FILE__, __LINE__, __VA_ARGS__)

#define SK_LOG_DEBUG(comp, ...)                                                                    \
  sk_log_write(SK_LOG_LEVEL_DEBUG, (comp), __FILE__, __LINE__, __VA_ARGS__)

#define SK_LOG_TRACE(comp, ...)                                                                    \
  sk_log_write(SK_LOG_LEVEL_TRACE, (comp), __FILE__, __LINE__, __VA_ARGS__)

  /* ------------------------------------------------------------------ */
  /* Crash handling — NFR-OBS-09, NFR-OBS-10                            */
  /* ------------------------------------------------------------------ */

  /**
   * Install signal handlers for SIGSEGV, SIGABRT, SIGBUS, SIGFPE.
   * Called automatically by sk_log_init().
   */
  void sk_crash_handler_install(void);

  /**
   * Check for crash dump files from a previous run.
   * @return true if at least one crash dump exists.
   */
  bool sk_crash_has_previous_dumps(void);

  /**
   * Get the path to the crash directory.
   * Caller must free the returned string.
   */
  char *sk_crash_get_dir(void);

  /* ------------------------------------------------------------------ */
  /* Query helpers                                                      */
  /* ------------------------------------------------------------------ */

  SkLogLevel sk_log_get_level(void);
  void sk_log_set_level(SkLogLevel level);
  const char *sk_log_level_to_string(SkLogLevel level);
  SkLogLevel sk_log_level_from_string(const char *str);
  const char *sk_log_component_to_string(SkLogComponent comp);
  SkLogComponent sk_log_component_from_string(const char *str);

#ifdef __cplusplus
}
#endif

#endif /* SK_LOG_H */
