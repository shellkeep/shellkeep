// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_lock.c
 * @brief Client-ID lock mechanism using tmux sessions.
 *
 * Implements FR-LOCK-01..11:
 * - Lock acquire via `tmux new-session` (atomic, fails if exists)
 * - Lock release via `tmux kill-session`
 * - Lock check: read env vars from lock session
 * - Heartbeat: update SHELLKEEP_LOCK_CONNECTED_AT
 * - Orphan detection: compare timestamp with 2x timeout threshold
 * - Env vars: SHELLKEEP_LOCK_CLIENT_ID, _HOSTNAME, _CONNECTED_AT, _PID, _VERSION
 *
 * INV-LOCK-3: Lock session is never used as a terminal.
 */

#include "sk_session_internal.h"
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

/* ------------------------------------------------------------------ */
/* Lock acquire (FR-LOCK-02, FR-LOCK-03, FR-LOCK-08)                  */
/* ------------------------------------------------------------------ */

bool
sk_lock_acquire(SkSessionManager *mgr, const char *client_id, const char *hostname, GError **error)
{
  g_return_val_if_fail(mgr != NULL, false);
  g_return_val_if_fail(client_id != NULL, false);
  g_return_val_if_fail(hostname != NULL, false);

  g_autofree char *lock_name = sk_lock_session_name(client_id);
  g_autofree char *timestamp = sk_iso8601_now();
  g_autofree char *pid_str = g_strdup_printf("%d", (int)getpid());

  /* NFR-SEC-07: shell-safe quoting to prevent injection. */
  g_autofree char *q_lock = sk_shell_quote(lock_name);
  g_autofree char *q_client = sk_shell_quote(client_id);
  g_autofree char *q_host = sk_shell_quote(hostname);
  g_autofree char *q_ts = sk_shell_quote(timestamp);
  g_autofree char *q_pid = sk_shell_quote(pid_str);
  g_autofree char *q_ver = sk_shell_quote(SK_VERSION_STRING);

  /* FR-LOCK-02: tmux new-session is atomic — fails if session exists.
   * FR-LOCK-03: set all required env vars.
   * FR-LOCK-11: -d = detached, never used as a terminal (INV-LOCK-3). */
  g_autofree char *cmd =
      g_strdup_printf("tmux new-session -d -s %s "
                      "\\; set-environment -t %s SHELLKEEP_LOCK_CLIENT_ID %s "
                      "\\; set-environment -t %s SHELLKEEP_LOCK_HOSTNAME %s "
                      "\\; set-environment -t %s SHELLKEEP_LOCK_CONNECTED_AT %s "
                      "\\; set-environment -t %s SHELLKEEP_LOCK_PID %s "
                      "\\; set-environment -t %s SHELLKEEP_LOCK_VERSION %s",
                      q_lock, q_lock, q_client, q_lock, q_host, q_lock, q_ts,
                      q_lock, q_pid, q_lock, q_ver);

  char *output = NULL;
  int rc = sk_session_exec_command(mgr->conn, cmd, &output, error);
  g_free(output);

  if (rc != 0)
  {
    if (error != NULL && *error == NULL)
    {
      g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_LOCK_CONFLICT,
                  "Lock session '%s' already exists — "
                  "another client may be connected",
                  lock_name);
    }
    return false;
  }

  return true;
}

/* ------------------------------------------------------------------ */
/* Lock release (FR-LOCK-10)                                           */
/* ------------------------------------------------------------------ */

bool
sk_lock_release(SkSessionManager *mgr, const char *client_id, GError **error)
{
  g_return_val_if_fail(mgr != NULL, false);
  g_return_val_if_fail(client_id != NULL, false);

  g_autofree char *lock_name = sk_lock_session_name(client_id);

  /* FR-LOCK-10: destroy lock as last operation before disconnect. */
  g_autofree char *q_lock = sk_shell_quote(lock_name);
  g_autofree char *cmd = g_strdup_printf("tmux kill-session -t %s 2>/dev/null", q_lock);

  char *output = NULL;
  int rc = sk_session_exec_command(mgr->conn, cmd, &output, error);
  g_free(output);

  if (rc != 0)
  {
    if (error != NULL && *error == NULL)
    {
      g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_LOCK,
                  "Failed to release lock session '%s'", lock_name);
    }
    return false;
  }

  return true;
}

/* ------------------------------------------------------------------ */
/* Lock check (FR-LOCK-03, FR-LOCK-04)                                */
/* ------------------------------------------------------------------ */

/**
 * Parse an env var line like "VARNAME=value" from tmux show-environment.
 * Returns the value portion, or NULL if no '=' found.
 */
static char *
parse_env_value(const char *line, const char *var_name)
{
  if (line == NULL)
    return NULL;

  /* Lines starting with '-' are unset variables. */
  if (line[0] == '-')
    return NULL;

  size_t name_len = strlen(var_name);
  if (strncmp(line, var_name, name_len) == 0 && line[name_len] == '=')
  {
    return g_strdup(line + name_len + 1);
  }
  return NULL;
}

SkLockInfo *
sk_lock_check(SkSessionManager *mgr, const char *client_id, GError **error)
{
  g_return_val_if_fail(mgr != NULL, NULL);
  g_return_val_if_fail(client_id != NULL, NULL);

  g_autofree char *lock_name = sk_lock_session_name(client_id);

  /* Check if the lock session exists. NFR-SEC-07: safe quoting. */
  g_autofree char *q_lock = sk_shell_quote(lock_name);
  g_autofree char *has_cmd = g_strdup_printf("tmux has-session -t %s 2>/dev/null", q_lock);
  int rc = sk_session_exec_command(mgr->conn, has_cmd, NULL, NULL);
  if (rc != 0)
  {
    return NULL; /* No lock exists. */
  }

  /* Read all environment variables from the lock session. */
  g_autofree char *env_cmd =
      g_strdup_printf("tmux show-environment -t %s 2>/dev/null", q_lock);
  char *output = NULL;
  rc = sk_session_exec_command(mgr->conn, env_cmd, &output, error);
  if (rc != 0 || output == NULL)
  {
    g_free(output);
    return NULL;
  }

  SkLockInfo *info = g_new0(SkLockInfo, 1);

  gchar **lines = g_strsplit(output, "\n", -1);
  g_free(output);

  for (int i = 0; lines[i] != NULL; i++)
  {
    g_strstrip(lines[i]);

    char *val;

    val = parse_env_value(lines[i], "SHELLKEEP_LOCK_CLIENT_ID");
    if (val != NULL)
    {
      info->client_id = val;
      continue;
    }

    val = parse_env_value(lines[i], "SHELLKEEP_LOCK_HOSTNAME");
    if (val != NULL)
    {
      info->hostname = val;
      continue;
    }

    val = parse_env_value(lines[i], "SHELLKEEP_LOCK_CONNECTED_AT");
    if (val != NULL)
    {
      info->connected_at = val;
      continue;
    }

    val = parse_env_value(lines[i], "SHELLKEEP_LOCK_PID");
    if (val != NULL)
    {
      info->pid = val;
      continue;
    }

    val = parse_env_value(lines[i], "SHELLKEEP_LOCK_VERSION");
    if (val != NULL)
    {
      info->version = val;
      continue;
    }
  }

  g_strfreev(lines);

  /* FR-LOCK-04: validate that all required fields are present
   * and client_id matches the suffix. */
  info->valid =
      (info->client_id != NULL && info->hostname != NULL && info->connected_at != NULL &&
       info->pid != NULL && info->version != NULL && strcmp(info->client_id, client_id) == 0);

  /* FR-LOCK-04: invalid lock — destroy silently. */
  if (!info->valid)
  {
    sk_session_kill_by_name(mgr, lock_name, NULL);
    sk_lock_info_free(info);
    return NULL;
  }

  /* Check orphan status. */
  info->orphaned = sk_lock_is_orphaned(info, SK_LOCK_DEFAULT_KEEPALIVE_TIMEOUT);

  return info;
}

/* ------------------------------------------------------------------ */
/* Heartbeat update (FR-LOCK-09)                                       */
/* ------------------------------------------------------------------ */

bool
sk_lock_update_heartbeat(SkSessionManager *mgr, const char *client_id, GError **error)
{
  g_return_val_if_fail(mgr != NULL, false);
  g_return_val_if_fail(client_id != NULL, false);

  g_autofree char *lock_name = sk_lock_session_name(client_id);
  g_autofree char *timestamp = sk_iso8601_now();

  /* FR-LOCK-09: update connected_at timestamp. NFR-SEC-07: safe quoting. */
  g_autofree char *q_lock = sk_shell_quote(lock_name);
  g_autofree char *q_ts = sk_shell_quote(timestamp);
  g_autofree char *cmd = g_strdup_printf(
      "tmux set-environment -t %s SHELLKEEP_LOCK_CONNECTED_AT %s", q_lock, q_ts);

  char *output = NULL;
  int rc = sk_session_exec_command(mgr->conn, cmd, &output, error);
  g_free(output);

  if (rc != 0)
  {
    if (error != NULL && *error == NULL)
    {
      g_set_error(error, SK_SESSION_ERROR, SK_SESSION_ERROR_LOCK,
                  "Failed to update lock heartbeat for '%s'", lock_name);
    }
    return false;
  }

  return true;
}

/* ------------------------------------------------------------------ */
/* Orphan detection (FR-LOCK-07)                                       */
/* ------------------------------------------------------------------ */

bool
sk_lock_is_orphaned(const SkLockInfo *info, int keepalive_timeout)
{
  if (info == NULL || info->connected_at == NULL)
  {
    return true;
  }

  time_t connected_at = sk_iso8601_parse(info->connected_at);
  if (connected_at == (time_t)-1)
  {
    return true; /* Unparseable timestamp = treat as orphaned. */
  }

  time_t now = time(NULL);

  /* FR-LOCK-07: orphaned if connected_at + (2 * keepalive_timeout) < now. */
  time_t threshold = connected_at + (time_t)(SK_LOCK_ORPHAN_MULTIPLIER * keepalive_timeout);

  return (now > threshold);
}

/* ------------------------------------------------------------------ */
/* Own lock check (FR-LOCK-06)                                         */
/* ------------------------------------------------------------------ */

bool
sk_lock_is_own(const SkLockInfo *info, const char *hostname, const char *pid)
{
  if (info == NULL || info->hostname == NULL || info->pid == NULL)
  {
    return false;
  }

  /* FR-LOCK-06: same hostname AND same PID = own reconnection. */
  return (strcmp(info->hostname, hostname) == 0 && strcmp(info->pid, pid) == 0);
}

/* ------------------------------------------------------------------ */
/* Cleanup                                                             */
/* ------------------------------------------------------------------ */

void
sk_lock_info_free(SkLockInfo *info)
{
  if (info == NULL)
    return;
  g_free(info->client_id);
  g_free(info->hostname);
  g_free(info->connected_at);
  g_free(info->pid);
  g_free(info->version);
  g_free(info);
}
