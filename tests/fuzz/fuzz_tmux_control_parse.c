// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file fuzz_tmux_control_parse.c
 * @brief Fuzz target for sk_ctrl_parse_notification() and sk_tmux_parse_version().
 *
 * Feeds arbitrary strings into the tmux control mode notification parser
 * and the tmux version string parser. These accept external input from
 * the remote server via SSH, so they must handle adversarial data.
 *
 * Build: clang -g -fsanitize=address,undefined,fuzzer ...
 * Run:   ./fuzz_tmux_control_parse tests/fuzz/corpus/tmux_control/
 */

#include "shellkeep/sk_session.h"

#include <glib.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size);

int
LLVMFuzzerTestOneInput(const uint8_t *data, size_t size)
{
  /* Limit input size. */
  if (size > (256 * 1024))
  {
    return 0;
  }

  /* NUL-terminate the input. */
  char *input = g_malloc(size + 1);
  memcpy(input, data, size);
  input[size] = '\0';

  /* --- Test 1: sk_ctrl_parse_notification() on entire input --- */
  {
    SkCtrlNotification *notif = sk_ctrl_parse_notification(input);
    if (notif != NULL)
    {
      /* Access fields to trigger any deferred issues. */
      (void)notif->type;
      (void)notif->cmd_number;
      if (notif->data != NULL)
      {
        (void)strlen(notif->data);
      }
      sk_ctrl_notification_free(notif);
    }
  }

  /* --- Test 2: Parse line-by-line (simulating real control mode stream) --- */
  {
    char **lines = g_strsplit(input, "\n", -1);
    if (lines != NULL)
    {
      for (int i = 0; lines[i] != NULL; i++)
      {
        SkCtrlNotification *notif = sk_ctrl_parse_notification(lines[i]);
        if (notif != NULL)
        {
          sk_ctrl_notification_free(notif);
        }
      }
      g_strfreev(lines);
    }
  }

  /* --- Test 3: sk_tmux_parse_version() --- */
  {
    int major = 0, minor = 0;
    bool ok = sk_tmux_parse_version(input, &major, &minor);
    if (ok)
    {
      (void)sk_tmux_version_ok(major, minor);
    }
  }

  /* --- Test 4: sk_session_parse_name() --- */
  {
    char *client_id = NULL;
    char *environment = NULL;
    char *session_name = NULL;
    bool ok = sk_session_parse_name(input, &client_id, &environment, &session_name);
    if (ok)
    {
      g_free(client_id);
      g_free(environment);
      g_free(session_name);
    }
  }

  g_free(input);
  return 0;
}
