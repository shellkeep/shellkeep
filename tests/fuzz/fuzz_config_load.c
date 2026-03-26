// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file fuzz_config_load.c
 * @brief Fuzz target for sk_config_load() — arbitrary INI input.
 *
 * Feeds arbitrary data (valid/invalid INI) into the config parser.
 * sk_config_load() should always return defaults on parse error, never crash.
 *
 * Build: clang -g -fsanitize=address,undefined,fuzzer ...
 * Run:   ./fuzz_config_load tests/fuzz/corpus/config/
 */

#include "shellkeep/sk_config.h"

#include <glib.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size);

int
LLVMFuzzerTestOneInput(const uint8_t *data, size_t size)
{
  /* Limit input size to avoid OOM. */
  if (size > (512 * 1024))
  {
    return 0;
  }

  /* Write fuzzed data to a temporary config file. */
  char tmppath[] = "/tmp/fuzz_config_XXXXXX";
  int fd = mkstemp(tmppath);
  if (fd < 0)
  {
    return 0;
  }

  ssize_t written = write(fd, data, size);
  (void)written;
  close(fd);

  /* --- Test: sk_config_load() --- */
  GError *error = NULL;
  SkConfig *config = sk_config_load(tmppath, &error);

  if (config != NULL)
  {
    /* Exercise accessors to trigger any deferred parsing bugs. */
    (void)sk_config_get_string(config, "terminal.font_family");
    (void)sk_config_get_int(config, "terminal.font_size", 12);
    (void)sk_config_get_bool(config, "tray.enabled", false);
    (void)sk_config_get_string(config, "ssh.known_hosts_file");
    (void)sk_config_get_int(config, "ssh.connect_timeout", 30);
    (void)sk_config_get_keepalive_interval(config);
    (void)sk_config_get_keepalive_max_attempts(config);

    /* Validate client_id if present. */
    if (config->client_id != NULL)
    {
      (void)sk_config_validate_client_id(config->client_id);
    }

    sk_config_free(config);
  }
  g_clear_error(&error);

  unlink(tmppath);
  return 0;
}
