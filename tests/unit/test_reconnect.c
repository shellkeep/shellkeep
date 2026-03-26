// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file test_reconnect.c
 * @brief Unit tests for the reconnection engine.
 *
 * Tests FR-RECONNECT-06..07: backoff calculation with jitter,
 * error classification (transient vs permanent).
 *
 * NFR-BUILD-03..05
 */

#include "shellkeep/sk_reconnect.h"
#include "shellkeep/sk_ssh.h"

#include "test_helpers.h"
/* clang-format off */
#include <setjmp.h>
#include <stdarg.h>
#include <stddef.h>
#include <cmocka.h>
/* clang-format on */
#include <math.h>

/* ---- Test: backoff base case -------------------------------------------- */

static void
test_backoff_base(void **state)
{
  (void)state;

  /* Attempt 0: delay should be around base_sec (2.0) +/- 25%. */
  double delay = sk_backoff_delay(2.0, 0);
  assert_true(delay >= 1.4); /* 2.0 - 25% = 1.5, with floor 0.1 */
  assert_true(delay <= 2.6); /* 2.0 + 25% = 2.5, with some tolerance */
}

/* ---- Test: backoff exponential growth ----------------------------------- */

static void
test_backoff_exponential(void **state)
{
  (void)state;

  /* Attempt 0: ~2s, 1: ~4s, 2: ~8s, 3: ~16s, 4: ~32s */
  double prev = 0;
  for (int attempt = 0; attempt < 5; attempt++)
  {
    /* Run multiple times to average out jitter. */
    double sum = 0;
    int n = 100;
    for (int i = 0; i < n; i++)
    {
      sum += sk_backoff_delay(2.0, attempt);
    }
    double avg = sum / n;

    /* Expected raw: 2 * 2^attempt */
    double expected_raw = 2.0 * pow(2.0, (double)attempt);
    if (expected_raw > 60.0)
      expected_raw = 60.0;

    /* Average should be close to raw (jitter averages out). */
    assert_true(avg > expected_raw * 0.5);
    assert_true(avg < expected_raw * 1.5);

    /* Should generally grow. */
    if (attempt > 0)
    {
      assert_true(avg > prev * 0.5);
    }
    prev = avg;
  }
}

/* ---- Test: backoff cap at 60 seconds ------------------------------------ */

static void
test_backoff_cap(void **state)
{
  (void)state;

  /* Very high attempt number — should be capped at 60s + jitter. */
  for (int i = 0; i < 50; i++)
  {
    double delay = sk_backoff_delay(2.0, 20);
    /* Max raw is 60. With +25% jitter: 75. */
    assert_true(delay <= 76.0);
    /* Min is 60 - 25% = 45, but floor is 0.1. */
    assert_true(delay >= 0.1);
  }
}

/* ---- Test: backoff with negative attempt defaults to 0 ------------------ */

static void
test_backoff_negative_attempt(void **state)
{
  (void)state;

  double delay = sk_backoff_delay(2.0, -5);
  /* Should treat as attempt 0: ~2s. */
  assert_true(delay >= 0.1);
  assert_true(delay <= 3.0);
}

/* ---- Test: backoff with zero/negative base uses default (2.0) ----------- */

static void
test_backoff_zero_base(void **state)
{
  (void)state;

  double delay = sk_backoff_delay(0.0, 0);
  /* Default base is 2.0. */
  assert_true(delay >= 0.1);
  assert_true(delay <= 3.0);

  delay = sk_backoff_delay(-1.0, 0);
  assert_true(delay >= 0.1);
  assert_true(delay <= 3.0);
}

/* ---- Test: backoff jitter range ----------------------------------------- */

static void
test_backoff_jitter(void **state)
{
  (void)state;

  /* For attempt 0, base 10.0: raw = 10.0, jitter range = +-2.5.
   * Delay should be in [7.5, 12.5]. */
  double min_seen = 100.0;
  double max_seen = 0.0;

  for (int i = 0; i < 1000; i++)
  {
    double d = sk_backoff_delay(10.0, 0);
    if (d < min_seen)
      min_seen = d;
    if (d > max_seen)
      max_seen = d;
  }

  /* Should see values across the jitter range. */
  assert_true(min_seen < 8.5);
  assert_true(max_seen > 11.0);
  /* Should stay within theoretical bounds. */
  assert_true(min_seen >= 7.4);
  assert_true(max_seen <= 12.6);
}

/* ---- Test: error classification — transient errors ---------------------- */

static void
test_classify_transient(void **state)
{
  (void)state;

  /* NULL error is transient. */
  assert_int_equal(sk_reconnect_classify_error(NULL), SK_DISCONNECT_TRANSIENT);

  /* CONNECT error. */
  GError *err = g_error_new(SK_SSH_ERROR, SK_SSH_ERROR_CONNECT, "Connection refused");
  assert_int_equal(sk_reconnect_classify_error(err), SK_DISCONNECT_TRANSIENT);
  g_error_free(err);

  /* TIMEOUT error. */
  err = g_error_new(SK_SSH_ERROR, SK_SSH_ERROR_TIMEOUT, "Connection timed out");
  assert_int_equal(sk_reconnect_classify_error(err), SK_DISCONNECT_TRANSIENT);
  g_error_free(err);

  /* DISCONNECTED error. */
  err = g_error_new(SK_SSH_ERROR, SK_SSH_ERROR_DISCONNECTED, "Connection lost");
  assert_int_equal(sk_reconnect_classify_error(err), SK_DISCONNECT_TRANSIENT);
  g_error_free(err);

  /* CHANNEL error. */
  err = g_error_new(SK_SSH_ERROR, SK_SSH_ERROR_CHANNEL, "Channel failed");
  assert_int_equal(sk_reconnect_classify_error(err), SK_DISCONNECT_TRANSIENT);
  g_error_free(err);

  /* SFTP error. */
  err = g_error_new(SK_SSH_ERROR, SK_SSH_ERROR_SFTP, "SFTP failed");
  assert_int_equal(sk_reconnect_classify_error(err), SK_DISCONNECT_TRANSIENT);
  g_error_free(err);
}

/* ---- Test: error classification — permanent errors ---------------------- */

static void
test_classify_permanent(void **state)
{
  (void)state;

  /* AUTH error — FR-RECONNECT-07: risk of account lockout. */
  GError *err = g_error_new(SK_SSH_ERROR, SK_SSH_ERROR_AUTH, "Authentication denied");
  assert_int_equal(sk_reconnect_classify_error(err), SK_DISCONNECT_PERMANENT);
  g_error_free(err);

  /* HOST_KEY error — possible MITM. */
  err = g_error_new(SK_SSH_ERROR, SK_SSH_ERROR_HOST_KEY, "Host key mismatch");
  assert_int_equal(sk_reconnect_classify_error(err), SK_DISCONNECT_PERMANENT);
  g_error_free(err);

  /* PROTOCOL error. */
  err = g_error_new(SK_SSH_ERROR, SK_SSH_ERROR_PROTOCOL, "Protocol error");
  assert_int_equal(sk_reconnect_classify_error(err), SK_DISCONNECT_PERMANENT);
  g_error_free(err);

  /* CRYPTO error. */
  err = g_error_new(SK_SSH_ERROR, SK_SSH_ERROR_CRYPTO, "No acceptable algorithms");
  assert_int_equal(sk_reconnect_classify_error(err), SK_DISCONNECT_PERMANENT);
  g_error_free(err);
}

/* ---- Test: non-SSH domain error defaults to transient ------------------- */

static void
test_classify_non_ssh_domain(void **state)
{
  (void)state;

  /* An error from a different domain should not be classified as permanent.
   * The function checks error->domain == SK_SSH_ERROR; if not, it falls
   * through to transient. */
  GError *err = g_error_new(g_quark_from_static_string("other-domain"), 42, "Some other error");
  assert_int_equal(sk_reconnect_classify_error(err), SK_DISCONNECT_TRANSIENT);
  g_error_free(err);
}

/* ---- Test: reconnect error quark ---------------------------------------- */

static void
test_reconnect_error_quark(void **state)
{
  (void)state;

  GQuark q = sk_reconnect_error_quark();
  assert_true(q != 0);
  assert_int_equal(q, sk_reconnect_error_quark());
}

/* ---- Test: SkTabReconnState enum values --------------------------------- */

static void
test_tab_reconn_state_values(void **state)
{
  (void)state;

  assert_int_equal(SK_TAB_RECONN_IDLE, 0);
  assert_int_not_equal(SK_TAB_RECONN_IDLE, SK_TAB_RECONN_WAITING);
  assert_int_not_equal(SK_TAB_RECONN_WAITING, SK_TAB_RECONN_CONNECTING);
  assert_int_not_equal(SK_TAB_RECONN_CONNECTING, SK_TAB_RECONN_PAUSED);
  assert_int_not_equal(SK_TAB_RECONN_PAUSED, SK_TAB_RECONN_FAILED);
}

/* ---- Test: SkDisconnectClass enum values -------------------------------- */

static void
test_disconnect_class_values(void **state)
{
  (void)state;

  assert_int_not_equal(SK_DISCONNECT_TRANSIENT, SK_DISCONNECT_PERMANENT);
}

/* ---- main --------------------------------------------------------------- */

int
main(void)
{
  const struct CMUnitTest tests[] = {
    cmocka_unit_test(test_backoff_base),
    cmocka_unit_test(test_backoff_exponential),
    cmocka_unit_test(test_backoff_cap),
    cmocka_unit_test(test_backoff_negative_attempt),
    cmocka_unit_test(test_backoff_zero_base),
    cmocka_unit_test(test_backoff_jitter),
    cmocka_unit_test(test_classify_transient),
    cmocka_unit_test(test_classify_permanent),
    cmocka_unit_test(test_classify_non_ssh_domain),
    cmocka_unit_test(test_reconnect_error_quark),
    cmocka_unit_test(test_tab_reconn_state_values),
    cmocka_unit_test(test_disconnect_class_values),
  };

  return cmocka_run_group_tests(tests, NULL, NULL);
}
