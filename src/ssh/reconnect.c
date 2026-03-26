// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file reconnect.c
 * @brief Reconnection engine — connection manager, backoff, NM monitor, UI state.
 *
 * Implements FR-RECONNECT-01..10:
 *
 * - Per-server connection manager coordinating all tab reconnections.
 * - Disconnection detection and error classification (transient vs permanent).
 * - Exponential backoff with jitter: 2/4/8/16/32/60/60..., ±25%.
 * - Coordinated flow: master first, then batch (max 5 simultaneous).
 * - NetworkManager D-Bus monitoring for proactive reconnection.
 * - Per-tab reconnection state for UI overlay.
 *
 * Threading model (INV-IO-1):
 * - All SSH reconnection attempts run in GTask worker threads.
 * - State-change callbacks are dispatched to the GTK main thread via
 *   g_idle_add().
 * - The backoff timer uses g_timeout_add() on the main loop.
 */

#include "shellkeep/sk_reconnect.h"
#include "shellkeep/sk_ssh.h"

#include <math.h>
#include <stdlib.h>
#include <string.h>

/* ------------------------------------------------------------------ */
/*  Constants                                                          */
/* ------------------------------------------------------------------ */

/** FR-RECONNECT-05: default max simultaneous SSH reconnections. */
#define SK_DEFAULT_MAX_CONCURRENT 5

/** FR-RECONNECT-04: default max attempts before pausing. */
#define SK_DEFAULT_MAX_ATTEMPTS 10

/** FR-RECONNECT-06: default backoff base in seconds. */
#define SK_DEFAULT_BACKOFF_BASE 2.0

/** FR-RECONNECT-06: max backoff delay in seconds. */
#define SK_BACKOFF_MAX_DELAY 60.0

/** FR-RECONNECT-06: jitter fraction (±25%). */
#define SK_BACKOFF_JITTER 0.25

/* ------------------------------------------------------------------ */
/*  Error quark                                                        */
/* ------------------------------------------------------------------ */

G_DEFINE_QUARK(sk - reconnect - error - quark, sk_reconnect_error)

/* ------------------------------------------------------------------ */
/*  Exponential backoff  (FR-RECONNECT-06)                             */
/* ------------------------------------------------------------------ */

double
sk_backoff_delay(double base_sec, int attempt)
{
  if (base_sec <= 0.0)
    base_sec = SK_DEFAULT_BACKOFF_BASE;
  if (attempt < 0)
    attempt = 0;

  /* Compute raw delay: base * 2^attempt, capped at 60s. */
  double raw = base_sec * pow(2.0, (double)attempt);
  if (raw > SK_BACKOFF_MAX_DELAY)
    raw = SK_BACKOFF_MAX_DELAY;

  /* Apply jitter: ±25%. */
  double jitter_range = raw * SK_BACKOFF_JITTER;
  /* Random in [-jitter_range, +jitter_range]. */
  double r = ((double)g_random_int() / (double)G_MAXUINT32); /* 0..1 */
  double jitter = (r * 2.0 - 1.0) * jitter_range;

  double delay = raw + jitter;
  if (delay < 0.1)
    delay = 0.1; /* Floor: never less than 100ms. */

  return delay;
}

/* ------------------------------------------------------------------ */
/*  Error classification  (FR-RECONNECT-07)                            */
/* ------------------------------------------------------------------ */

SkDisconnectClass
sk_reconnect_classify_error(const GError *error)
{
  if (error == NULL)
    return SK_DISCONNECT_TRANSIENT;

  /* Permanent errors from the SSH layer — never auto-retry. */
  if (error->domain == SK_SSH_ERROR)
  {
    switch (error->code)
    {
    case SK_SSH_ERROR_AUTH:
      /* FR-RECONNECT-07: AUTH_DENIED — risk of account lockout. */
      return SK_DISCONNECT_PERMANENT;

    case SK_SSH_ERROR_HOST_KEY:
      /* FR-RECONNECT-07: host key mismatch — possible MITM. */
      return SK_DISCONNECT_PERMANENT;

    case SK_SSH_ERROR_PROTOCOL:
      /* Protocol error — something fundamentally wrong. */
      return SK_DISCONNECT_PERMANENT;

    case SK_SSH_ERROR_CRYPTO:
      /* No acceptable algorithms — won't improve with retry. */
      return SK_DISCONNECT_PERMANENT;

    case SK_SSH_ERROR_CONNECT:
    case SK_SSH_ERROR_TIMEOUT:
    case SK_SSH_ERROR_DISCONNECTED:
    case SK_SSH_ERROR_CHANNEL:
    case SK_SSH_ERROR_SFTP:
    default:
      /* Transient: timeout, reset, unreachable, channel errors. */
      return SK_DISCONNECT_TRANSIENT;
    }
  }

  /* Unknown error domain — treat as transient to be safe. */
  return SK_DISCONNECT_TRANSIENT;
}

/* ------------------------------------------------------------------ */
/*  Per-tab reconnection handle                                        */
/* ------------------------------------------------------------------ */

struct _SkReconnHandle
{
  SkConnManager *mgr;    /* Owning manager (back-ptr).   */
  SkSshConnection *conn; /* The SSH connection to manage. */
  gboolean is_master;    /* Master/control connection?    */

  /* Callbacks. */
  SkReconnConnectCb connect_cb;
  SkReconnStateChangedCb state_cb;
  gpointer user_data;

  /* Reconnection state (protected by mgr->lock). */
  SkTabReconnState state;
  int attempt; /* 0-based attempt counter.     */
  double next_retry_sec;
  char *message; /* g_strdup'd status message.   */

  /* Timer source ID for backoff wait (0 = not scheduled). */
  guint timer_id;

  /* GCancellable for in-flight reconnection attempt. */
  GCancellable *cancellable;

  /* Linked list within the manager. */
  GList *link; /* Points to this handle's node. */
};

/* ------------------------------------------------------------------ */
/*  Per-server connection manager                                      */
/* ------------------------------------------------------------------ */

struct _SkConnManager
{
  char *hostname;
  int port;
  char *username;

  int max_concurrent;  /* FR-RECONNECT-05: max simultaneous conns. */
  int max_attempts;    /* FR-RECONNECT-04: max retries per tab.    */
  double backoff_base; /* FR-RECONNECT-06: base delay in seconds.  */

  GMutex lock; /* Protects all mutable state below.        */

  GList *handles;   /* All registered SkReconnHandle*.          */
  int active_count; /* Number of in-flight reconnection tasks.  */

  gboolean reconnecting;     /* TRUE when a reconnection cycle is active. */
  gboolean master_connected; /* TRUE after master reconnects successfully. */
};

/* Forward declarations. */
static void reconn_schedule_next(SkConnManager *mgr);
static void reconn_attempt_tab(SkConnManager *mgr, SkReconnHandle *handle);
static void reconn_update_state(SkReconnHandle *handle, SkTabReconnState new_state,
                                const char *message);
static void schedule_backoff_timer(SkConnManager *mgr, SkReconnHandle *handle);

/* ------------------------------------------------------------------ */
/*  Helpers: UI state dispatch to main thread                          */
/* ------------------------------------------------------------------ */

typedef struct
{
  SkReconnHandle *handle;
  SkTabReconnInfo info;
  SkReconnStateChangedCb callback;
  gpointer user_data;
} StateChangedIdle;

static gboolean
state_changed_idle_cb(gpointer data)
{
  StateChangedIdle *idle = data;
  if (idle->callback != NULL)
  {
    idle->callback(idle->handle, &idle->info, idle->user_data);
  }
  g_free((char *)idle->info.message);
  g_free(idle);
  return G_SOURCE_REMOVE;
}

/**
 * Dispatch a state-change notification to the main thread.
 * Must be called with mgr->lock held; copies all data needed.
 */
static void
dispatch_state_changed(SkReconnHandle *handle)
{
  if (handle->state_cb == NULL)
    return;

  StateChangedIdle *idle = g_new0(StateChangedIdle, 1);
  idle->handle = handle;
  idle->callback = handle->state_cb;
  idle->user_data = handle->user_data;

  idle->info.state = handle->state;
  idle->info.attempt = handle->attempt + 1; /* 1-based for UI. */
  idle->info.max_attempts = handle->mgr->max_attempts;
  idle->info.next_retry_sec = handle->next_retry_sec;
  idle->info.message = g_strdup(handle->message);

  g_idle_add(state_changed_idle_cb, idle);
}

/**
 * Update a handle's state, message, and notify the UI.
 * Must be called with mgr->lock held.
 */
static void
reconn_update_state(SkReconnHandle *handle, SkTabReconnState new_state, const char *message)
{
  handle->state = new_state;
  g_free(handle->message);
  handle->message = g_strdup(message);
  dispatch_state_changed(handle);
}

/* ------------------------------------------------------------------ */
/*  Connection manager lifecycle                                       */
/* ------------------------------------------------------------------ */

SkConnManager *
sk_conn_manager_new(const char *hostname, int port, const char *username, int max_concurrent,
                    int max_attempts, double backoff_base)
{
  g_return_val_if_fail(hostname != NULL, NULL);

  SkConnManager *mgr = g_new0(SkConnManager, 1);
  g_mutex_init(&mgr->lock);

  mgr->hostname = g_strdup(hostname);
  mgr->port = port > 0 ? port : 22;
  mgr->username = g_strdup(username); /* NULL-safe via g_strdup. */

  mgr->max_concurrent = max_concurrent > 0 ? max_concurrent : SK_DEFAULT_MAX_CONCURRENT;
  mgr->max_attempts = max_attempts > 0 ? max_attempts : SK_DEFAULT_MAX_ATTEMPTS;
  mgr->backoff_base = backoff_base > 0.0 ? backoff_base : SK_DEFAULT_BACKOFF_BASE;

  return mgr;
}

void
sk_conn_manager_free(SkConnManager *mgr)
{
  if (mgr == NULL)
    return;

  g_mutex_lock(&mgr->lock);

  /* Cancel all pending timers and in-flight tasks. */
  for (GList *l = mgr->handles; l != NULL; l = l->next)
  {
    SkReconnHandle *h = l->data;
    if (h->timer_id > 0)
    {
      g_source_remove(h->timer_id);
      h->timer_id = 0;
    }
    if (h->cancellable != NULL)
    {
      g_cancellable_cancel(h->cancellable);
      g_object_unref(h->cancellable);
      h->cancellable = NULL;
    }
    g_free(h->message);
    g_free(h);
  }
  g_list_free(mgr->handles);
  mgr->handles = NULL;

  g_mutex_unlock(&mgr->lock);
  g_mutex_clear(&mgr->lock);

  g_free(mgr->hostname);
  g_free(mgr->username);
  g_free(mgr);
}

/* ------------------------------------------------------------------ */
/*  Tab registration                                                   */
/* ------------------------------------------------------------------ */

SkReconnHandle *
sk_conn_manager_register(SkConnManager *mgr, SkSshConnection *conn, gboolean is_master,
                         SkReconnConnectCb connect_cb, SkReconnStateChangedCb state_cb,
                         gpointer user_data)
{
  g_return_val_if_fail(mgr != NULL, NULL);
  g_return_val_if_fail(conn != NULL, NULL);
  g_return_val_if_fail(connect_cb != NULL, NULL);

  SkReconnHandle *handle = g_new0(SkReconnHandle, 1);
  handle->mgr = mgr;
  handle->conn = conn;
  handle->is_master = is_master;
  handle->connect_cb = connect_cb;
  handle->state_cb = state_cb;
  handle->user_data = user_data;
  handle->state = SK_TAB_RECONN_IDLE;
  handle->message = g_strdup("Connected");

  g_mutex_lock(&mgr->lock);
  mgr->handles = g_list_append(mgr->handles, handle);
  handle->link = g_list_last(mgr->handles);
  g_mutex_unlock(&mgr->lock);

  return handle;
}

void
sk_conn_manager_unregister(SkConnManager *mgr, SkReconnHandle *handle)
{
  if (mgr == NULL || handle == NULL)
    return;

  g_mutex_lock(&mgr->lock);

  /* Cancel any pending timer. */
  if (handle->timer_id > 0)
  {
    g_source_remove(handle->timer_id);
    handle->timer_id = 0;
  }

  /* Cancel any in-flight attempt. */
  if (handle->cancellable != NULL)
  {
    g_cancellable_cancel(handle->cancellable);
    g_object_unref(handle->cancellable);
    handle->cancellable = NULL;
  }

  mgr->handles = g_list_remove(mgr->handles, handle);

  g_mutex_unlock(&mgr->lock);

  g_free(handle->message);
  g_free(handle);
}

/* ------------------------------------------------------------------ */
/*  Query                                                              */
/* ------------------------------------------------------------------ */

void
sk_reconn_handle_get_info(SkReconnHandle *handle, SkTabReconnInfo *info)
{
  g_return_if_fail(handle != NULL);
  g_return_if_fail(info != NULL);

  SkConnManager *mgr = handle->mgr;
  g_mutex_lock(&mgr->lock);

  info->state = handle->state;
  info->attempt = handle->attempt + 1;
  info->max_attempts = mgr->max_attempts;
  info->next_retry_sec = handle->next_retry_sec;
  info->message = handle->message;

  g_mutex_unlock(&mgr->lock);
}

SkSshConnection *
sk_reconn_handle_get_connection(SkReconnHandle *handle)
{
  g_return_val_if_fail(handle != NULL, NULL);
  return handle->conn;
}

/* ------------------------------------------------------------------ */
/*  GTask reconnection worker  (INV-IO-1: never block main thread)     */
/* ------------------------------------------------------------------ */

typedef struct
{
  SkConnManager *mgr;
  SkReconnHandle *handle;
} ReconnTaskData;

static void
reconn_task_thread(GTask *task, gpointer source_object, gpointer task_data,
                   GCancellable *cancellable)
{
  (void)source_object;
  (void)cancellable;

  ReconnTaskData *td = task_data;
  SkReconnHandle *handle = td->handle;
  GError *error = NULL;

  /* Invoke the caller's reconnection callback (blocking). */
  gboolean ok = handle->connect_cb(handle->conn, handle->user_data, &error);
  if (ok)
  {
    g_task_return_boolean(task, TRUE);
  }
  else
  {
    g_task_return_error(task, error);
  }
}

/**
 * Callback on main thread when a reconnection GTask completes.
 */
static void
reconn_task_done(GObject *source_object, GAsyncResult *result, gpointer user_data)
{
  (void)source_object;

  ReconnTaskData *td = user_data;
  SkConnManager *mgr = td->mgr;
  SkReconnHandle *handle = td->handle;

  GError *error = NULL;
  gboolean ok = g_task_propagate_boolean(G_TASK(result), &error);

  g_mutex_lock(&mgr->lock);

  /* Decrement active count. */
  if (mgr->active_count > 0)
    mgr->active_count--;

  /* Clear cancellable. */
  if (handle->cancellable != NULL)
  {
    g_object_unref(handle->cancellable);
    handle->cancellable = NULL;
  }

  if (ok)
  {
    /* FR-RECONNECT-03: success — reattached to tmux session. */
    handle->attempt = 0;
    handle->next_retry_sec = 0.0;
    reconn_update_state(handle, SK_TAB_RECONN_IDLE, "Connected");

    if (handle->is_master)
    {
      mgr->master_connected = TRUE;
    }

    /* Schedule next batch of reconnections. */
    reconn_schedule_next(mgr);
  }
  else
  {
    /* Classify the error. */
    SkDisconnectClass cls = sk_reconnect_classify_error(error);

    if (cls == SK_DISCONNECT_PERMANENT)
    {
      /* FR-RECONNECT-07: permanent — stop and notify. */
      g_autofree char *msg = g_strdup_printf("Connection failed: %s", error->message);
      reconn_update_state(handle, SK_TAB_RECONN_FAILED, msg);

      if (handle->is_master)
      {
        /* If master fails permanently, fail all others too. */
        for (GList *l = mgr->handles; l != NULL; l = l->next)
        {
          SkReconnHandle *h = l->data;
          if (h != handle && h->state != SK_TAB_RECONN_IDLE)
          {
            reconn_update_state(h, SK_TAB_RECONN_FAILED, "Server connection failed permanently");
          }
        }
        mgr->reconnecting = FALSE;
      }
    }
    else
    {
      /* Transient error — schedule retry with backoff. */
      handle->attempt++;

      if (handle->attempt >= mgr->max_attempts)
      {
        /* FR-RECONNECT-04: pause after N failures. */
        g_autofree char *msg = g_strdup_printf("Reconnection paused after %d attempts. "
                                               "Last error: %s",
                                               handle->attempt, error->message);
        reconn_update_state(handle, SK_TAB_RECONN_PAUSED, msg);
      }
      else
      {
        /* FR-RECONNECT-06: schedule next attempt with backoff. */
        double delay = sk_backoff_delay(mgr->backoff_base, handle->attempt);
        handle->next_retry_sec = delay;

        g_autofree char *msg = g_strdup_printf("Reconnecting... attempt %d/%d, next in %.0fs",
                                               handle->attempt + 1, mgr->max_attempts, delay);
        reconn_update_state(handle, SK_TAB_RECONN_WAITING, msg);

        /* Schedule backoff timer for next attempt. */
        schedule_backoff_timer(mgr, handle);
      }
    }

    /* Schedule next batch regardless (other tabs may be waiting). */
    reconn_schedule_next(mgr);

    g_error_free(error);
  }

  g_mutex_unlock(&mgr->lock);
  g_free(td);
}

/* ------------------------------------------------------------------ */
/*  Backoff timer callback                                             */
/* ------------------------------------------------------------------ */

typedef struct
{
  SkConnManager *mgr;
  SkReconnHandle *handle;
} BackoffTimerData;

static gboolean
backoff_timer_cb(gpointer data)
{
  BackoffTimerData *bt = data;
  SkConnManager *mgr = bt->mgr;
  SkReconnHandle *handle = bt->handle;

  g_mutex_lock(&mgr->lock);

  handle->timer_id = 0;
  handle->next_retry_sec = 0.0;

  /* If still in WAITING state, attempt reconnection. */
  if (handle->state == SK_TAB_RECONN_WAITING)
  {
    reconn_schedule_next(mgr);
  }

  g_mutex_unlock(&mgr->lock);
  g_free(bt);

  return G_SOURCE_REMOVE;
}

/**
 * Schedule a backoff timer for a handle.
 * Must be called with mgr->lock held.  Timer fires on the main loop.
 */
static void
schedule_backoff_timer(SkConnManager *mgr, SkReconnHandle *handle)
{
  if (handle->timer_id > 0)
    return; /* Already scheduled. */

  double delay = handle->next_retry_sec;
  if (delay <= 0.0)
    delay = 0.1;

  BackoffTimerData *bt = g_new0(BackoffTimerData, 1);
  bt->mgr = mgr;
  bt->handle = handle;

  guint ms = (guint)(delay * 1000.0);
  handle->timer_id = g_timeout_add(ms, backoff_timer_cb, bt);
}

/* ------------------------------------------------------------------ */
/*  Coordinated reconnection flow  (FR-RECONNECT-05)                   */
/* ------------------------------------------------------------------ */

/**
 * Start a reconnection attempt for a single tab.
 * Must be called with mgr->lock held.
 */
static void
reconn_attempt_tab(SkConnManager *mgr, SkReconnHandle *handle)
{
  g_assert(mgr->active_count < mgr->max_concurrent);

  mgr->active_count++;

  g_autofree char *msg =
      g_strdup_printf("Reconnecting... attempt %d/%d", handle->attempt + 1, mgr->max_attempts);
  reconn_update_state(handle, SK_TAB_RECONN_CONNECTING, msg);

  /* Create cancellable for this attempt. */
  handle->cancellable = g_cancellable_new();

  ReconnTaskData *td = g_new0(ReconnTaskData, 1);
  td->mgr = mgr;
  td->handle = handle;

  /* INV-IO-1: run blocking reconnection in a worker thread. */
  GTask *task = g_task_new(NULL, handle->cancellable, reconn_task_done, td);
  g_task_set_task_data(task, td, NULL); /* freed in reconn_task_done */
  g_task_run_in_thread(task, reconn_task_thread);
  g_object_unref(task);
}

/**
 * Schedule the next batch of reconnections.
 * Implements the master-first strategy from FR-RECONNECT-05.
 *
 * Must be called with mgr->lock held.
 */
static void
reconn_schedule_next(SkConnManager *mgr)
{
  if (!mgr->reconnecting)
    return;

  /* Phase 1: if master is not yet connected, reconnect master first. */
  if (!mgr->master_connected)
  {
    for (GList *l = mgr->handles; l != NULL; l = l->next)
    {
      SkReconnHandle *h = l->data;
      if (!h->is_master)
        continue;

      if (h->state == SK_TAB_RECONN_WAITING && h->timer_id == 0 &&
          mgr->active_count < mgr->max_concurrent)
      {
        reconn_attempt_tab(mgr, h);
        return; /* Wait for master to complete. */
      }

      if (h->state == SK_TAB_RECONN_WAITING && h->timer_id > 0)
      {
        return; /* Master is in backoff; wait for timer. */
      }

      if (h->state == SK_TAB_RECONN_CONNECTING)
      {
        return; /* Master attempt in progress; wait. */
      }

      /* Master is IDLE (connected), PAUSED, or FAILED — move on. */
      break;
    }
  }

  /* Phase 2: batch reconnect non-master tabs (up to max_concurrent). */
  for (GList *l = mgr->handles; l != NULL; l = l->next)
  {
    if (mgr->active_count >= mgr->max_concurrent)
      break;

    SkReconnHandle *h = l->data;

    if (h->state == SK_TAB_RECONN_WAITING && h->timer_id == 0)
    {
      /* Ready for attempt — not waiting on backoff timer. */
      reconn_attempt_tab(mgr, h);
    }
    else if (h->state == SK_TAB_RECONN_WAITING && h->timer_id > 0)
    {
      /* In backoff — timer will trigger reconn_schedule_next. */
      continue;
    }
  }

  /* Check if all handles are done (IDLE, PAUSED, or FAILED). */
  gboolean all_done = TRUE;
  for (GList *l = mgr->handles; l != NULL; l = l->next)
  {
    SkReconnHandle *h = l->data;
    if (h->state == SK_TAB_RECONN_WAITING || h->state == SK_TAB_RECONN_CONNECTING)
    {
      all_done = FALSE;
      break;
    }
  }
  if (all_done)
  {
    mgr->reconnecting = FALSE;
  }
}

/**
 * Begin a reconnection cycle for all disconnected tabs.
 * Must be called with mgr->lock held.
 */
static void
begin_reconnection_cycle(SkConnManager *mgr)
{
  if (mgr->reconnecting)
    return; /* Already in a cycle. */

  mgr->reconnecting = TRUE;
  mgr->master_connected = FALSE;

  /* Mark all non-idle handles as WAITING with initial backoff. */
  for (GList *l = mgr->handles; l != NULL; l = l->next)
  {
    SkReconnHandle *h = l->data;

    if (h->state != SK_TAB_RECONN_IDLE)
      continue; /* Already in a reconnection state. */

    /* Check if connection is actually down. */
    if (sk_ssh_connection_is_connected(h->conn))
      continue;

    h->attempt = 0;
    h->next_retry_sec = 0.0;

    /* FR-RECONNECT-02: per-tab spinner message. */
    reconn_update_state(h, SK_TAB_RECONN_WAITING, "Reconnecting... attempt 1");
  }

  reconn_schedule_next(mgr);
}

/* ------------------------------------------------------------------ */
/*  Reconnection triggers                                              */
/* ------------------------------------------------------------------ */

/* FR-RECONNECT-01 */
void
sk_conn_manager_notify_disconnected(SkConnManager *mgr, SkReconnHandle *handle)
{
  g_return_if_fail(mgr != NULL);
  g_return_if_fail(handle != NULL);

  g_mutex_lock(&mgr->lock);

  /* Only transition from IDLE. */
  if (handle->state == SK_TAB_RECONN_IDLE)
  {
    handle->attempt = 0;
    handle->next_retry_sec = 0.0;
    reconn_update_state(handle, SK_TAB_RECONN_WAITING, "Reconnecting... attempt 1");
  }

  if (!mgr->reconnecting)
  {
    mgr->reconnecting = TRUE;
    mgr->master_connected = FALSE;

    /* Check if the master is still alive. */
    for (GList *l = mgr->handles; l != NULL; l = l->next)
    {
      SkReconnHandle *h = l->data;
      if (h->is_master && sk_ssh_connection_is_connected(h->conn))
      {
        mgr->master_connected = TRUE;
        break;
      }
    }
  }

  reconn_schedule_next(mgr);

  g_mutex_unlock(&mgr->lock);
}

/* FR-RECONNECT-08 */
void
sk_conn_manager_notify_network_changed(SkConnManager *mgr)
{
  g_return_if_fail(mgr != NULL);

  g_mutex_lock(&mgr->lock);

  /*
   * Network change detected (e.g. IP changed, interface flap).
   * Assume ALL SSH connections are now invalid.
   * Cancel any in-flight attempts and restart the cycle.
   */

  /* Cancel active attempts. */
  for (GList *l = mgr->handles; l != NULL; l = l->next)
  {
    SkReconnHandle *h = l->data;

    /* Cancel in-flight task. */
    if (h->cancellable != NULL)
    {
      g_cancellable_cancel(h->cancellable);
      g_object_unref(h->cancellable);
      h->cancellable = NULL;
    }

    /* Cancel backoff timer. */
    if (h->timer_id > 0)
    {
      g_source_remove(h->timer_id);
      h->timer_id = 0;
    }

    /* Reset to IDLE so begin_reconnection_cycle picks them up. */
    h->state = SK_TAB_RECONN_IDLE;
    h->attempt = 0;
  }

  mgr->reconnecting = FALSE;
  mgr->active_count = 0;

  /* Start fresh reconnection cycle. */
  begin_reconnection_cycle(mgr);

  g_mutex_unlock(&mgr->lock);
}

/* FR-RECONNECT-04 */
void
sk_conn_manager_retry(SkConnManager *mgr, SkReconnHandle *handle)
{
  g_return_if_fail(mgr != NULL);
  g_return_if_fail(handle != NULL);

  g_mutex_lock(&mgr->lock);

  if (handle->state == SK_TAB_RECONN_PAUSED || handle->state == SK_TAB_RECONN_FAILED)
  {
    /* Reset attempt counter and resume. */
    handle->attempt = 0;
    handle->next_retry_sec = 0.0;
    reconn_update_state(handle, SK_TAB_RECONN_WAITING, "Reconnecting... attempt 1");

    if (!mgr->reconnecting)
    {
      mgr->reconnecting = TRUE;
      mgr->master_connected = FALSE;
      /* Check master status. */
      for (GList *l = mgr->handles; l != NULL; l = l->next)
      {
        SkReconnHandle *h = l->data;
        if (h->is_master && h->state == SK_TAB_RECONN_IDLE)
        {
          mgr->master_connected = TRUE;
          break;
        }
      }
    }

    reconn_schedule_next(mgr);
  }

  g_mutex_unlock(&mgr->lock);
}

void
sk_conn_manager_discard(SkConnManager *mgr, SkReconnHandle *handle)
{
  g_return_if_fail(mgr != NULL);
  g_return_if_fail(handle != NULL);

  g_mutex_lock(&mgr->lock);

  /* Cancel timer if any. */
  if (handle->timer_id > 0)
  {
    g_source_remove(handle->timer_id);
    handle->timer_id = 0;
  }

  /* Cancel in-flight task. */
  if (handle->cancellable != NULL)
  {
    g_cancellable_cancel(handle->cancellable);
    g_object_unref(handle->cancellable);
    handle->cancellable = NULL;
  }

  reconn_update_state(handle, SK_TAB_RECONN_FAILED, "Reconnection discarded by user");

  reconn_schedule_next(mgr);

  g_mutex_unlock(&mgr->lock);
}

/* ------------------------------------------------------------------ */
/*  NetworkManager D-Bus monitor  (FR-RECONNECT-08)                    */
/* ------------------------------------------------------------------ */

/*
 * Monitors org.freedesktop.NetworkManager on the system D-Bus for
 * the StateChanged signal. When the network transitions to a connected
 * state after being disconnected (or when the connectivity changes),
 * we invoke the callback so the connection manager can trigger
 * proactive reconnection.
 *
 * Falls back gracefully if D-Bus or NetworkManager is unavailable.
 */

#define NM_DBUS_SERVICE "org.freedesktop.NetworkManager"
#define NM_DBUS_PATH "/org/freedesktop/NetworkManager"
#define NM_DBUS_INTERFACE "org.freedesktop.NetworkManager"

/* NetworkManager connectivity states (from NM source). */
#define NM_STATE_CONNECTED_GLOBAL 70

struct _SkNetworkMonitor
{
  GDBusConnection *bus;
  guint signal_id;
  SkNetworkChangedCb callback;
  gpointer user_data;
  guint32 last_state;
};

static void
nm_state_changed_cb(GDBusConnection *connection, const gchar *sender_name, const gchar *object_path,
                    const gchar *interface_name, const gchar *signal_name, GVariant *parameters,
                    gpointer user_data)
{
  (void)connection;
  (void)sender_name;
  (void)object_path;
  (void)interface_name;
  (void)signal_name;

  SkNetworkMonitor *mon = user_data;
  if (mon == NULL || mon->callback == NULL)
    return;

  guint32 new_state = 0;
  g_variant_get(parameters, "(u)", &new_state);

  /*
   * Trigger callback when state changes.  We care about transitions
   * that indicate connectivity restored or IP change.
   *
   * Any state change could indicate the network path has changed,
   * so trigger on every transition (the connection manager will
   * decide whether action is needed).
   */
  if (new_state != mon->last_state)
  {
    mon->last_state = new_state;
    mon->callback(mon->user_data);
  }
}

SkNetworkMonitor *
sk_network_monitor_new(SkNetworkChangedCb callback, gpointer user_data)
{
  g_return_val_if_fail(callback != NULL, NULL);

  GError *error = NULL;
  GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SYSTEM, NULL, &error);
  if (bus == NULL)
  {
    /* D-Bus unavailable — non-fatal. */
    g_warning("sk_reconnect: cannot connect to system D-Bus: %s", error->message);
    g_error_free(error);
    return NULL;
  }

  SkNetworkMonitor *mon = g_new0(SkNetworkMonitor, 1);
  mon->bus = bus;
  mon->callback = callback;
  mon->user_data = user_data;

  /* Subscribe to NM StateChanged signal. */
  mon->signal_id = g_dbus_connection_signal_subscribe(bus, NM_DBUS_SERVICE, /* sender */
                                                      NM_DBUS_INTERFACE,    /* interface */
                                                      "StateChanged",       /* signal name */
                                                      NM_DBUS_PATH,         /* object path */
                                                      NULL,                 /* arg0 (match all) */
                                                      G_DBUS_SIGNAL_FLAGS_NONE, nm_state_changed_cb,
                                                      mon, NULL /* user_data_free_func */
  );

  if (mon->signal_id == 0)
  {
    g_warning("sk_reconnect: failed to subscribe to NM signal");
    g_object_unref(bus);
    g_free(mon);
    return NULL;
  }

  return mon;
}

void
sk_network_monitor_free(SkNetworkMonitor *monitor)
{
  if (monitor == NULL)
    return;

  if (monitor->signal_id > 0 && monitor->bus != NULL)
  {
    g_dbus_connection_signal_unsubscribe(monitor->bus, monitor->signal_id);
  }

  if (monitor->bus != NULL)
  {
    g_object_unref(monitor->bus);
  }

  g_free(monitor);
}
