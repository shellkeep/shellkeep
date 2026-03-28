// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_log.c
 * @brief Structured async logger with ring buffer, file rotation,
 *        env-var control, and optional journald output.
 *
 * NFR-OBS-01 .. NFR-OBS-08, NFR-OBS-14, FR-CLI-04
 */

#ifndef _GNU_SOURCE
#define _GNU_SOURCE /* for prctl, strdup */
#endif

#include "shellkeep/sk_log.h"

#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdarg.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifndef _WIN32
#include <strings.h>
#else
#define strcasecmp _stricmp
#endif
#include <sys/stat.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

#ifdef _WIN32
#define fchmod(fd, mode) ((void)(fd), (void)(mode), 0)
#endif

#ifdef HAVE_SYSTEMD
#include <syslog.h>
#include <systemd/sd-journal.h>
#endif

/* ================================================================== */
/* Constants                                                          */
/* ================================================================== */

#define SK_LOG_MAX_MSG_LEN 4096
#define SK_LOG_RING_SIZE 1024 /* power of 2 — number of slots    */
#define SK_LOG_RING_MASK (SK_LOG_RING_SIZE - 1)
#define SK_LOG_FLUSH_INTERVAL_NS (1000000000LL) /* 1 second — NFR-OBS-05   */
#define SK_LOG_FLUSH_BYTES (64 * 1024)          /* 64 KB    — NFR-OBS-05   */
#define SK_LOG_MAX_FILE_SIZE (10 * 1024 * 1024) /* 10 MB — NFR-OBS-04 */
#define SK_LOG_MAX_ROTATED_FILES 5              /* NFR-OBS-04         */
#define SK_LOG_TIMESTAMP_LEN 40

/* ================================================================== */
/* Ring-buffer entry                                                  */
/* ================================================================== */

typedef struct
{
  char message[SK_LOG_MAX_MSG_LEN];
  SkLogLevel level;
  SkLogComponent component;
  size_t len; /* actual formatted length, excl. NUL */
} SkLogEntry;

/* ================================================================== */
/* Module-level state                                                 */
/* ================================================================== */

static struct
{
  /* Ring buffer — NFR-OBS-05 */
  SkLogEntry ring[SK_LOG_RING_SIZE];
  _Atomic uint64_t write_head; /* next slot to write into */
  _Atomic uint64_t read_head;  /* next slot to consume    */

  /* Writer thread */
  pthread_t writer_thread;
  _Atomic bool running;
  pthread_mutex_t wake_mutex;
  pthread_cond_t wake_cond;

  /* Configuration */
  SkLogLevel min_level;
  bool component_filter[SK_LOG_COMPONENT_COUNT];
  bool component_filter_active; /* false = all enabled */
  bool stderr_enabled;          /* duplicate to stderr */
  bool journald_enabled;

  /* File output — NFR-OBS-02 */
  char *log_file_path;
  FILE *log_fp;
  size_t bytes_written;

  /* Initialisation guard */
  bool initialised;
} s_sk_log;

/* ================================================================== */
/* String ↔ enum helpers                                              */
/* ================================================================== */

static const char *s_level_names[] = {
  "ERROR", "WARN", "INFO", "DEBUG", "TRACE",
};

static const char *s_component_names[] = {
  "ssh", "terminal", "state", "ui", "tmux", "sftp", "general",
};

const char *
sk_log_level_to_string(SkLogLevel level)
{
  if (level >= 0 && level <= SK_LOG_LEVEL_TRACE)
    return s_level_names[level];
  return "UNKNOWN";
}

SkLogLevel
sk_log_level_from_string(const char *str)
{
  if (!str)
    return SK_LOG_LEVEL_INFO;
  if (strcasecmp(str, "error") == 0)
    return SK_LOG_LEVEL_ERROR;
  if (strcasecmp(str, "warn") == 0)
    return SK_LOG_LEVEL_WARN;
  if (strcasecmp(str, "info") == 0)
    return SK_LOG_LEVEL_INFO;
  if (strcasecmp(str, "debug") == 0)
    return SK_LOG_LEVEL_DEBUG;
  if (strcasecmp(str, "trace") == 0)
    return SK_LOG_LEVEL_TRACE;
  return SK_LOG_LEVEL_INFO;
}

const char *
sk_log_component_to_string(SkLogComponent comp)
{
  if (comp >= 0 && comp < SK_LOG_COMPONENT_COUNT)
    return s_component_names[comp];
  return "unknown";
}

SkLogComponent
sk_log_component_from_string(const char *str)
{
  if (!str)
    return SK_LOG_COMPONENT_GENERAL;
  if (strcasecmp(str, "ssh") == 0)
    return SK_LOG_COMPONENT_SSH;
  if (strcasecmp(str, "terminal") == 0)
    return SK_LOG_COMPONENT_TERMINAL;
  if (strcasecmp(str, "state") == 0)
    return SK_LOG_COMPONENT_STATE;
  if (strcasecmp(str, "ui") == 0)
    return SK_LOG_COMPONENT_UI;
  if (strcasecmp(str, "tmux") == 0)
    return SK_LOG_COMPONENT_TMUX;
  if (strcasecmp(str, "sftp") == 0)
    return SK_LOG_COMPONENT_SFTP;
  return SK_LOG_COMPONENT_GENERAL;
}

SkLogLevel
sk_log_get_level(void)
{
  return s_sk_log.min_level;
}

void
sk_log_set_level(SkLogLevel level)
{
  s_sk_log.min_level = level;
}

/* ================================================================== */
/* XDG path helpers (minimal — avoid pulling state-layer dependency)   */
/* ================================================================== */

/**
 * Return $XDG_STATE_HOME/shellkeep  (default: ~/.local/state/shellkeep).
 * Caller must free().
 */
static char *
sk_log_state_dir(void)
{
  const char *xdg = getenv("XDG_STATE_HOME");
  char *path = NULL;

  if (xdg && xdg[0] != '\0')
  {
    if (asprintf(&path, "%s/shellkeep", xdg) < 0)
      return NULL;
  }
  else
  {
    const char *home = getenv("HOME");
    if (!home)
      home = "/tmp";
    if (asprintf(&path, "%s/.local/state/shellkeep", home) < 0)
      return NULL;
  }
  return path;
}

/**
 * Recursively create directory with mode 0700.
 */
static int
sk_mkdir_p(const char *path, mode_t mode)
{
  char *tmp = strdup(path);
  if (!tmp)
    return -1;

  for (char *p = tmp + 1; *p; p++)
  {
    if (*p == '/')
    {
      *p = '\0';
      if (mkdir(tmp, mode) != 0 && errno != EEXIST)
      {
        free(tmp);
        return -1;
      }
      *p = '/';
    }
  }
  int rc = mkdir(tmp, mode);
  free(tmp);
  return (rc == 0 || errno == EEXIST) ? 0 : -1;
}

/* ================================================================== */
/* File rotation — NFR-OBS-04                                         */
/* ================================================================== */

static void
sk_log_rotate(void)
{
  if (!s_sk_log.log_file_path)
    return;

  if (s_sk_log.log_fp)
  {
    fclose(s_sk_log.log_fp);
    s_sk_log.log_fp = NULL;
  }

  /* Rotate existing files: .5 → delete, .4 → .5, … , .1 → .2, current → .1 */
  char old_path[4096];
  char new_path[4096];

  /* Delete oldest if it exists */
  snprintf(old_path, sizeof(old_path), "%s.%d", s_sk_log.log_file_path, SK_LOG_MAX_ROTATED_FILES);
  (void)unlink(old_path);

  for (int i = SK_LOG_MAX_ROTATED_FILES - 1; i >= 1; i--)
  {
    snprintf(old_path, sizeof(old_path), "%s.%d", s_sk_log.log_file_path, i);
    snprintf(new_path, sizeof(new_path), "%s.%d", s_sk_log.log_file_path, i + 1);
    (void)rename(old_path, new_path);
  }

  snprintf(new_path, sizeof(new_path), "%s.1", s_sk_log.log_file_path);
  (void)rename(s_sk_log.log_file_path, new_path);

  /* Open fresh log file — 0600 per INV-SECURITY-3 */
  int fd = open(s_sk_log.log_file_path, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
  if (fd >= 0)
  {
    s_sk_log.log_fp = fdopen(fd, "w");
  }
  s_sk_log.bytes_written = 0;
}

/* ================================================================== */
/* Writer thread — NFR-OBS-05                                         */
/* ================================================================== */

#ifdef HAVE_SYSTEMD
static int
sk_log_level_to_journal_priority(SkLogLevel level)
{
  /* NFR-OBS-14 */
  switch (level)
  {
  case SK_LOG_LEVEL_ERROR:
    return LOG_ERR;
  case SK_LOG_LEVEL_WARN:
    return LOG_WARNING;
  case SK_LOG_LEVEL_INFO:
    return LOG_INFO;
  case SK_LOG_LEVEL_DEBUG:
    return LOG_DEBUG;
  case SK_LOG_LEVEL_TRACE:
    return LOG_DEBUG;
  default:
    return LOG_INFO;
  }
}
#endif

static void
sk_log_flush_entry(const SkLogEntry *entry)
{
  /* Write to log file */
  if (s_sk_log.log_fp)
  {
    size_t written = fwrite(entry->message, 1, entry->len, s_sk_log.log_fp);
    s_sk_log.bytes_written += written;

    /* Check if rotation needed */
    if (s_sk_log.bytes_written >= SK_LOG_MAX_FILE_SIZE)
    {
      sk_log_rotate();
    }
  }

  /* Write to stderr if debug/trace mode */
  if (s_sk_log.stderr_enabled)
  {
    (void)fwrite(entry->message, 1, entry->len, stderr);
  }

#ifdef HAVE_SYSTEMD
  /* NFR-OBS-14: optional journald output */
  if (s_sk_log.journald_enabled)
  {
    /* Strip trailing newline for journald */
    char jmsg[SK_LOG_MAX_MSG_LEN];
    size_t jlen = entry->len;
    if (jlen > 0 && entry->message[jlen - 1] == '\n')
      jlen--;
    if (jlen >= sizeof(jmsg))
      jlen = sizeof(jmsg) - 1;
    memcpy(jmsg, entry->message, jlen);
    jmsg[jlen] = '\0';

    sd_journal_send("MESSAGE=%s", jmsg, "PRIORITY=%d",
                    sk_log_level_to_journal_priority(entry->level), "SYSLOG_IDENTIFIER=shellkeep",
                    "SK_COMPONENT=%s", sk_log_component_to_string(entry->component), NULL);
  }
#endif
}

static void *
sk_log_writer_func(void *arg)
{
  (void)arg;

  struct timespec flush_deadline;
  size_t bytes_since_flush = 0;

  while (atomic_load(&s_sk_log.running) ||
         atomic_load(&s_sk_log.write_head) != atomic_load(&s_sk_log.read_head))
  {
    uint64_t rh = atomic_load(&s_sk_log.read_head);
    uint64_t wh = atomic_load(&s_sk_log.write_head);

    if (rh == wh)
    {
      /* Nothing to read — wait for signal or timeout (1 second) */
      pthread_mutex_lock(&s_sk_log.wake_mutex);
      clock_gettime(CLOCK_REALTIME, &flush_deadline);
      flush_deadline.tv_sec += 1;
      pthread_cond_timedwait(&s_sk_log.wake_cond, &s_sk_log.wake_mutex, &flush_deadline);
      pthread_mutex_unlock(&s_sk_log.wake_mutex);
      continue;
    }

    /* Consume entries */
    while (rh != wh)
    {
      const SkLogEntry *entry = &s_sk_log.ring[rh & SK_LOG_RING_MASK];
      sk_log_flush_entry(entry);
      bytes_since_flush += entry->len;
      rh++;
      atomic_store(&s_sk_log.read_head, rh);

      /* Flush to disk every 64 KB — NFR-OBS-05 */
      if (bytes_since_flush >= SK_LOG_FLUSH_BYTES)
      {
        if (s_sk_log.log_fp)
          fflush(s_sk_log.log_fp);
        bytes_since_flush = 0;
      }

      wh = atomic_load(&s_sk_log.write_head);
    }

    /* Periodic flush */
    if (s_sk_log.log_fp && bytes_since_flush > 0)
    {
      fflush(s_sk_log.log_fp);
      bytes_since_flush = 0;
    }
  }

  /* Final flush */
  if (s_sk_log.log_fp)
    fflush(s_sk_log.log_fp);

  return NULL;
}

/* ================================================================== */
/* Timestamp formatting — NFR-OBS-03                                  */
/* ================================================================== */

static void
sk_log_format_timestamp(char *buf, size_t buflen)
{
  struct timespec ts;
  clock_gettime(CLOCK_REALTIME, &ts);

  struct tm tm_info;
  localtime_r(&ts.tv_sec, &tm_info);

  /* YYYY-MM-DDTHH:MM:SS.mmm+TZ */
  size_t off = strftime(buf, buflen, "%Y-%m-%dT%H:%M:%S", &tm_info);
  off += (size_t)snprintf(buf + off, buflen - off, ".%03ld", ts.tv_nsec / 1000000);
  strftime(buf + off, buflen - off, "%z", &tm_info);
}

/* ================================================================== */
/* Parse component filter string — NFR-OBS-06                         */
/* ================================================================== */

static void
sk_log_parse_component_filter(const char *str)
{
  if (!str || str[0] == '\0')
    return;

  s_sk_log.component_filter_active = true;
  memset(s_sk_log.component_filter, 0, sizeof(s_sk_log.component_filter));

  char *copy = strdup(str);
  if (!copy)
    return;

  char *saveptr = NULL;
  for (char *tok = strtok_r(copy, ",", &saveptr); tok != NULL; tok = strtok_r(NULL, ",", &saveptr))
  {
    /* Strip leading/trailing whitespace */
    while (*tok == ' ')
      tok++;
    char *end = tok + strlen(tok) - 1;
    while (end > tok && *end == ' ')
      *end-- = '\0';

    SkLogComponent c = sk_log_component_from_string(tok);
    s_sk_log.component_filter[(int)c] = true;
  }

  free(copy);
}

/* ================================================================== */
/* Public API: sk_log_write                                           */
/* ================================================================== */

void
sk_log_write(SkLogLevel level, SkLogComponent component, const char *file, int line,
             const char *fmt, ...)
{
  /* Level check */
  if (level > s_sk_log.min_level)
    return;

  /* Component filter check — NFR-OBS-06 */
  if (s_sk_log.component_filter_active && !s_sk_log.component_filter[(int)component])
    return;

  /* Format timestamp — NFR-OBS-03 */
  char ts[SK_LOG_TIMESTAMP_LEN];
  sk_log_format_timestamp(ts, sizeof(ts));

  /* Build user message */
  char user_msg[SK_LOG_MAX_MSG_LEN / 2];
  va_list ap;
  va_start(ap, fmt);
  vsnprintf(user_msg, sizeof(user_msg), fmt, ap);
  va_end(ap);

  /* Pad level string to 5 chars for alignment */
  const char *lvl_str = sk_log_level_to_string(level);

  /* Assemble final log line:
   * TIMESTAMP LEVEL [COMPONENT] message   (source:line)
   * NFR-OBS-03
   */
  uint64_t slot = atomic_fetch_add(&s_sk_log.write_head, 1);
  SkLogEntry *entry = &s_sk_log.ring[slot & SK_LOG_RING_MASK];

  entry->level = level;
  entry->component = component;
  entry->len =
      (size_t)snprintf(entry->message, sizeof(entry->message), "%s %-5s [%-8s] %s  (%s:%d)\n", ts,
                       lvl_str, sk_log_component_to_string(component), user_msg, file, line);

  if (entry->len >= sizeof(entry->message))
    entry->len = sizeof(entry->message) - 1;

  /* Wake writer thread */
  pthread_mutex_lock(&s_sk_log.wake_mutex);
  pthread_cond_signal(&s_sk_log.wake_cond);
  pthread_mutex_unlock(&s_sk_log.wake_mutex);
}

/* ================================================================== */
/* Open the log file                                                  */
/* ================================================================== */

static int
sk_log_open_file(void)
{
  /* If overridden by SHELLKEEP_LOG_FILE — NFR-OBS-06 */
  const char *env_file = getenv("SHELLKEEP_LOG_FILE");
  if (env_file && env_file[0] != '\0')
  {
    if (strcmp(env_file, "/dev/stderr") == 0)
    {
      /* No file output; stderr only */
      s_sk_log.stderr_enabled = true;
      s_sk_log.log_file_path = NULL;
      s_sk_log.log_fp = NULL;
      return 0;
    }
    s_sk_log.log_file_path = strdup(env_file);
  }
  else
  {
    /* Default: $XDG_STATE_HOME/shellkeep/logs/shellkeep.log — NFR-OBS-02 */
    char *state = sk_log_state_dir();
    if (!state)
      return -1;

    char *dir = NULL;
    if (asprintf(&dir, "%s/logs", state) < 0)
    {
      free(state);
      return -1;
    }
    free(state);

    if (sk_mkdir_p(dir, 0700) != 0)
    {
      free(dir);
      return -1;
    }

    if (asprintf(&s_sk_log.log_file_path, "%s/shellkeep.log", dir) < 0)
    {
      free(dir);
      return -1;
    }
    free(dir);
  }

  if (s_sk_log.log_file_path)
  {
    /* Open with 0600 permissions — INV-SECURITY-3 */
    int fd = open(s_sk_log.log_file_path, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0600);
    if (fd < 0)
      return -1;

    s_sk_log.log_fp = fdopen(fd, "a");
    if (!s_sk_log.log_fp)
    {
      close(fd);
      return -1;
    }

    /* Check current size for rotation */
    struct stat st;
    if (fstat(fd, &st) == 0)
      s_sk_log.bytes_written = (size_t)st.st_size;
    else
      s_sk_log.bytes_written = 0;

    /* Ensure permissions are correct */
    fchmod(fd, 0600);
  }

  return 0;
}

/* ================================================================== */
/* Public API: sk_log_init                                            */
/* ================================================================== */

int
sk_log_init(bool debug_mode, bool trace_mode, const char *debug_components)
{
  if (s_sk_log.initialised)
    return 0;

  memset(&s_sk_log, 0, sizeof(s_sk_log));
  atomic_store(&s_sk_log.write_head, 0);
  atomic_store(&s_sk_log.read_head, 0);
  atomic_store(&s_sk_log.running, true);

  /* Default level: INFO — NFR-OBS-01 */
  s_sk_log.min_level = SK_LOG_LEVEL_INFO;

  /* CLI overrides — FR-CLI-04 */
  if (trace_mode)
  {
    s_sk_log.min_level = SK_LOG_LEVEL_TRACE;
    s_sk_log.stderr_enabled = true;
  }
  else if (debug_mode)
  {
    s_sk_log.min_level = SK_LOG_LEVEL_DEBUG;
    s_sk_log.stderr_enabled = true;
  }

  /* Env var overrides — NFR-OBS-06 */
  const char *env_level = getenv("SHELLKEEP_LOG_LEVEL");
  if (env_level && env_level[0] != '\0')
    s_sk_log.min_level = sk_log_level_from_string(env_level);

  /* Component filter from CLI --debug=ssh,tmux or from env var */
  if (debug_mode && debug_components)
    sk_log_parse_component_filter(debug_components);

  const char *env_comp = getenv("SHELLKEEP_LOG_COMPONENT");
  if (env_comp && env_comp[0] != '\0')
    sk_log_parse_component_filter(env_comp);

    /* journald — NFR-OBS-14 */
#ifdef HAVE_SYSTEMD
  const char *env_journald = getenv("SHELLKEEP_LOG_JOURNALD");
  if (env_journald && strcmp(env_journald, "1") == 0)
    s_sk_log.journald_enabled = true;
#endif

  /* Open log file */
  if (sk_log_open_file() != 0)
  {
    /* Fall back to stderr only */
    s_sk_log.stderr_enabled = true;
  }

  /* Start writer thread — NFR-OBS-05 */
  pthread_mutex_init(&s_sk_log.wake_mutex, NULL);
  pthread_cond_init(&s_sk_log.wake_cond, NULL);

  if (pthread_create(&s_sk_log.writer_thread, NULL, sk_log_writer_func, NULL) != 0)
  {
    if (s_sk_log.log_fp)
      fclose(s_sk_log.log_fp);
    return -1;
  }

  /* Install crash handlers — NFR-OBS-09 */
  sk_crash_handler_install();

  s_sk_log.initialised = true;

  SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "logging initialised level=%s",
              sk_log_level_to_string(s_sk_log.min_level));

  return 0;
}

/* ================================================================== */
/* Public API: sk_log_shutdown                                        */
/* ================================================================== */

void
sk_log_shutdown(void)
{
  if (!s_sk_log.initialised)
    return;

  SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "logging shutdown");

  /* Signal writer thread to stop */
  atomic_store(&s_sk_log.running, false);

  pthread_mutex_lock(&s_sk_log.wake_mutex);
  pthread_cond_signal(&s_sk_log.wake_cond);
  pthread_mutex_unlock(&s_sk_log.wake_mutex);

  pthread_join(s_sk_log.writer_thread, NULL);
  pthread_mutex_destroy(&s_sk_log.wake_mutex);
  pthread_cond_destroy(&s_sk_log.wake_cond);

  if (s_sk_log.log_fp)
  {
    fclose(s_sk_log.log_fp);
    s_sk_log.log_fp = NULL;
  }

  free(s_sk_log.log_file_path);
  s_sk_log.log_file_path = NULL;
  s_sk_log.initialised = false;
}
