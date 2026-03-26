// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_connect.h
 * @brief End-to-end connection flow — public API.
 *
 * Integrates SSH, session, state, terminal, and UI layers into the main
 * connection lifecycle: host key verification, authentication, tmux
 * detection, lock acquisition, environment selection, state restoration,
 * dead/orphaned session handling, ProxyJump support, and graceful
 * disconnect/shutdown.
 *
 * Requirements: FR-CONN-16..22, FR-STATE-06..08, FR-HISTORY-04..12,
 *               FR-PROXY-*, FR-SESSION-12
 */

#ifndef SK_CONNECT_H
#define SK_CONNECT_H

#include "shellkeep/sk_types.h"

#include <gtk/gtk.h>

#include <glib.h>

#include <stdbool.h>

#ifdef __cplusplus
extern "C"
{
#endif

  /* ------------------------------------------------------------------ */
  /* Error domain                                                        */
  /* ------------------------------------------------------------------ */

#define SK_CONNECT_ERROR (sk_connect_error_quark())
  GQuark sk_connect_error_quark(void);

  typedef enum
  {
    SK_CONNECT_ERROR_AUTH,          /**< E-CONN-1: authentication failure. */
    SK_CONNECT_ERROR_UNREACHABLE,   /**< E-CONN-2: host unreachable. */
    SK_CONNECT_ERROR_TMUX_MISSING,  /**< E-CONN-3: tmux not found. */
    SK_CONNECT_ERROR_TMUX_VERSION,  /**< E-CONN-4: tmux too old. */
    SK_CONNECT_ERROR_STATE_CORRUPT, /**< E-CONN-5: corrupted state. */
    SK_CONNECT_ERROR_HOST_KEY,      /**< E-CONN-6: host key changed. */
    SK_CONNECT_ERROR_HOST_UNKNOWN,  /**< E-CONN-7: unknown host (TOFU). */
    SK_CONNECT_ERROR_SFTP,          /**< E-CONN-8: SFTP unavailable. */
    SK_CONNECT_ERROR_LOCK,          /**< Lock acquisition failed. */
    SK_CONNECT_ERROR_CANCELLED,     /**< User cancelled at some phase. */
    SK_CONNECT_ERROR_PROXY,         /**< ProxyJump hop failure. */
  } SkConnectErrorCode;

  /* ------------------------------------------------------------------ */
  /* Connection parameters                                               */
  /* ------------------------------------------------------------------ */

  /** Parameters for a connection flow. */
  typedef struct
  {
    const char *hostname;      /**< Remote hostname or IP. */
    int port;                  /**< Port (0 = default 22). */
    const char *username;      /**< Remote username (NULL = current). */
    const char *identity_file; /**< Explicit key path, or NULL. */
    const char *proxy_jump;    /**< ProxyJump host or NULL (FR-PROXY-01). */
    GtkApplication *app;       /**< GtkApplication instance. */
    GtkWindow *parent_window;  /**< Parent window for dialogs, or NULL. */
  } SkConnectParams;

  /* ------------------------------------------------------------------ */
  /* Connection context                                                  */
  /* ------------------------------------------------------------------ */

  /** Opaque connection context holding all flow state. */
  typedef struct _SkConnectContext SkConnectContext;

  /* ------------------------------------------------------------------ */
  /* Callbacks                                                           */
  /* ------------------------------------------------------------------ */

  /**
   * Called on the main thread when the connection flow completes.
   *
   * @param ctx       Connection context.
   * @param success   TRUE if the flow completed successfully.
   * @param error     Error details on failure (NULL on success).
   * @param user_data Caller-supplied data.
   */
  typedef void (*SkConnectDoneCb)(SkConnectContext *ctx, bool success, const GError *error,
                                  gpointer user_data);

  /* ------------------------------------------------------------------ */
  /* Lifecycle                                                           */
  /* ------------------------------------------------------------------ */

  /**
   * Start the end-to-end connection flow asynchronously.
   *
   * Phases (FR-CONN-16):
   *  1. Connect (TCP + host key verification)
   *  2. Authenticate
   *  3. Check tmux
   *  4. Acquire lock
   *  5. Load state
   *  6. Select environment
   *  7. Restore sessions (progressive, parallel batches of 5)
   *
   * @param params     Connection parameters (copied internally).
   * @param config     Application configuration (borrowed, must outlive ctx).
   * @param done_cb    Callback when flow completes or fails.
   * @param user_data  Passed to done_cb.
   * @return New connection context, or NULL on immediate failure.
   */
  SkConnectContext *sk_connect_start(const SkConnectParams *params, SkConfig *config,
                                     SkConnectDoneCb done_cb, gpointer user_data);

  /**
   * Initiate graceful disconnect.
   *
   * Lifecycle (FR-LOCK-10, INV-LOCK-2):
   *  1. Save final state to server
   *  2. Release lock
   *  3. Close all SSH channels
   *  4. Disconnect SSH connections
   *
   * @param ctx  Connection context.
   */
  void sk_connect_disconnect(SkConnectContext *ctx);

  /**
   * Free the connection context and all owned resources.
   * Calls sk_connect_disconnect() if still connected.
   *
   * @param ctx  Connection context (may be NULL).
   */
  void sk_connect_free(SkConnectContext *ctx);

  /**
   * Emergency shutdown handler for SIGTERM/SIGINT.
   * Saves state and releases lock synchronously (best effort).
   * Must only be called from signal context or shutdown path.
   *
   * @param ctx  Connection context (may be NULL).
   */
  void sk_connect_emergency_shutdown(SkConnectContext *ctx);

  /* ------------------------------------------------------------------ */
  /* Queries                                                             */
  /* ------------------------------------------------------------------ */

  /**
   * Get the hostname of the current connection.
   * @return Internal string; do NOT free. NULL if not connected.
   */
  const char *sk_connect_get_hostname(const SkConnectContext *ctx);

  /**
   * Get the active environment name.
   * @return Internal string; do NOT free. NULL if not selected.
   */
  const char *sk_connect_get_environment(const SkConnectContext *ctx);

  /**
   * Get the client-id used for this connection.
   * @return Internal string; do NOT free.
   */
  const char *sk_connect_get_client_id(const SkConnectContext *ctx);

  /**
   * Check whether the connection is currently established.
   */
  bool sk_connect_is_connected(const SkConnectContext *ctx);

#ifdef __cplusplus
}
#endif

#endif /* SK_CONNECT_H */
