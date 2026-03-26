// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file fuzz_state_load.c
 * @brief Fuzz target for sk_state_from_json() and sk_state_load().
 *
 * Feeds arbitrary JSON (and non-JSON) data into the state parser to
 * find crashes, memory leaks, and undefined behavior.
 *
 * Build: clang -g -fsanitize=address,undefined,fuzzer ...
 * Run:   ./fuzz_state_load tests/fuzz/corpus/state/
 */

#include "shellkeep/sk_state.h"

#include <glib.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/**
 * libFuzzer entry point.
 *
 * Strategy: parse the fuzzed data as a JSON string via sk_state_from_json().
 * Also test the file-based sk_state_load() path by writing data to a
 * temporary file.
 */
int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size);

int
LLVMFuzzerTestOneInput(const uint8_t *data, size_t size)
{
  /* Limit input size to avoid OOM on enormous inputs. */
  if (size > (1024 * 1024))
  {
    return 0;
  }

  /* Ensure NUL-terminated string for sk_state_from_json(). */
  char *json = g_malloc(size + 1);
  memcpy(json, data, size);
  json[size] = '\0';

  /* --- Test 1: sk_state_from_json() --- */
  GError *error = NULL;
  SkStateFile *state = sk_state_from_json(json, &error);

  if (state != NULL)
  {
    /* If parsing succeeded, exercise validation and serialization. */
    GError *val_err = NULL;
    sk_state_validate(state, &val_err);
    g_clear_error(&val_err);

    /* Round-trip: serialize back to JSON. */
    char *json_out = sk_state_to_json(state);
    g_free(json_out);

    sk_state_file_free(state);
  }
  g_clear_error(&error);

  /* --- Test 2: sk_state_load() via temp file --- */
  char tmppath[] = "/tmp/fuzz_state_XXXXXX";
  int fd = mkstemp(tmppath);
  if (fd >= 0)
  {
    ssize_t written = write(fd, data, size);
    (void)written;
    close(fd);

    GError *load_err = NULL;
    SkStateFile *loaded = sk_state_load(tmppath, &load_err);
    if (loaded != NULL)
    {
      sk_state_file_free(loaded);
    }
    g_clear_error(&load_err);

    unlink(tmppath);
  }

  g_free(json);
  return 0;
}
