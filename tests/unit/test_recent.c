// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_recent.c
 * @brief Unit tests for recent connections management.
 *
 * Tests Appendix A.3: sk_recent_new, sk_recent_add, merge/dedup,
 * eviction at max 50 entries, sk_recent_free, sk_recent_connection_free.
 */

#include "shellkeep/sk_state.h"

#include "test_helpers.h"
/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */
#include <string.h>

/* ---- Test: sk_recent_new ------------------------------------------------ */

static void
test_recent_new(void **state)
{
  (void)state;

  SkRecentConnections *rc = sk_recent_new();
  assert_non_null(rc);
  assert_int_equal(rc->schema_version, SK_RECENT_SCHEMA_VERSION);
  assert_int_equal(rc->n_connections, 0);

  sk_recent_free(rc);
}

/* ---- Test: sk_recent_free NULL safety ----------------------------------- */

static void
test_recent_free_null(void **state)
{
  (void)state;
  sk_recent_free(NULL); /* Should not crash. */
}

/* ---- Test: sk_recent_connection_free NULL safety ------------------------ */

static void
test_recent_connection_free_null(void **state)
{
  (void)state;
  sk_recent_connection_free(NULL); /* Should not crash. */
}

/* ---- Test: sk_recent_add basic ------------------------------------------ */

static void
test_recent_add_basic(void **state)
{
  (void)state;

  SkRecentConnections *rc = sk_recent_new();
  assert_non_null(rc);

  sk_recent_add(rc, "example.com", "user1", 22, "my-server", "SHA256:abc123");

  assert_int_equal(rc->n_connections, 1);
  assert_string_equal(rc->connections[0]->host, "example.com");
  assert_string_equal(rc->connections[0]->user, "user1");
  assert_int_equal(rc->connections[0]->port, 22);
  assert_string_equal(rc->connections[0]->alias, "my-server");
  assert_string_equal(rc->connections[0]->host_key_fingerprint, "SHA256:abc123");
  assert_non_null(rc->connections[0]->last_connected);

  sk_recent_free(rc);
}

/* ---- Test: sk_recent_add merge duplicate -------------------------------- */

static void
test_recent_add_merge_duplicate(void **state)
{
  (void)state;

  SkRecentConnections *rc = sk_recent_new();

  sk_recent_add(rc, "example.com", "user1", 22, "old-alias", NULL);
  assert_int_equal(rc->n_connections, 1);
  char *first_ts = g_strdup(rc->connections[0]->last_connected);

  /* Add same host+user+port again with new alias. */
  sk_recent_add(rc, "example.com", "user1", 22, "new-alias", "SHA256:xyz");

  /* Should still be 1 entry (merged, not duplicated). */
  assert_int_equal(rc->n_connections, 1);
  assert_string_equal(rc->connections[0]->alias, "new-alias");
  assert_string_equal(rc->connections[0]->host_key_fingerprint, "SHA256:xyz");

  g_free(first_ts);
  sk_recent_free(rc);
}

/* ---- Test: sk_recent_add different entries not merged -------------------- */

static void
test_recent_add_different_entries(void **state)
{
  (void)state;

  SkRecentConnections *rc = sk_recent_new();

  sk_recent_add(rc, "host1.com", "user1", 22, NULL, NULL);
  sk_recent_add(rc, "host2.com", "user1", 22, NULL, NULL);
  sk_recent_add(rc, "host1.com", "user2", 22, NULL, NULL); /* Different user */
  sk_recent_add(rc, "host1.com", "user1", 2222, NULL, NULL); /* Different port */

  assert_int_equal(rc->n_connections, 4);

  sk_recent_free(rc);
}

/* ---- Test: sk_recent_add most recent at front --------------------------- */

static void
test_recent_add_front_ordering(void **state)
{
  (void)state;

  SkRecentConnections *rc = sk_recent_new();

  sk_recent_add(rc, "host1.com", "user", 22, NULL, NULL);
  sk_recent_add(rc, "host2.com", "user", 22, NULL, NULL);
  sk_recent_add(rc, "host3.com", "user", 22, NULL, NULL);

  /* Most recently added should be at front. */
  assert_string_equal(rc->connections[0]->host, "host3.com");
  assert_string_equal(rc->connections[1]->host, "host2.com");
  assert_string_equal(rc->connections[2]->host, "host1.com");

  sk_recent_free(rc);
}

/* ---- Test: sk_recent_add merge moves to front --------------------------- */

static void
test_recent_add_merge_moves_to_front(void **state)
{
  (void)state;

  SkRecentConnections *rc = sk_recent_new();

  sk_recent_add(rc, "host1.com", "user", 22, NULL, NULL);
  sk_recent_add(rc, "host2.com", "user", 22, NULL, NULL);
  sk_recent_add(rc, "host3.com", "user", 22, NULL, NULL);

  /* Re-connect to host1 — should move to front. */
  sk_recent_add(rc, "host1.com", "user", 22, NULL, NULL);

  assert_int_equal(rc->n_connections, 3);
  assert_string_equal(rc->connections[0]->host, "host1.com");
  assert_string_equal(rc->connections[1]->host, "host3.com");
  assert_string_equal(rc->connections[2]->host, "host2.com");

  sk_recent_free(rc);
}

/* ---- Test: sk_recent_add eviction at max 50 ----------------------------- */

static void
test_recent_add_eviction(void **state)
{
  (void)state;

  SkRecentConnections *rc = sk_recent_new();

  /* Add 55 entries — should be capped at 50. */
  for (int i = 0; i < 55; i++)
  {
    char host[32];
    g_snprintf(host, sizeof(host), "host-%03d.com", i);
    sk_recent_add(rc, host, "user", 22, NULL, NULL);
  }

  assert_int_equal(rc->n_connections, SK_RECENT_MAX_ENTRIES);
  /* Most recent should be at front. */
  assert_string_equal(rc->connections[0]->host, "host-054.com");

  sk_recent_free(rc);
}

/* ---- Test: sk_recent_add with NULL optional fields ---------------------- */

static void
test_recent_add_null_optional(void **state)
{
  (void)state;

  SkRecentConnections *rc = sk_recent_new();

  sk_recent_add(rc, "example.com", "user", 22, NULL, NULL);
  assert_int_equal(rc->n_connections, 1);
  /* alias and fingerprint can be NULL. */
  assert_non_null(rc->connections[0]->host);
  assert_non_null(rc->connections[0]->user);

  sk_recent_free(rc);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_recent_new),
    cmocka_unit_test(test_recent_free_null),
    cmocka_unit_test(test_recent_connection_free_null),
    cmocka_unit_test(test_recent_add_basic),
    cmocka_unit_test(test_recent_add_merge_duplicate),
    cmocka_unit_test(test_recent_add_different_entries),
    cmocka_unit_test(test_recent_add_front_ordering),
    cmocka_unit_test(test_recent_add_merge_moves_to_front),
    cmocka_unit_test(test_recent_add_eviction),
    cmocka_unit_test(test_recent_add_null_optional),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
