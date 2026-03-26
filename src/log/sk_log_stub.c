// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_log_stub.c
 * @brief Stub implementation of the logging API.
 *
 * Provides minimal functional stubs so that main.c and other early code
 * can compile and link before the full logging layer is implemented.
 *
 * TODO: replace with full implementation when logging layer is ready.
 */

#include "shellkeep/sk_log.h"

#include <stdarg.h>
#include <stdio.h>
#include <string.h>
#include <time.h>

static SkLogLevel current_level = SK_LOG_LEVEL_INFO;

/* clang-format off */
static const char *level_names[] = {
  [SK_LOG_LEVEL_ERROR] = "ERROR",
  [SK_LOG_LEVEL_WARN]  = "WARN",
  [SK_LOG_LEVEL_INFO]  = "INFO",
  [SK_LOG_LEVEL_DEBUG] = "DEBUG",
  [SK_LOG_LEVEL_TRACE] = "TRACE",
};

static const char *component_names[] = {
  [SK_LOG_COMPONENT_SSH]      = "ssh",
  [SK_LOG_COMPONENT_TERMINAL] = "terminal",
  [SK_LOG_COMPONENT_STATE]    = "state",
  [SK_LOG_COMPONENT_UI]       = "ui",
  [SK_LOG_COMPONENT_TMUX]     = "tmux",
  [SK_LOG_COMPONENT_SFTP]     = "sftp",
  [SK_LOG_COMPONENT_GENERAL]  = "general",
};
/* clang-format on */

int
sk_log_init(bool debug_mode, bool trace_mode, const char *debug_components G_GNUC_UNUSED)
{
  const char *env_level;
  SkLogLevel lvl;

  if (trace_mode)
    current_level = SK_LOG_LEVEL_TRACE;
  else if (debug_mode)
    current_level = SK_LOG_LEVEL_DEBUG;

  /* Check env var override */
  env_level = g_getenv("SHELLKEEP_LOG_LEVEL");
  if (env_level != NULL)
  {
    lvl = sk_log_level_from_string(env_level);
    if ((int)lvl >= 0)
      current_level = lvl;
  }

  return 0;
}

void
sk_log_shutdown(void)
{
  /* Stub: nothing to flush */
}

void
sk_log_write(SkLogLevel level, SkLogComponent component, const char *file, int line,
             const char *fmt, ...)
{
  time_t now;
  struct tm tm_buf;
  char ts[32];
  va_list ap;

  if (level > current_level)
    return;

  /* Timestamp */
  now = time(NULL);
  localtime_r(&now, &tm_buf);
  strftime(ts, sizeof(ts), "%Y-%m-%dT%H:%M:%S", &tm_buf);

  /* Format: TIMESTAMP LEVEL [COMPONENT] file:line message */
  fprintf(stderr, "%s %-5s [%-8s] %s:%d ", ts, level_names[level], component_names[component], file,
          line);

  va_start(ap, fmt);
  vfprintf(stderr, fmt, ap);
  va_end(ap);

  fputc('\n', stderr);
}

void
sk_crash_handler_install(void)
{
  /* TODO: implement signal handlers when logging layer is ready */
}

bool
sk_crash_has_previous_dumps(void)
{
  /* TODO: implement when logging layer is ready */
  return false;
}

char *
sk_crash_get_dir(void)
{
  /* TODO: implement when logging layer is ready */
  return g_strdup("/tmp/shellkeep-crashes");
}

SkLogLevel
sk_log_get_level(void)
{
  return current_level;
}

void
sk_log_set_level(SkLogLevel level)
{
  current_level = level;
}

const char *
sk_log_level_to_string(SkLogLevel level)
{
  if (level >= 0 && level <= SK_LOG_LEVEL_TRACE)
    return level_names[level];
  return "UNKNOWN";
}

SkLogLevel
sk_log_level_from_string(const char *str)
{
  if (str == NULL)
    return SK_LOG_LEVEL_INFO;

  for (int i = 0; i <= SK_LOG_LEVEL_TRACE; i++)
  {
    if (g_ascii_strcasecmp(str, level_names[i]) == 0)
      return (SkLogLevel)i;
  }
  return SK_LOG_LEVEL_INFO;
}

const char *
sk_log_component_to_string(SkLogComponent comp)
{
  if (comp >= 0 && comp < SK_LOG_COMPONENT_COUNT)
    return component_names[comp];
  return "unknown";
}

SkLogComponent
sk_log_component_from_string(const char *str)
{
  if (str == NULL)
    return SK_LOG_COMPONENT_GENERAL;

  for (int i = 0; i < SK_LOG_COMPONENT_COUNT; i++)
  {
    if (g_ascii_strcasecmp(str, component_names[i]) == 0)
      return (SkLogComponent)i;
  }
  return SK_LOG_COMPONENT_GENERAL;
}
