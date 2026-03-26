// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_crash.c
 * @brief Crash signal handler — backtrace to dump file.
 *
 * NFR-OBS-09: Signal handler for SIGSEGV, SIGABRT, SIGBUS, SIGFPE.
 * NFR-OBS-10: Crash dumps never include terminal content, keys, or env vars.
 * NFR-OBS-13: No telemetry — crash reporting is local-only.
 */

#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif

#include "shellkeep/sk_log.h"

#include <dirent.h>
#include <errno.h>
#include <execinfo.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

/* ================================================================== */
/* Constants                                                          */
/* ================================================================== */

#define SK_CRASH_MAX_FRAMES 64
#define SK_CRASH_DIR_MAXLEN 256

/* Pre-computed crash directory (set during sk_crash_handler_install) */
static char s_crash_dir[SK_CRASH_DIR_MAXLEN];

/* ================================================================== */
/* Crash directory path                                               */
/* ================================================================== */

/**
 * Build the crash directory path:
 *   $XDG_STATE_HOME/shellkeep/crashes/
 * Caller must free().
 */
static char *
sk_crash_build_dir(void)
{
  const char *xdg = getenv("XDG_STATE_HOME");
  char *path = NULL;

  if (xdg && xdg[0] != '\0')
  {
    if (asprintf(&path, "%s/shellkeep/crashes", xdg) < 0)
      return NULL;
  }
  else
  {
    const char *home = getenv("HOME");
    if (!home)
      home = "/tmp";
    if (asprintf(&path, "%s/.local/state/shellkeep/crashes", home) < 0)
      return NULL;
  }
  return path;
}

char *
sk_crash_get_dir(void)
{
  return sk_crash_build_dir();
}

/* ================================================================== */
/* Async-signal-safe helpers                                          */
/* ================================================================== */

/**
 * Write a string to an fd — async-signal-safe.
 */
static void
safe_write(int fd, const char *s)
{
  if (!s)
    return;
  size_t len = 0;
  while (s[len] != '\0')
    len++;
  (void)write(fd, s, len);
}

/**
 * Convert an integer to a decimal string — async-signal-safe.
 */
static void
int_to_str(int val, char *buf, size_t buflen)
{
  if (buflen == 0)
    return;
  buf[buflen - 1] = '\0';

  int negative = 0;
  unsigned int uval;
  if (val < 0)
  {
    negative = 1;
    uval = (unsigned int)(-val);
  }
  else
  {
    uval = (unsigned int)val;
  }

  size_t pos = buflen - 1;
  do
  {
    if (pos == 0)
      break;
    pos--;
    buf[pos] = (char)('0' + (uval % 10));
    uval /= 10;
  } while (uval > 0);

  if (negative && pos > 0)
  {
    pos--;
    buf[pos] = '-';
  }

  /* Shift to start of buffer */
  if (pos > 0)
  {
    size_t out = 0;
    while (buf[pos] != '\0')
    {
      buf[out++] = buf[pos++];
    }
    buf[out] = '\0';
  }
}

/* ================================================================== */
/* Signal handler — NFR-OBS-09                                        */
/* ================================================================== */

static void
sk_crash_signal_handler(int signum)
{
  /* We must use only async-signal-safe functions here.
   * We use the pre-computed s_crash_dir to avoid malloc in signal context.
   */

  /* Build crash dump filename:
   * crash-YYYYMMDD-HHMMSS-PID.txt
   */
  if (s_crash_dir[0] == '\0')
    goto reraise;

  /* Create crash dir (best-effort, may already exist) */
  (void)mkdir(s_crash_dir, 0700);

  time_t now = time(NULL);
  struct tm tm_info;
  gmtime_r(&now, &tm_info);

  char filename[512];
  /* Format manually to stay async-signal-safe */
  char year[8], month[4], day[4], hour[4], minute[4], second[4], pidbuf[16];
  int_to_str(tm_info.tm_year + 1900, year, sizeof(year));
  int_to_str(tm_info.tm_mon + 1, month, sizeof(month));
  int_to_str(tm_info.tm_mday, day, sizeof(day));
  int_to_str(tm_info.tm_hour, hour, sizeof(hour));
  int_to_str(tm_info.tm_min, minute, sizeof(minute));
  int_to_str(tm_info.tm_sec, second, sizeof(second));
  int_to_str((int)getpid(), pidbuf, sizeof(pidbuf));

  /* Manual path assembly (snprintf is NOT async-signal-safe) */
  size_t pos = 0;
  size_t dlen = strlen(s_crash_dir);
  if (dlen + 100 > sizeof(filename))
    goto reraise;
  memcpy(filename, s_crash_dir, dlen);
  pos = dlen;

  filename[pos++] = '/';

  /* "crash-" */
  memcpy(filename + pos, "crash-", 6);
  pos += 6;

  /* YYYYMMDD */
  size_t ylen = strlen(year);
  memcpy(filename + pos, year, ylen);
  pos += ylen;
  /* Zero-pad month/day */
  if (strlen(month) == 1)
  {
    filename[pos++] = '0';
  }
  size_t mlen = strlen(month);
  memcpy(filename + pos, month, mlen);
  pos += mlen;
  if (strlen(day) == 1)
  {
    filename[pos++] = '0';
  }
  size_t dalen = strlen(day);
  memcpy(filename + pos, day, dalen);
  pos += dalen;

  filename[pos++] = '-';

  /* HHMMSS */
  if (strlen(hour) == 1)
  {
    filename[pos++] = '0';
  }
  size_t hlen = strlen(hour);
  memcpy(filename + pos, hour, hlen);
  pos += hlen;
  if (strlen(minute) == 1)
  {
    filename[pos++] = '0';
  }
  size_t minlen = strlen(minute);
  memcpy(filename + pos, minute, minlen);
  pos += minlen;
  if (strlen(second) == 1)
  {
    filename[pos++] = '0';
  }
  size_t slen = strlen(second);
  memcpy(filename + pos, second, slen);
  pos += slen;

  filename[pos++] = '-';

  /* PID */
  size_t plen = strlen(pidbuf);
  memcpy(filename + pos, pidbuf, plen);
  pos += plen;

  /* ".txt" */
  memcpy(filename + pos, ".txt", 4);
  pos += 4;
  filename[pos] = '\0';

  /* Open dump file with 0600 — INV-SECURITY-3 */
  int fd = open(filename, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
  if (fd < 0)
    goto reraise;

  /* Write header */
  safe_write(fd, "shellkeep crash dump\n");
  safe_write(fd, "====================\n\n");

  safe_write(fd, "Signal: ");
  char sigbuf[16];
  int_to_str(signum, sigbuf, sizeof(sigbuf));
  safe_write(fd, sigbuf);

  const char *signame = "UNKNOWN";
  switch (signum)
  {
  case SIGSEGV:
    signame = " (SIGSEGV)";
    break;
  case SIGABRT:
    signame = " (SIGABRT)";
    break;
  case SIGBUS:
    signame = " (SIGBUS)";
    break;
  case SIGFPE:
    signame = " (SIGFPE)";
    break;
  }
  safe_write(fd, signame);
  safe_write(fd, "\n");

  safe_write(fd, "PID: ");
  safe_write(fd, pidbuf);
  safe_write(fd, "\n");

  safe_write(fd, "Time: ");
  safe_write(fd, year);
  safe_write(fd, "-");
  safe_write(fd, month);
  safe_write(fd, "-");
  safe_write(fd, day);
  safe_write(fd, "T");
  safe_write(fd, hour);
  safe_write(fd, ":");
  safe_write(fd, minute);
  safe_write(fd, ":");
  safe_write(fd, second);
  safe_write(fd, "Z\n\n");

  /* NFR-OBS-10: explicitly note prohibited data is NOT included */
  safe_write(fd, "NOTE: This dump does not contain terminal content, "
                 "keys, or environment variables.\n\n");

  /* Backtrace — NFR-OBS-09 */
  safe_write(fd, "Backtrace:\n");
  void *frames[SK_CRASH_MAX_FRAMES];
  int nframes = backtrace(frames, SK_CRASH_MAX_FRAMES);
  backtrace_symbols_fd(frames, nframes, fd);
  safe_write(fd, "\n");

  close(fd);

  /* Also attempt to write to stderr for immediate visibility */
  safe_write(STDERR_FILENO, "\n[shellkeep] FATAL: caught signal ");
  safe_write(STDERR_FILENO, sigbuf);
  safe_write(STDERR_FILENO, signame);
  safe_write(STDERR_FILENO, "\n[shellkeep] Crash dump written to: ");
  safe_write(STDERR_FILENO, filename);
  safe_write(STDERR_FILENO, "\n");

reraise:
  /* Re-raise signal with default handler — NFR-OBS-09 */
  signal(signum, SIG_DFL);
  raise(signum);
}

/* ================================================================== */
/* Public API                                                         */
/* ================================================================== */

void
sk_crash_handler_install(void)
{
  /* Pre-compute crash directory path (before entering signal context) */
  char *dir = sk_crash_build_dir();
  if (dir)
  {
    size_t len = strlen(dir);
    if (len < SK_CRASH_DIR_MAXLEN)
    {
      memcpy(s_crash_dir, dir, len + 1);
      /* Pre-create the directory tree with 0700 */
      /* Walk the path creating each component */
      char tmp[SK_CRASH_DIR_MAXLEN];
      memcpy(tmp, dir, len + 1);
      for (char *p = tmp + 1; *p; p++)
      {
        if (*p == '/')
        {
          *p = '\0';
          (void)mkdir(tmp, 0700);
          *p = '/';
        }
      }
      (void)mkdir(tmp, 0700);
    }
    free(dir);
  }

  /* NFR-OBS-09: install signal handlers */
  struct sigaction sa;
  memset(&sa, 0, sizeof(sa));
  sa.sa_handler = sk_crash_signal_handler;
  sa.sa_flags = SA_RESETHAND; /* one-shot: restore default after first delivery */
  sigemptyset(&sa.sa_mask);

  sigaction(SIGSEGV, &sa, NULL);
  sigaction(SIGABRT, &sa, NULL);
  sigaction(SIGBUS, &sa, NULL);
  sigaction(SIGFPE, &sa, NULL);

  /* Prevent core dump from leaking sensitive memory — NFR-OBS-10 */
  prctl(PR_SET_DUMPABLE, 0);
}

bool
sk_crash_has_previous_dumps(void)
{
  /* NFR-OBS-11: check for crash dumps from a previous run */
  char *dir = sk_crash_build_dir();
  if (!dir)
    return false;

  struct stat st;
  if (stat(dir, &st) != 0 || !S_ISDIR(st.st_mode))
  {
    free(dir);
    return false;
  }

  /* Scan directory for crash-*.txt files */
  DIR *dp = opendir(dir);
  free(dir);
  if (!dp)
    return false;

  bool found = false;
  struct dirent *ent;
  while ((ent = readdir(dp)) != NULL)
  {
    if (strncmp(ent->d_name, "crash-", 6) == 0 && strstr(ent->d_name, ".txt") != NULL)
    {
      found = true;
      break;
    }
  }
  closedir(dp);
  return found;
}
