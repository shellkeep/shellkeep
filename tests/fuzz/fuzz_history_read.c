// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file fuzz_history_read.c
 * @brief Fuzz target for sk_history_read() and sk_history_event_from_json().
 *
 * Feeds arbitrary JSONL data (one JSON object per line) into the history
 * parser to find crashes, memory corruption, and UB.
 *
 * Build: clang -g -fsanitize=address,undefined,fuzzer ...
 * Run:   ./fuzz_history_read tests/fuzz/corpus/history/
 */

#include "shellkeep/sk_state.h"

#include <glib.h>
#include <fcntl.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size);

int
LLVMFuzzerTestOneInput(const uint8_t *data, size_t size)
{
  /* Limit input size. */
  if (size > (2 * 1024 * 1024))
  {
    return 0;
  }

  /* --- Test 1: Parse individual lines via sk_history_event_from_json() --- */
  {
    char *buf = g_malloc(size + 1);
    memcpy(buf, data, size);
    buf[size] = '\0';

    /* Split by newlines and parse each line. */
    char **lines = g_strsplit(buf, "\n", -1);
    if (lines != NULL)
    {
      for (int i = 0; lines[i] != NULL; i++)
      {
        SkHistoryEvent *event = sk_history_event_from_json(lines[i]);
        if (event != NULL)
        {
          /* Exercise serialization round-trip. */
          char *json_out = sk_history_event_to_json(event);
          g_free(json_out);

          sk_history_event_free(event);
        }
      }
      g_strfreev(lines);
    }

    g_free(buf);
  }

  /* --- Test 2: sk_history_read() via temp file --- */
  {
    /* Create a temp directory for the history file. */
    char tmpdir[] = "/tmp/fuzz_hist_XXXXXX";
    char *dir = mkdtemp(tmpdir);
    if (dir == NULL)
    {
      return 0;
    }

    /* Use a fixed UUID for the session. */
    const char *session_uuid = "fuzz-test-session";
    char *filepath = g_strdup_printf("%s/%s.jsonl", dir, session_uuid);

    /* Write fuzzed data as the JSONL file. */
    int fd = open(filepath, O_WRONLY | O_CREAT | O_TRUNC, 0600);
    if (fd >= 0)
    {
      ssize_t written = write(fd, data, size);
      (void)written;
      close(fd);

      GError *error = NULL;
      int n_events = 0;
      SkHistoryEvent **events = sk_history_read(session_uuid, dir, &n_events, &error);

      if (events != NULL)
      {
        for (int i = 0; i < n_events; i++)
        {
          sk_history_event_free(events[i]);
        }
        g_free(events);
      }
      g_clear_error(&error);
    }

    unlink(filepath);
    rmdir(dir);
    g_free(filepath);
  }

  return 0;
}
