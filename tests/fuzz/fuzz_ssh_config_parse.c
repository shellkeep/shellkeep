// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file fuzz_ssh_config_parse.c
 * @brief Fuzz target for SSH config file parsing.
 *
 * shellkeep reads ~/.ssh/config indirectly via libssh's ssh_options_parse_config().
 * Since we cannot fuzz libssh internals directly, this target exercises the
 * shellkeep code paths that process SSH config values after libssh parses them.
 *
 * We also fuzz the sk_config_load() path with ssh-related INI fields set to
 * adversarial values, and test sk_config_validate_client_id() with arbitrary
 * strings (since client_id may come from user-editable config).
 *
 * Build: clang -g -fsanitize=address,undefined,fuzzer ...
 * Run:   ./fuzz_ssh_config_parse tests/fuzz/corpus/ssh_config/
 *
 * NOTE: The actual SSH config parsing is done by libssh. This fuzz target
 * tests shellkeep's handling of values that originate from SSH config,
 * such as hostnames, identity files, ports, and known_hosts paths.
 */

#include "shellkeep/sk_config.h"
#include "shellkeep/sk_state.h"

#include <glib.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

/**
 * Generate an INI config string with adversarial SSH-related values
 * derived from the fuzz input.
 */
static char *
build_adversarial_ini(const uint8_t *data, size_t size)
{
  /* Split the fuzz input into fields by NUL bytes (or use chunks). */
  GString *ini = g_string_new("[ssh]\n");

  /* Use portions of the input for different SSH config fields. */
  size_t offset = 0;

  /* known_hosts_file */
  if (offset < size)
  {
    size_t field_len = 0;
    while (offset + field_len < size && data[offset + field_len] != '\0'
           && data[offset + field_len] != '\n')
    {
      field_len++;
    }
    char *val = g_strndup((const char *)data + offset, field_len);
    g_string_append_printf(ini, "known_hosts_file = %s\n", val);
    g_free(val);
    offset += field_len + 1;
  }

  /* identity_file */
  if (offset < size)
  {
    size_t field_len = 0;
    while (offset + field_len < size && data[offset + field_len] != '\0'
           && data[offset + field_len] != '\n')
    {
      field_len++;
    }
    char *val = g_strndup((const char *)data + offset, field_len);
    g_string_append_printf(ini, "identity_file = %s\n", val);
    g_free(val);
    offset += field_len + 1;
  }

  /* connect_timeout — try to inject non-numeric / extreme values */
  if (offset + 4 <= size)
  {
    int32_t timeout;
    memcpy(&timeout, data + offset, 4);
    g_string_append_printf(ini, "connect_timeout = %d\n", timeout);
    offset += 4;
  }

  /* keepalive_interval */
  if (offset + 4 <= size)
  {
    int32_t keepalive;
    memcpy(&keepalive, data + offset, 4);
    g_string_append_printf(ini, "keepalive_interval = %d\n", keepalive);
    offset += 4;
  }

  /* Add a general section with adversarial client_id. */
  g_string_append(ini, "\n[general]\n");
  if (offset < size)
  {
    size_t field_len = size - offset;
    if (field_len > 256)
      field_len = 256;
    char *val = g_strndup((const char *)data + offset, field_len);
    g_string_append_printf(ini, "client_id = %s\n", val);
    g_free(val);
  }

  return g_string_free(ini, FALSE);
}

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size);

int
LLVMFuzzerTestOneInput(const uint8_t *data, size_t size)
{
  /* Limit input size. */
  if (size > (256 * 1024))
  {
    return 0;
  }

  /* --- Test 1: sk_config_validate_client_id() with arbitrary strings --- */
  {
    char *str = g_malloc(size + 1);
    memcpy(str, data, size);
    str[size] = '\0';

    (void)sk_config_validate_client_id(str);
    g_free(str);
  }

  /* --- Test 2: sk_config_load() with adversarial SSH config values --- */
  {
    char *ini_content = build_adversarial_ini(data, size);

    char tmppath[] = "/tmp/fuzz_sshcfg_XXXXXX";
    int fd = mkstemp(tmppath);
    if (fd >= 0)
    {
      ssize_t written = write(fd, ini_content, strlen(ini_content));
      (void)written;
      close(fd);

      GError *error = NULL;
      SkConfig *config = sk_config_load(tmppath, &error);

      if (config != NULL)
      {
        /* Exercise SSH-related accessors. */
        (void)sk_config_get_string(config, "ssh.known_hosts_file");
        (void)sk_config_get_string(config, "ssh.identity_file");
        (void)sk_config_get_int(config, "ssh.connect_timeout", 30);
        (void)sk_config_get_keepalive_interval(config);
        (void)sk_config_get_keepalive_max_attempts(config);

        if (config->client_id != NULL)
        {
          (void)sk_config_validate_client_id(config->client_id);
        }

        sk_config_free(config);
      }
      g_clear_error(&error);

      unlink(tmppath);
    }

    g_free(ini_content);
  }

  /* --- Test 3: Parse a raw SSH config file through libssh (if available). */
  /* libssh's ssh_options_parse_config() is the actual parser; we write the
   * fuzz data as an SSH config file and attempt to parse it. This tests
   * libssh's robustness, not shellkeep's directly, but it is a dependency
   * that processes external input on behalf of shellkeep. */
  /* TODO: Enable when libssh fuzz harness integration is ready. */

  return 0;
}
