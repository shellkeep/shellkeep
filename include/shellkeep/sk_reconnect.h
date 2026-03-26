// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_reconnect.h
 * @brief Reconnection engine for shellkeep.
 *
 * Provides a per-server connection manager that coordinates reconnection
 * of all SSH sessions after a network drop.  Implements exponential backoff
 * with jitter, error classification (transient vs permanent), coordinated
 * master-first reconnection, NetworkManager D-Bus monitoring, and per-tab
 * UI overlay state.
 *
 * Requirements: FR-RECONNECT-01..10
 */

#ifndef SHELLKEEP_SK_RECONNECT_H
#define SHELLKEEP_SK_RECONNECT_H

#include "shellkeep/sk_ssh.h"
#include "shellkeep/sk_types.h"

#include <glib.h>

#include <gio/gio.h>
#include <stdbool.h>

G_BEGIN_DECLS

/* ------------------------------------------------------------------ */
/*  Error domain                                                       */
/* ------------------------------------------------------------------ */

#define SK_RECONNECT_ERROR (sk_reconnect_error_quark())
GQuark sk_reconnect_error_quark(void);

typedef enum
{
  SK_RECONNECT_ERROR_PERMANENT,   /**< Permanent failure; do not retry. */
  SK_RECONNECT_ERROR_TRANSIENT,   /**< Transient failure; will retry.   */
  SK_RECONNECT_ERROR_MAX_RETRIES, /**< Max retries exhausted.           */
  SK_RECONNECT_ERROR_CANCELLED,   /**< User or system cancelled.        */
} SkReconnectErrorCode;

/* ------------------------------------------------------------------ */
/*  Error classification  (FR-RECONNECT-07)                            */
/* ------------------------------------------------------------------ */

/**
 * Classification of SSH disconnection causes.
 * Determines whether auto-retry is safe.
 */
typedef enum
{
  SK_DISCONNECT_TRANSIENT, /**< Timeout, reset, unreachable — auto-retry. */
  SK_DISCONNECT_PERMANENT, /**< Auth denied, host key, protocol — stop.   */
} SkDisconnectClass;

/**
 * Classify a GError from the SSH layer.
 *
 * @param error  The GError from a failed SSH operation.
 * @return SK_DISCONNECT_TRANSIENT or SK_DISCONNECT_PERMANENT.
 */
SkDisconnectClass sk_reconnect_classify_error(const GError *error);

/* ------------------------------------------------------------------ */
/*  Tab reconnection state (for UI overlay)  (FR-RECONNECT-02)         */
/* ------------------------------------------------------------------ */

/** State of a single tab's reconnection process. */
typedef enum
{
  SK_TAB_RECONN_IDLE,       /**< Not reconnecting; connection is live. */
  SK_TAB_RECONN_WAITING,    /**< Waiting for next attempt (backoff).   */
  SK_TAB_RECONN_CONNECTING, /**< Attempt in progress.                  */
  SK_TAB_RECONN_PAUSED,     /**< Max retries hit; waiting for user.    */
  SK_TAB_RECONN_FAILED,     /**< Permanent failure; no retry.          */
} SkTabReconnState;

/** Per-tab reconnection info exposed to the UI overlay. */
typedef struct
{
  SkTabReconnState state;
  int attempt;           /**< Current attempt number (1-based). */
  int max_attempts;      /**< Configured max attempts.         */
  double next_retry_sec; /**< Seconds until next attempt.      */
  const char *message;   /**< Human-readable status message.   */
} SkTabReconnInfo;

/* ------------------------------------------------------------------ */
/*  Opaque types                                                       */
/* ------------------------------------------------------------------ */

/** Per-server connection manager.  Coordinates all tab reconnections. */
typedef struct _SkConnManager SkConnManager;

/** Per-tab reconnection handle within a connection manager. */
typedef struct _SkReconnHandle SkReconnHandle;

/* ------------------------------------------------------------------ */
/*  Callbacks                                                          */
/* ------------------------------------------------------------------ */

/**
 * Called on the main thread when a tab's reconnection state changes.
 * The UI layer uses this to update the overlay spinner.
 *
 * @param handle    The reconnection handle for the tab.
 * @param info      Current reconnection info (valid until next callback).
 * @param user_data Caller-supplied data.
 */
typedef void (*SkReconnStateChangedCb)(SkReconnHandle *handle, const SkTabReconnInfo *info,
                                       gpointer user_data);

/**
 * Called on a GTask worker thread to perform the actual SSH reconnection.
 * The connection manager invokes this for each tab that needs reconnecting.
 *
 * @param conn      The SSH connection to reuse/reconnect.
 * @param user_data Caller-supplied per-tab data.
 * @param error     Return location for a GError.
 * @return TRUE on success (connection re-established and tmux reattached).
 */
typedef gboolean (*SkReconnConnectCb)(SkSshConnection *conn, gpointer user_data, GError **error);

/* ------------------------------------------------------------------ */
/*  Connection manager  (FR-RECONNECT-05)                              */
/* ------------------------------------------------------------------ */

/**
 * Create a per-server connection manager.
 *
 * One manager per unique (hostname, port, username) tuple.
 * Coordinates all tab reconnections to that server, limiting
 * simultaneous SSH connections to max_concurrent (default 5).
 *
 * @param hostname        Server hostname.
 * @param port            Server port (0 = 22).
 * @param username        Username (may be NULL for current user).
 * @param max_concurrent  Max simultaneous reconnections (0 = default 5).
 * @param max_attempts    Max retry attempts per tab (0 = from config).
 * @param backoff_base    Backoff base in seconds (0 = default 2.0).
 * @return New connection manager (never NULL).
 */
SkConnManager *sk_conn_manager_new(const char *hostname, int port, const char *username,
                                   int max_concurrent, int max_attempts, double backoff_base);

/**
 * Free a connection manager and cancel all pending reconnections.
 *
 * @param mgr  Manager to free (may be NULL).
 */
void sk_conn_manager_free(SkConnManager *mgr);

/* ------------------------------------------------------------------ */
/*  Tab registration                                                   */
/* ------------------------------------------------------------------ */

/**
 * Register a tab's SSH connection for managed reconnection.
 *
 * @param mgr         Connection manager.
 * @param conn        The tab's SSH connection handle.
 * @param is_master   TRUE if this is the master/control connection.
 * @param connect_cb  Callback to perform the actual reconnection.
 * @param state_cb    Callback for UI overlay state changes (may be NULL).
 * @param user_data   Passed to both callbacks.
 * @return Reconnection handle for this tab.
 */
SkReconnHandle *sk_conn_manager_register(SkConnManager *mgr, SkSshConnection *conn,
                                         gboolean is_master, SkReconnConnectCb connect_cb,
                                         SkReconnStateChangedCb state_cb, gpointer user_data);

/**
 * Unregister a tab from managed reconnection.
 *
 * @param mgr     Connection manager.
 * @param handle  Handle to unregister (freed after this call).
 */
void sk_conn_manager_unregister(SkConnManager *mgr, SkReconnHandle *handle);

/* ------------------------------------------------------------------ */
/*  Reconnection triggers                                              */
/* ------------------------------------------------------------------ */

/**
 * Notify the manager that a specific tab's connection has been lost.
 * Triggers the coordinated reconnection flow.
 *
 * FR-RECONNECT-01: keepalive timeout detected.
 *
 * @param mgr     Connection manager.
 * @param handle  Handle of the disconnected tab.
 */
void sk_conn_manager_notify_disconnected(SkConnManager *mgr, SkReconnHandle *handle);

/**
 * Notify the manager that the network has changed (e.g. NM signal).
 * Invalidates all connections and triggers proactive reconnection.
 *
 * FR-RECONNECT-08: network change detection.
 *
 * @param mgr  Connection manager.
 */
void sk_conn_manager_notify_network_changed(SkConnManager *mgr);

/**
 * User clicked "Try again" after max retries.
 * Resets attempt counter and resumes reconnection for the tab.
 *
 * FR-RECONNECT-04: user retry after pause.
 *
 * @param mgr     Connection manager.
 * @param handle  Handle of the paused tab.
 */
void sk_conn_manager_retry(SkConnManager *mgr, SkReconnHandle *handle);

/**
 * User clicked "Discard" — stop reconnecting this tab.
 *
 * @param mgr     Connection manager.
 * @param handle  Handle to discard.
 */
void sk_conn_manager_discard(SkConnManager *mgr, SkReconnHandle *handle);

/* ------------------------------------------------------------------ */
/*  Query                                                              */
/* ------------------------------------------------------------------ */

/**
 * Get the current reconnection info for a tab.
 *
 * @param handle  Reconnection handle.
 * @param info    Output structure (populated on return).
 */
void sk_reconn_handle_get_info(SkReconnHandle *handle, SkTabReconnInfo *info);

/**
 * Get the SSH connection associated with a handle.
 *
 * @param handle  Reconnection handle.
 * @return The SSH connection (owned by the caller who registered it).
 */
SkSshConnection *sk_reconn_handle_get_connection(SkReconnHandle *handle);

/* ------------------------------------------------------------------ */
/*  NetworkManager D-Bus monitor  (FR-RECONNECT-08)                    */
/* ------------------------------------------------------------------ */

/** Opaque NetworkManager monitor handle. */
typedef struct _SkNetworkMonitor SkNetworkMonitor;

/**
 * Callback invoked on the main thread when the network state changes.
 *
 * @param user_data  Caller-supplied pointer.
 */
typedef void (*SkNetworkChangedCb)(gpointer user_data);

/**
 * Start monitoring NetworkManager for connectivity changes.
 * Listens to org.freedesktop.NetworkManager on D-Bus.
 *
 * @param callback   Called when IP or interface changes are detected.
 * @param user_data  Passed to callback.
 * @return Monitor handle, or NULL if D-Bus unavailable (non-fatal).
 */
SkNetworkMonitor *sk_network_monitor_new(SkNetworkChangedCb callback, gpointer user_data);

/**
 * Stop monitoring and free resources.
 *
 * @param monitor  Handle to free (may be NULL).
 */
void sk_network_monitor_free(SkNetworkMonitor *monitor);

/* ------------------------------------------------------------------ */
/*  Exponential backoff  (FR-RECONNECT-06)                             */
/* ------------------------------------------------------------------ */

/**
 * Compute the next backoff delay.
 *
 * Sequence: base, base*2, base*4, ..., max(base*2^n, 60), capped at 60s.
 * Jitter: +/-25% per call.
 *
 * @param base_sec  Base delay in seconds (e.g. 2.0).
 * @param attempt   Attempt number (0-based: 0 => base_sec delay).
 * @return Delay in seconds with jitter applied.
 */
double sk_backoff_delay(double base_sec, int attempt);

G_END_DECLS

#endif /* SHELLKEEP_SK_RECONNECT_H */
