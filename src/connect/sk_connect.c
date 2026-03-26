// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_connect.c
 * @brief End-to-end connection flow integrating all shellkeep components.
 *
 * This module implements the main connection lifecycle:
 *
 *  1. TCP connect + host key verification (FR-CONN-01..05)
 *  2. Authentication (FR-CONN-06..12)
 *  3. tmux detection (FR-CONN-13..15)
 *  4. Lock acquisition (FR-LOCK-*)
 *  5. State loading via SFTP/shell fallback (FR-STATE-06..08, FR-CONN-20)
 *  6. Environment selection (FR-ENV-03..05)
 *  7. Session reconciliation + restore (FR-STATE-09..11, FR-SESSION-12)
 *  8. Dead session handling (FR-HISTORY-04..12)
 *  9. ProxyJump support (FR-PROXY-*)
 * 10. Graceful disconnect / shutdown (FR-LOCK-10, INV-LOCK-2)
 *
 * Threading model (INV-IO-1): All blocking SSH/SFTP/tmux operations run in
 * GTask worker threads.  UI callbacks are dispatched to the main thread via
 * g_idle_add().
 *
 * Error handling follows table E-CONN-1 through E-CONN-8.
 */

#include "shellkeep/sk_config.h"
#include "shellkeep/sk_connect.h"
#include "shellkeep/sk_log.h"
#include "shellkeep/sk_reconnect.h"
#include "shellkeep/sk_session.h"
#include "shellkeep/sk_ssh.h"
#include "shellkeep/sk_state.h"
#include "shellkeep/sk_terminal.h"
#include "shellkeep/sk_types.h"
#include "shellkeep/sk_ui.h"

#include <gtk/gtk.h>

#include <glib-unix.h>
#include <glib.h>

#include <signal.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* ------------------------------------------------------------------ */
/* Constants                                                           */
/* ------------------------------------------------------------------ */

/** Maximum parallel SSH connections during restore (FR-STATE-11). */
#define SK_RESTORE_BATCH_SIZE 5

/** Remote state file path template (FR-STATE-01). */
#define SK_REMOTE_STATE_PATH "~/.terminal-state/%s.json"

/** Remote state directory (FR-STATE-01). */
#define SK_REMOTE_STATE_DIR "~/.terminal-state"

/* ------------------------------------------------------------------ */
/* Error domain                                                        */
/* ------------------------------------------------------------------ */

G_DEFINE_QUARK(sk - connect - error - quark, sk_connect_error)

/* ------------------------------------------------------------------ */
/* Internal types                                                      */
/* ------------------------------------------------------------------ */

/** Per-tab restore context for parallel restoration. */
typedef struct
{
  SkConnectContext *ctx;       /**< Back-pointer to parent context. */
  SkTab *state_tab;            /**< State tab definition (borrowed). */
  SkWindow *state_window;      /**< State window (borrowed). */
  SkSshConnection *ssh_conn;   /**< Per-tab SSH connection (owned). */
  SkSshChannel *ssh_channel;   /**< Per-tab SSH channel (owned). */
  SkTerminalTab *terminal;     /**< Terminal tab widget (owned). */
  SkAppWindow *app_window;     /**< UI window this tab belongs to. */
  SkAppTab *app_tab;           /**< UI tab handle. */
  SkTmuxSession *tmux_session; /**< Tmux session handle (owned). */
  char *session_uuid;          /**< Session UUID (owned). */
  char *tmux_name;             /**< Full tmux session name (owned). */
  bool is_dead;                /**< TRUE if session was detected dead. */
  int index;                   /**< Restore order index. */
} SkTabRestore;

/** Connection context holding the full flow state. */
struct _SkConnectContext
{
  /* Parameters (owned copies). */
  char *hostname;
  int port;
  char *username;
  char *identity_file;
  char *proxy_jump;         /* FR-PROXY-01 */
  GtkApplication *app;      /* Borrowed. */
  GtkWindow *parent_window; /* Borrowed. */
  SkConfig *config;         /* Borrowed. */

  /* Resolved identifiers. */
  char *client_id;
  char *host_fingerprint;
  char *environment; /* Active environment name (owned). */

  /* SSH control connection (for SFTP, tmux control mode). */
  SkSshConnection *control_conn;
  SkSftpSession *sftp; /* NULL if SFTP unavailable. */
  bool sftp_warned;    /* TRUE once E-CONN-8 warning shown. */

  /* Session layer. */
  SkSessionManager *session_mgr;

  /* State. */
  SkStateFile *state;        /* Server state (owned). */
  SkStateDebounce *debounce; /* Debounced state writer. */

  /* UI. */
  SkConnFeedback *feedback; /* Connection progress overlay. */
  GPtrArray *app_windows;   /* Array of SkAppWindow* (owned). */
  GPtrArray *tab_restores;  /* Array of SkTabRestore* (owned). */

  /* Reconnection. */
  SkConnManager *reconn_mgr;

  /* Lifecycle. */
  bool connected;
  bool shutting_down;
  GCancellable *cancellable;

  /* Callbacks. */
  SkConnectDoneCb done_cb;
  gpointer done_user_data;

  /* Signal handlers. */
  gulong sigterm_handler_id;
  gulong sigint_handler_id;
};

/* ------------------------------------------------------------------ */
/* Forward declarations — internal helpers                             */
/* ------------------------------------------------------------------ */

static void connect_phase_1_connect(GTask *task, gpointer src, gpointer data, GCancellable *cancel);
static void on_connect_done(GObject *src, GAsyncResult *res, gpointer data);
static void connect_phase_2_post_auth(SkConnectContext *ctx);
static void phase_tmux_detect_async(SkConnectContext *ctx);
static void on_tmux_detect_done(GObject *src, GAsyncResult *res, gpointer data);
static void phase_lock_acquire(SkConnectContext *ctx);
static void on_lock_done(GObject *src, GAsyncResult *res, gpointer data);
static void phase_load_state(SkConnectContext *ctx);
static void on_state_loaded(GObject *src, GAsyncResult *res, gpointer data);
static void phase_env_select(SkConnectContext *ctx);
static void phase_restore_sessions(SkConnectContext *ctx);
static void restore_batch(SkConnectContext *ctx, int start_index);
static void on_tab_restore_done(GObject *src, GAsyncResult *res, gpointer data);
static void tab_restore_worker(GTask *task, gpointer src, gpointer data, GCancellable *cancel);
static void handle_dead_sessions(SkConnectContext *ctx, SkReconcileResult *reconcile);
static void handle_orphaned_sessions(SkConnectContext *ctx, SkReconcileResult *reconcile);
static void flow_complete(SkConnectContext *ctx, bool success, GError *error);
static void flow_fail(SkConnectContext *ctx, GError *error);

/* Helpers. */
static SkSshOptions build_ssh_options(SkConnectContext *ctx);
static char *build_remote_state_path(const char *client_id);
static SkStateFile *load_state_from_server(SkConnectContext *ctx, GError **error);
static bool save_state_to_server(SkConnectContext *ctx, GError **error);
static char *read_remote_file(SkConnectContext *ctx, const char *path, size_t *out_len,
                              GError **error);
static bool write_remote_file(SkConnectContext *ctx, const char *path, const char *data, size_t len,
                              GError **error);
static void setup_proxy_command(SkSshOptions *opts, const char *proxy_jump);
static void tab_restore_free(SkTabRestore *tr);
static void install_signal_handlers(SkConnectContext *ctx);
static void remove_signal_handlers(SkConnectContext *ctx);

/* UI callback adapters for SSH layer. */
static gboolean ui_host_key_unknown_cb(SkSshConnection *conn, const char *fingerprint,
                                       const char *key_type, gpointer user_data);
static gboolean ui_host_key_other_cb(SkSshConnection *conn, const char *fingerprint,
                                     const char *old_type, const char *new_type,
                                     gpointer user_data);
static char *ui_password_cb(SkSshConnection *conn, const char *prompt, gpointer user_data);
static char **ui_kbd_interactive_cb(SkSshConnection *conn, const char *name,
                                    const char *instruction, const char **prompts,
                                    const gboolean *show_input, int n_prompts, gpointer user_data);
static char *ui_passphrase_cb(SkSshConnection *conn, const char *key_path, gpointer user_data);

/* ------------------------------------------------------------------ */
/* Public API                                                          */
/* ------------------------------------------------------------------ */

SkConnectContext *
sk_connect_start(const SkConnectParams *params, SkConfig *config, SkConnectDoneCb done_cb,
                 gpointer user_data)
{
  g_return_val_if_fail(params != NULL, NULL);
  g_return_val_if_fail(params->hostname != NULL, NULL);
  g_return_val_if_fail(config != NULL, NULL);

  SkConnectContext *ctx = g_new0(SkConnectContext, 1);

  /* Copy parameters. */
  ctx->hostname = g_strdup(params->hostname);
  ctx->port = params->port > 0 ? params->port : 22;
  ctx->username = g_strdup(params->username);
  ctx->identity_file = g_strdup(params->identity_file);
  ctx->proxy_jump = g_strdup(params->proxy_jump);
  ctx->app = params->app;
  ctx->parent_window = params->parent_window;
  ctx->config = config;
  ctx->cancellable = g_cancellable_new();
  ctx->done_cb = done_cb;
  ctx->done_user_data = user_data;
  ctx->app_windows = g_ptr_array_new();
  ctx->tab_restores = g_ptr_array_new_with_free_func((GDestroyNotify)tab_restore_free);

  /* Resolve client-id (FR-CONFIG-08). */
  GError *error = NULL;
  ctx->client_id = sk_config_resolve_client_id(config, &error);
  if (ctx->client_id == NULL)
  {
    SK_LOG_ERROR(SK_LOG_COMPONENT_UI, "failed to resolve client-id: %s",
                 error ? error->message : "unknown");
    g_clear_error(&error);
    ctx->client_id = g_strdup("unknown");
  }

  SK_LOG_INFO(SK_LOG_COMPONENT_UI, "connection flow starting: host=%s port=%d client_id=%s",
              ctx->hostname, ctx->port, ctx->client_id);

  /* FR-CONN-16: show connection feedback overlay. */
  if (ctx->parent_window != NULL)
  {
    ctx->feedback = sk_conn_feedback_new(ctx->parent_window);
    sk_conn_feedback_set_phase(ctx->feedback, SK_CONN_PHASE_CONNECTING);
  }

  /* Install SIGTERM/SIGINT handlers for graceful shutdown. */
  install_signal_handlers(ctx);

  /* Phase 1: TCP connect + host key + auth (in worker thread). */
  GTask *task = g_task_new(NULL, ctx->cancellable, on_connect_done, ctx);
  g_task_set_task_data(task, ctx, NULL);
  g_task_run_in_thread(task, connect_phase_1_connect);
  g_object_unref(task);

  return ctx;
}

void
sk_connect_disconnect(SkConnectContext *ctx)
{
  if (ctx == NULL || ctx->shutting_down)
    return;

  ctx->shutting_down = true;
  SK_LOG_INFO(SK_LOG_COMPONENT_UI, "graceful disconnect initiated");

  /* 1. Save final state to server (FR-STATE-06). */
  if (ctx->state != NULL && ctx->debounce != NULL)
  {
    sk_state_debounce_flush(ctx->debounce);
  }
  if (ctx->state != NULL && ctx->control_conn != NULL)
  {
    GError *error = NULL;
    if (!save_state_to_server(ctx, &error))
    {
      SK_LOG_WARN(SK_LOG_COMPONENT_STATE, "failed to save final state: %s",
                  error ? error->message : "unknown");
      g_clear_error(&error);
    }
  }

  /* 2. Release lock (FR-LOCK-10, INV-LOCK-2). */
  if (ctx->session_mgr != NULL && ctx->client_id != NULL)
  {
    GError *error = NULL;
    if (!sk_lock_release(ctx->session_mgr, ctx->client_id, &error))
    {
      SK_LOG_WARN(SK_LOG_COMPONENT_TMUX, "failed to release lock: %s",
                  error ? error->message : "unknown");
      g_clear_error(&error);
    }
  }

  /* 3. Disconnect control mode. */
  if (ctx->session_mgr != NULL)
  {
    sk_tmux_control_disconnect(ctx->session_mgr);
  }

  /* 4. Close all tab SSH channels and connections. */
  for (guint i = 0; i < ctx->tab_restores->len; i++)
  {
    SkTabRestore *tr = g_ptr_array_index(ctx->tab_restores, i);
    if (tr->terminal != NULL)
    {
      sk_terminal_tab_disconnect(tr->terminal);
    }
    if (tr->ssh_channel != NULL)
    {
      sk_ssh_channel_free(tr->ssh_channel);
      tr->ssh_channel = NULL;
    }
    if (tr->ssh_conn != NULL)
    {
      sk_ssh_connection_disconnect(tr->ssh_conn);
      sk_ssh_connection_free(tr->ssh_conn);
      tr->ssh_conn = NULL;
    }
  }

  /* 5. Close SFTP and control connection. */
  if (ctx->sftp != NULL)
  {
    sk_sftp_session_free(ctx->sftp);
    ctx->sftp = NULL;
  }
  if (ctx->control_conn != NULL)
  {
    sk_ssh_connection_disconnect(ctx->control_conn);
    sk_ssh_connection_free(ctx->control_conn);
    ctx->control_conn = NULL;
  }

  ctx->connected = false;
  SK_LOG_INFO(SK_LOG_COMPONENT_UI, "disconnect complete");
}

void
sk_connect_free(SkConnectContext *ctx)
{
  if (ctx == NULL)
    return;

  if (ctx->connected)
  {
    sk_connect_disconnect(ctx);
  }

  remove_signal_handlers(ctx);

  g_cancellable_cancel(ctx->cancellable);
  g_clear_object(&ctx->cancellable);

  if (ctx->feedback != NULL)
  {
    sk_conn_feedback_free(ctx->feedback);
    ctx->feedback = NULL;
  }

  if (ctx->debounce != NULL)
  {
    sk_state_debounce_free(ctx->debounce);
    ctx->debounce = NULL;
  }

  if (ctx->state != NULL)
  {
    sk_state_file_free(ctx->state);
    ctx->state = NULL;
  }

  if (ctx->session_mgr != NULL)
  {
    sk_session_manager_free(ctx->session_mgr);
    ctx->session_mgr = NULL;
  }

  if (ctx->reconn_mgr != NULL)
  {
    sk_conn_manager_free(ctx->reconn_mgr);
    ctx->reconn_mgr = NULL;
  }

  /* Free UI windows. */
  for (guint i = 0; i < ctx->app_windows->len; i++)
  {
    SkAppWindow *win = g_ptr_array_index(ctx->app_windows, i);
    sk_app_window_free(win);
  }
  g_ptr_array_free(ctx->app_windows, TRUE);

  g_ptr_array_free(ctx->tab_restores, TRUE);

  g_free(ctx->hostname);
  g_free(ctx->username);
  g_free(ctx->identity_file);
  g_free(ctx->proxy_jump);
  g_free(ctx->client_id);
  g_free(ctx->host_fingerprint);
  g_free(ctx->environment);

  g_free(ctx);
}

void
sk_connect_emergency_shutdown(SkConnectContext *ctx)
{
  if (ctx == NULL)
    return;

  SK_LOG_WARN(SK_LOG_COMPONENT_UI, "emergency shutdown triggered");

  /* Best-effort: save state and release lock synchronously. */
  if (ctx->state != NULL && ctx->control_conn != NULL)
  {
    GError *error = NULL;
    save_state_to_server(ctx, &error);
    g_clear_error(&error);
  }

  if (ctx->session_mgr != NULL && ctx->client_id != NULL)
  {
    GError *error = NULL;
    sk_lock_release(ctx->session_mgr, ctx->client_id, &error);
    g_clear_error(&error);
  }
}

/* ------------------------------------------------------------------ */
/* Queries                                                             */
/* ------------------------------------------------------------------ */

const char *
sk_connect_get_hostname(const SkConnectContext *ctx)
{
  return ctx != NULL ? ctx->hostname : NULL;
}

const char *
sk_connect_get_environment(const SkConnectContext *ctx)
{
  return ctx != NULL ? ctx->environment : NULL;
}

const char *
sk_connect_get_client_id(const SkConnectContext *ctx)
{
  return ctx != NULL ? ctx->client_id : NULL;
}

bool
sk_connect_is_connected(const SkConnectContext *ctx)
{
  return ctx != NULL && ctx->connected;
}

/* ------------------------------------------------------------------ */
/* Phase 1: TCP connect + host key + auth (worker thread)              */
/* ------------------------------------------------------------------ */

/**
 * Worker thread: establish SSH connection, verify host key, authenticate.
 * FR-CONN-01..12, FR-CONN-21, FR-PROXY-01..02
 */
static void
connect_phase_1_connect(GTask *task, gpointer src G_GNUC_UNUSED, gpointer data,
                        GCancellable *cancel G_GNUC_UNUSED)
{
  SkConnectContext *ctx = data;
  GError *error = NULL;

  /* Build SSH options. */
  SkSshOptions opts = build_ssh_options(ctx);

  /* FR-PROXY-01: Set ProxyCommand if proxy_jump is specified. */
  if (ctx->proxy_jump != NULL)
  {
    setup_proxy_command(&opts, ctx->proxy_jump);
  }

  /* FR-CONN-21: Create connection via libssh (never invoke ssh binary). */
  ctx->control_conn = sk_ssh_connection_new(&opts, &error);
  if (ctx->control_conn == NULL)
  {
    g_task_return_error(task, error);
    return;
  }

  /* Blocking connect, host key verification, and authentication.
   * The callbacks for TOFU dialog, password dialog etc. will be invoked
   * on the worker thread and schedule GTK dialogs on the main thread
   * via g_idle_add(). */
  if (!sk_ssh_connection_connect(ctx->control_conn, &error))
  {
    /* Classify the error for the appropriate E-CONN code. */
    if (error != NULL && error->domain == SK_SSH_ERROR)
    {
      switch (error->code)
      {
      case SK_SSH_ERROR_AUTH:
        /* E-CONN-1: authentication failure (FR-CONN-17). */
        SK_LOG_WARN(SK_LOG_COMPONENT_SSH, "authentication failed for %s", ctx->hostname);
        break;
      case SK_SSH_ERROR_CONNECT:
      case SK_SSH_ERROR_TIMEOUT:
        /* E-CONN-2: host unreachable (FR-CONN-18). */
        SK_LOG_WARN(SK_LOG_COMPONENT_SSH, "host unreachable: %s", ctx->hostname);
        break;
      case SK_SSH_ERROR_HOST_KEY:
        /* E-CONN-6 or E-CONN-7: host key issue. */
        SK_LOG_WARN(SK_LOG_COMPONENT_SSH, "host key verification failed for %s", ctx->hostname);
        break;
      default:
        break;
      }
    }
    /* FR-PROXY-03: Indicate proxy hop failure if applicable. */
    if (ctx->proxy_jump != NULL && error != NULL)
    {
      GError *proxy_error =
          g_error_new(SK_CONNECT_ERROR, SK_CONNECT_ERROR_PROXY,
                      "Connection via proxy '%s' failed: %s", ctx->proxy_jump, error->message);
      g_error_free(error);
      error = proxy_error;
    }
    g_task_return_error(task, error);
    return;
  }

  /* Store host fingerprint for local cache path. */
  ctx->host_fingerprint = sk_ssh_get_host_fingerprint(ctx->control_conn);

  SK_LOG_INFO(SK_LOG_COMPONENT_SSH, "SSH connection established to %s:%d", ctx->hostname,
              ctx->port);

  g_task_return_boolean(task, TRUE);
}

/**
 * Main thread callback after Phase 1 completes.
 */
static void
on_connect_done(GObject *src G_GNUC_UNUSED, GAsyncResult *res, gpointer data)
{
  SkConnectContext *ctx = data;
  GError *error = NULL;

  if (!g_task_propagate_boolean(G_TASK(res), &error))
  {
    flow_fail(ctx, error);
    return;
  }

  /* FR-CONN-16: update feedback to "Authenticating..." phase done,
   * move to tmux detection. */
  if (ctx->feedback != NULL)
  {
    sk_conn_feedback_set_phase(ctx->feedback, SK_CONN_PHASE_CHECKING_TMUX);
  }

  connect_phase_2_post_auth(ctx);
}

/* ------------------------------------------------------------------ */
/* Phase 2: Post-auth setup (SFTP, tmux detect)                        */
/* ------------------------------------------------------------------ */

static void
connect_phase_2_post_auth(SkConnectContext *ctx)
{
  /* Open SFTP session (FR-CONN-20: fallback if unavailable). */
  GError *error = NULL;
  ctx->sftp = sk_sftp_session_new(ctx->control_conn, &error);
  if (ctx->sftp == NULL)
  {
    /* E-CONN-8: SFTP unavailable, warn once and use shell fallback. */
    SK_LOG_WARN(SK_LOG_COMPONENT_SFTP, "SFTP unavailable, using shell fallback: %s",
                error ? error->message : "unknown");
    g_clear_error(&error);
    if (!ctx->sftp_warned && ctx->parent_window != NULL)
    {
      sk_dialog_info(ctx->parent_window, "SFTP Unavailable",
                     "SFTP is not available on this server. "
                     "Using shell commands as fallback for file "
                     "operations. Performance may be reduced.");
      ctx->sftp_warned = true;
    }
  }

  /* Create session manager. */
  ctx->session_mgr = sk_session_manager_new(ctx->control_conn);
  if (ctx->session_mgr == NULL)
  {
    GError *err = g_error_new(SK_CONNECT_ERROR, SK_CONNECT_ERROR_TMUX_MISSING,
                              "Failed to create session manager");
    flow_fail(ctx, err);
    return;
  }

  /* Phase 2b: tmux detection (async). */
  phase_tmux_detect_async(ctx);
}

/* ------------------------------------------------------------------ */
/* Phase 3: tmux detection (FR-CONN-13..15)                            */
/* ------------------------------------------------------------------ */

static void
tmux_detect_worker(GTask *task, gpointer src G_GNUC_UNUSED, gpointer data,
                   GCancellable *cancel G_GNUC_UNUSED)
{
  SkConnectContext *ctx = data;
  GError *error = NULL;
  SkTmuxVersion version = { 0 };

  bool ok = sk_tmux_detect(ctx->session_mgr, &version, &error);
  if (!ok)
  {
    if (error != NULL && error->code == SK_SESSION_ERROR_TMUX_NOT_FOUND)
    {
      /* E-CONN-3 (FR-CONN-14). */
      g_task_return_error(task, error);
    }
    else if (error != NULL && error->code == SK_SESSION_ERROR_TMUX_VERSION)
    {
      /* E-CONN-4 (FR-CONN-15): version too old but allow attempt. */
      SK_LOG_WARN(SK_LOG_COMPONENT_TMUX, "tmux version below minimum: %s",
                  version.version_string ? version.version_string : "unknown");
      /* We continue anyway with a warning. */
      g_clear_error(&error);
      g_task_return_boolean(task, TRUE);
    }
    else
    {
      g_task_return_error(task, error);
    }
    g_free(version.version_string);
    return;
  }

  SK_LOG_INFO(SK_LOG_COMPONENT_TMUX, "tmux detected: %s (>= %d.%d)",
              version.version_string ? version.version_string : "?", SK_TMUX_MIN_VERSION_MAJOR,
              SK_TMUX_MIN_VERSION_MINOR);
  g_free(version.version_string);

  /* Open tmux control mode connection. */
  if (!sk_tmux_control_connect(ctx->session_mgr, &error))
  {
    g_task_return_error(task, error);
    return;
  }

  g_task_return_boolean(task, TRUE);
}

static void
phase_tmux_detect_async(SkConnectContext *ctx)
{
  GTask *task = g_task_new(NULL, ctx->cancellable, on_tmux_detect_done, ctx);
  g_task_set_task_data(task, ctx, NULL);
  g_task_run_in_thread(task, tmux_detect_worker);
  g_object_unref(task);
}

static void
on_tmux_detect_done(GObject *src G_GNUC_UNUSED, GAsyncResult *res, gpointer data)
{
  SkConnectContext *ctx = data;
  GError *error = NULL;

  if (!g_task_propagate_boolean(G_TASK(res), &error))
  {
    /* E-CONN-3: tmux not found — show installation dialog. */
    if (error != NULL && error->domain == SK_SESSION_ERROR &&
        error->code == SK_SESSION_ERROR_TMUX_NOT_FOUND)
    {
      if (ctx->parent_window != NULL)
      {
        sk_dialog_error(ctx->parent_window, "tmux Not Found",
                        "tmux is required on the server but was not found.\n\n"
                        "Install it using your package manager:\n"
                        "  Ubuntu/Debian: sudo apt install tmux\n"
                        "  Fedora/RHEL:   sudo dnf install tmux\n"
                        "  Arch Linux:    sudo pacman -S tmux\n"
                        "  macOS:         brew install tmux");
      }
    }
    flow_fail(ctx, error);
    return;
  }

  /* Move to lock acquisition phase. */
  if (ctx->feedback != NULL)
  {
    sk_conn_feedback_set_phase(ctx->feedback, SK_CONN_PHASE_LOADING_STATE);
  }

  phase_lock_acquire(ctx);
}

/* ------------------------------------------------------------------ */
/* Phase 4: Lock acquisition (FR-LOCK-*)                               */
/* ------------------------------------------------------------------ */

static void
lock_acquire_worker(GTask *task, gpointer src G_GNUC_UNUSED, gpointer data,
                    GCancellable *cancel G_GNUC_UNUSED)
{
  SkConnectContext *ctx = data;
  GError *error = NULL;

  /* Get local hostname for lock metadata. */
  char hostname_buf[256];
  if (gethostname(hostname_buf, sizeof(hostname_buf)) != 0)
  {
    g_strlcpy(hostname_buf, "unknown", sizeof(hostname_buf));
  }

  /* FR-LOCK-08: Acquire lock before reading state. */
  if (!sk_lock_acquire(ctx->session_mgr, ctx->client_id, hostname_buf, &error))
  {
    if (error != NULL && error->code == SK_SESSION_ERROR_LOCK_CONFLICT)
    {
      /* Lock exists. Check if it is ours or orphaned. */
      g_clear_error(&error);

      SkLockInfo *lock_info = sk_lock_check(ctx->session_mgr, ctx->client_id, &error);
      if (lock_info == NULL)
      {
        g_task_return_error(task, error);
        return;
      }

      /* FR-LOCK-06: If same hostname + PID, renew silently. */
      char pid_str[32];
      g_snprintf(pid_str, sizeof(pid_str), "%d", (int)getpid());

      if (sk_lock_is_own(lock_info, hostname_buf, pid_str))
      {
        SK_LOG_INFO(SK_LOG_COMPONENT_TMUX, "lock is ours (reconnection), renewing");
        sk_lock_info_free(lock_info);
        /* Release and re-acquire to update metadata. */
        sk_lock_release(ctx->session_mgr, ctx->client_id, NULL);
        if (!sk_lock_acquire(ctx->session_mgr, ctx->client_id, hostname_buf, &error))
        {
          g_task_return_error(task, error);
          return;
        }
        g_task_return_boolean(task, TRUE);
        return;
      }

      /* FR-LOCK-07: Check if orphaned. */
      int keepalive_timeout = SK_LOCK_DEFAULT_KEEPALIVE_TIMEOUT;
      if (sk_lock_is_orphaned(lock_info, keepalive_timeout))
      {
        SK_LOG_INFO(SK_LOG_COMPONENT_TMUX, "lock is orphaned, auto-takeover");
        sk_lock_info_free(lock_info);
        /* Force acquire: kill old lock, create new. */
        sk_lock_release(ctx->session_mgr, ctx->client_id, NULL);
        if (!sk_lock_acquire(ctx->session_mgr, ctx->client_id, hostname_buf, &error))
        {
          g_task_return_error(task, error);
          return;
        }
        g_task_return_boolean(task, TRUE);
        return;
      }

      /* FR-LOCK-05: Valid lock held by another client.
       * Store lock info so the main thread can show the dialog. */
      g_task_set_task_data(task, lock_info, NULL);
      g_task_return_new_error(task, SK_CONNECT_ERROR, SK_CONNECT_ERROR_LOCK,
                              "Lock held by %s since %s",
                              lock_info->hostname ? lock_info->hostname : "unknown",
                              lock_info->connected_at ? lock_info->connected_at : "unknown");
      return;
    }
    g_task_return_error(task, error);
    return;
  }

  SK_LOG_INFO(SK_LOG_COMPONENT_TMUX, "lock acquired for client %s", ctx->client_id);

  g_task_return_boolean(task, TRUE);
}

static void
phase_lock_acquire(SkConnectContext *ctx)
{
  GTask *task = g_task_new(NULL, ctx->cancellable, on_lock_done, ctx);
  g_task_set_task_data(task, ctx, NULL);
  g_task_run_in_thread(task, lock_acquire_worker);
  g_object_unref(task);
}

static void
on_lock_done(GObject *src G_GNUC_UNUSED, GAsyncResult *res, gpointer data)
{
  SkConnectContext *ctx = data;
  GError *error = NULL;

  if (!g_task_propagate_boolean(G_TASK(res), &error))
  {
    if (error != NULL && error->domain == SK_CONNECT_ERROR && error->code == SK_CONNECT_ERROR_LOCK)
    {
      /* FR-LOCK-05: Show conflict dialog. */
      /* Extract lock info from error message for dialog. */
      if (ctx->parent_window != NULL)
      {
        bool takeover = sk_dialog_conflict(ctx->parent_window, "another device", error->message);

        if (takeover)
        {
          g_clear_error(&error);
          SK_LOG_INFO(SK_LOG_COMPONENT_TMUX, "user chose to take over lock");
          /* Force release and re-acquire. */
          sk_lock_release(ctx->session_mgr, ctx->client_id, NULL);

          /* Re-attempt lock acquisition. */
          phase_lock_acquire(ctx);
          return;
        }

        /* User cancelled. */
        g_clear_error(&error);
        error = g_error_new(SK_CONNECT_ERROR, SK_CONNECT_ERROR_CANCELLED,
                            "Connection cancelled by user "
                            "(lock conflict)");
      }
    }
    flow_fail(ctx, error);
    return;
  }

  /* Proceed to state loading. */
  phase_load_state(ctx);
}

/* ------------------------------------------------------------------ */
/* Phase 5: State loading (FR-STATE-01..02, FR-STATE-06..08)           */
/* ------------------------------------------------------------------ */

static void
state_load_worker(GTask *task, gpointer src G_GNUC_UNUSED, gpointer data,
                  GCancellable *cancel G_GNUC_UNUSED)
{
  SkConnectContext *ctx = data;
  GError *error = NULL;

  /* FR-STATE-07: Clean orphaned .tmp files. */
  char *cache_dir = sk_paths_server_cache_dir(ctx->host_fingerprint);
  if (cache_dir != NULL)
  {
    sk_state_cleanup_tmp_files(cache_dir);
    g_free(cache_dir);
  }

  /* FR-STATE-02: Server state takes precedence over local cache. */
  SkStateFile *state = load_state_from_server(ctx, &error);

  if (state == NULL && error != NULL)
  {
    if (error->domain == SK_STATE_ERROR && error->code == SK_STATE_ERROR_CORRUPT)
    {
      /* E-CONN-5: Corrupted state file (FR-CONN-19).
       * Rename corrupt file and treat as first connection. */
      SK_LOG_WARN(SK_LOG_COMPONENT_STATE, "server state file corrupted, treating as new");
      g_clear_error(&error);

      /* Rename the corrupt file. */
      char *state_path = build_remote_state_path(ctx->client_id);
      char *corrupt_path = g_strdup_printf("%s.corrupt.%ld", state_path, (long)time(NULL));
      GError *rename_err = NULL;
      if (ctx->sftp != NULL)
      {
        sk_sftp_rename(ctx->sftp, state_path, corrupt_path, &rename_err);
      }
      g_clear_error(&rename_err);
      g_free(state_path);
      g_free(corrupt_path);

      /* Create fresh state. */
      state = sk_state_file_new(ctx->client_id);
    }
    else if (error->domain == SK_STATE_ERROR && error->code == SK_STATE_ERROR_VERSION_FUTURE)
    {
      /* FR-STATE-08: Version too high — show upgrade message. */
      g_task_return_error(task, error);
      return;
    }
    else
    {
      /* File not found or other I/O error. Try local cache. */
      g_clear_error(&error);
      state = sk_state_load_local_cache(ctx->host_fingerprint, ctx->client_id, &error);
      if (state == NULL)
      {
        /* No cached state either. First connection. */
        g_clear_error(&error);
        state = sk_state_file_new(ctx->client_id);
        SK_LOG_INFO(SK_LOG_COMPONENT_STATE, "no existing state found, starting fresh");
      }
    }
  }

  /* Validate state integrity (FR-STATE-16). */
  if (!sk_state_validate(state, &error))
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_STATE, "state validation failed: %s",
                error ? error->message : "unknown");
    g_clear_error(&error);
    /* Fall back to fresh state. */
    sk_state_file_free(state);
    state = sk_state_file_new(ctx->client_id);
  }

  ctx->state = state;

  /* Set up debounced state writer (FR-STATE-03, NFR-PERF-07). */
  char *remote_path = build_remote_state_path(ctx->client_id);
  ctx->debounce = sk_state_debounce_new(remote_path, ctx->host_fingerprint);
  g_free(remote_path);

  g_task_return_boolean(task, TRUE);
}

static void
phase_load_state(SkConnectContext *ctx)
{
  GTask *task = g_task_new(NULL, ctx->cancellable, on_state_loaded, ctx);
  g_task_set_task_data(task, ctx, NULL);
  g_task_run_in_thread(task, state_load_worker);
  g_object_unref(task);
}

static void
on_state_loaded(GObject *src G_GNUC_UNUSED, GAsyncResult *res, gpointer data)
{
  SkConnectContext *ctx = data;
  GError *error = NULL;

  if (!g_task_propagate_boolean(G_TASK(res), &error))
  {
    flow_fail(ctx, error);
    return;
  }

  /* Phase 6: Environment selection. */
  phase_env_select(ctx);
}

/* ------------------------------------------------------------------ */
/* Phase 6: Environment selection (FR-ENV-03..05)                      */
/* ------------------------------------------------------------------ */

static void
phase_env_select(SkConnectContext *ctx)
{
  g_assert(ctx->state != NULL);

  int n_envs = ctx->state->n_environments;

  if (n_envs == 0)
  {
    /* FR-ENV-05: First connection — create "Default" environment. */
    SkEnvironment *env = sk_environment_new("Default");
    SkWindow *win = sk_window_new(NULL, "shellkeep");

    /* Create a fresh tab. */
    char *session_name = sk_session_generate_name();
    char *uuid = g_uuid_string_random();
    SkTab *tab = sk_tab_new(uuid, session_name, session_name, 0);
    g_free(uuid);
    g_free(session_name);

    win->tabs = g_new0(SkTab *, 2);
    win->tabs[0] = tab;
    win->n_tabs = 1;
    win->active_tab = 0;

    env->windows = g_new0(SkWindow *, 2);
    env->windows[0] = win;
    env->n_windows = 1;

    ctx->state->environments = g_new0(SkEnvironment *, 2);
    ctx->state->environments[0] = env;
    ctx->state->n_environments = 1;
    g_free(ctx->state->last_environment);
    ctx->state->last_environment = g_strdup("Default");

    ctx->environment = g_strdup("Default");
    SK_LOG_INFO(SK_LOG_COMPONENT_UI, "first connection: created Default environment");
  }
  else if (n_envs == 1)
  {
    /* FR-ENV-04: Single environment — open directly. */
    ctx->environment = g_strdup(ctx->state->environments[0]->name);
    SK_LOG_INFO(SK_LOG_COMPONENT_UI, "single environment: %s", ctx->environment);
  }
  else
  {
    /* FR-ENV-03: Multiple environments — show selection dialog. */
    const char **env_names = g_new0(const char *, n_envs);
    for (int i = 0; i < n_envs; i++)
    {
      env_names[i] = ctx->state->environments[i]->name;
    }

    char *selected = NULL;
    if (ctx->parent_window != NULL)
    {
      selected = sk_dialog_environment_select(ctx->parent_window, env_names, n_envs,
                                              ctx->state->last_environment);
    }
    g_free(env_names);

    if (selected == NULL)
    {
      GError *err = g_error_new(SK_CONNECT_ERROR, SK_CONNECT_ERROR_CANCELLED,
                                "Environment selection cancelled");
      flow_fail(ctx, err);
      return;
    }

    ctx->environment = selected;
    SK_LOG_INFO(SK_LOG_COMPONENT_UI, "environment selected: %s", ctx->environment);
  }

  /* Update last_environment in state. */
  g_free(ctx->state->last_environment);
  ctx->state->last_environment = g_strdup(ctx->environment);

  /* FR-CONN-16: Update feedback to restoring phase. */
  if (ctx->feedback != NULL)
  {
    sk_conn_feedback_set_phase(ctx->feedback, SK_CONN_PHASE_RESTORING);
  }

  /* Phase 7: Restore sessions. */
  phase_restore_sessions(ctx);
}

/* ------------------------------------------------------------------ */
/* Phase 7: Session restoration (FR-STATE-09..11)                      */
/* ------------------------------------------------------------------ */

/**
 * Find the active environment in the state file.
 */
static SkEnvironment *
find_environment(const SkStateFile *state, const char *env_name)
{
  for (int i = 0; i < state->n_environments; i++)
  {
    if (g_strcmp0(state->environments[i]->name, env_name) == 0)
    {
      return state->environments[i];
    }
  }
  return NULL;
}

static void
phase_restore_sessions(SkConnectContext *ctx)
{
  SkEnvironment *env = find_environment(ctx->state, ctx->environment);
  if (env == NULL)
  {
    /* No windows in this environment — create a default one. */
    env = sk_environment_new(ctx->environment);
    /* Append to state. */
    int n = ctx->state->n_environments;
    ctx->state->environments = g_renew(SkEnvironment *, ctx->state->environments, n + 2);
    ctx->state->environments[n] = env;
    ctx->state->environments[n + 1] = NULL;
    ctx->state->n_environments = n + 1;
  }

  /* Reconcile state with live tmux sessions (FR-SESSION-07..08). */
  GError *error = NULL;

  /* Collect all session UUIDs from state for reconciliation. */
  GPtrArray *state_sessions = g_ptr_array_new();
  for (int w = 0; w < env->n_windows; w++)
  {
    SkWindow *win = env->windows[w];
    for (int t = 0; t < win->n_tabs; t++)
    {
      SkTab *tab = win->tabs[t];
      SkSessionInfo *si = g_new0(SkSessionInfo, 1);
      si->session_uuid = g_strdup(tab->session_uuid);
      si->name = g_strdup(tab->tmux_session_name);
      g_ptr_array_add(state_sessions, si);
    }
  }

  SkReconcileResult *reconcile = sk_session_reconcile(ctx->session_mgr, state_sessions,
                                                      ctx->client_id, ctx->environment, &error);

  /* Free state_sessions (SkSessionInfo elements borrowed by reconcile). */
  for (guint i = 0; i < state_sessions->len; i++)
  {
    sk_session_info_free(g_ptr_array_index(state_sessions, i));
  }
  g_ptr_array_free(state_sessions, TRUE);

  if (reconcile == NULL)
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_TMUX, "session reconciliation failed: %s",
                error ? error->message : "unknown");
    g_clear_error(&error);
    /* Continue with state as-is. */
  }

  /* Handle dead sessions (FR-HISTORY-04..12). */
  if (reconcile != NULL && reconcile->dead != NULL && reconcile->dead->len > 0)
  {
    handle_dead_sessions(ctx, reconcile);
  }

  /* FR-SESSION-12: Handle orphaned sessions. */
  if (reconcile != NULL && reconcile->orphaned != NULL && reconcile->orphaned->len > 0)
  {
    handle_orphaned_sessions(ctx, reconcile);
  }

  if (reconcile != NULL)
  {
    sk_reconcile_result_free(reconcile);
  }

  /* Set up the reconnection manager. */
  ctx->reconn_mgr = sk_conn_manager_new(
      ctx->hostname, ctx->port, ctx->username, SK_RESTORE_BATCH_SIZE,
      ctx->config->ssh_reconnect_max_attempts, ctx->config->ssh_reconnect_backoff_base);

  /* Build the list of tabs to restore across all windows. */
  g_ptr_array_set_size(ctx->tab_restores, 0);
  int restore_index = 0;

  /* FR-STATE-10: Progressive restoration — focused window first. */
  for (int w = 0; w < env->n_windows; w++)
  {
    SkWindow *win = env->windows[w];

    /* Create the UI window. */
    SkAppWindow *app_win;
    if (win->geometry.is_set)
    {
      app_win = sk_app_window_new_from_state(ctx->app, win->title, win->geometry.x, win->geometry.y,
                                             win->geometry.width, win->geometry.height);
    }
    else
    {
      app_win = sk_app_window_new(ctx->app);
    }

    if (app_win == NULL)
    {
      SK_LOG_ERROR(SK_LOG_COMPONENT_UI, "failed to create window %d", w);
      continue;
    }

    g_ptr_array_add(ctx->app_windows, app_win);
    sk_app_window_show(app_win);

    /* Create restore contexts for each tab. */
    for (int t = 0; t < win->n_tabs; t++)
    {
      SkTab *tab = win->tabs[t];
      SkTabRestore *tr = g_new0(SkTabRestore, 1);
      tr->ctx = ctx;
      tr->state_tab = tab;
      tr->state_window = win;
      tr->session_uuid = g_strdup(tab->session_uuid);
      tr->tmux_name = g_strdup(tab->tmux_session_name);
      tr->app_window = app_win;
      tr->index = restore_index++;
      g_ptr_array_add(ctx->tab_restores, tr);
    }
  }

  if (ctx->tab_restores->len == 0)
  {
    /* No tabs to restore — create a default window/tab. */
    SkAppWindow *app_win = sk_app_window_new(ctx->app);
    if (app_win != NULL)
    {
      g_ptr_array_add(ctx->app_windows, app_win);
      sk_app_window_show(app_win);

      SkTabRestore *tr = g_new0(SkTabRestore, 1);
      tr->ctx = ctx;
      tr->session_uuid = g_uuid_string_random();
      tr->tmux_name = NULL; /* Will be created fresh. */
      tr->app_window = app_win;
      tr->index = 0;
      g_ptr_array_add(ctx->tab_restores, tr);
    }
  }

  /* FR-CONN-16: Show progress. */
  if (ctx->feedback != NULL)
  {
    sk_conn_feedback_set_progress(ctx->feedback, 0, (int)ctx->tab_restores->len);
  }

  /* FR-STATE-11: Parallel restoration in batches of 5. */
  restore_batch(ctx, 0);
}

/* ------------------------------------------------------------------ */
/* Batch tab restoration                                               */
/* ------------------------------------------------------------------ */

/** Number of tabs currently being restored in parallel. */
static volatile gint g_active_restores = 0;
/** Start index of the current batch. */
static volatile gint g_batch_start = 0;

static void
restore_batch(SkConnectContext *ctx, int start_index)
{
  int total = (int)ctx->tab_restores->len;
  int end = start_index + SK_RESTORE_BATCH_SIZE;
  if (end > total)
    end = total;

  if (start_index >= total)
  {
    /* All tabs restored — connection flow complete. */
    flow_complete(ctx, true, NULL);
    return;
  }

  g_atomic_int_set(&g_batch_start, start_index);
  g_atomic_int_set(&g_active_restores, end - start_index);

  SK_LOG_INFO(SK_LOG_COMPONENT_UI, "restoring tabs %d..%d of %d", start_index + 1, end, total);

  /* FR-CONN-22: Each tab gets its own SSH connection. */
  for (int i = start_index; i < end; i++)
  {
    SkTabRestore *tr = g_ptr_array_index(ctx->tab_restores, i);
    GTask *task = g_task_new(NULL, ctx->cancellable, on_tab_restore_done, tr);
    g_task_set_task_data(task, tr, NULL);
    g_task_run_in_thread(task, tab_restore_worker);
    g_object_unref(task);
  }
}

/**
 * Worker thread: establish per-tab SSH, attach to tmux session.
 * FR-CONN-22, FR-SESSION-03
 */
static void
tab_restore_worker(GTask *task, gpointer src G_GNUC_UNUSED, gpointer data,
                   GCancellable *cancel G_GNUC_UNUSED)
{
  SkTabRestore *tr = data;
  SkConnectContext *ctx = tr->ctx;
  GError *error = NULL;

  /* Create per-tab SSH connection (FR-CONN-22). */
  SkSshOptions opts = build_ssh_options(ctx);

  /* FR-PROXY-01: Apply proxy if configured. */
  if (ctx->proxy_jump != NULL)
  {
    setup_proxy_command(&opts, ctx->proxy_jump);
  }

  tr->ssh_conn = sk_ssh_connection_new(&opts, &error);
  if (tr->ssh_conn == NULL)
  {
    g_task_return_error(task, error);
    return;
  }

  if (!sk_ssh_connection_connect(tr->ssh_conn, &error))
  {
    /* FR-PROXY-03: Indicate which hop failed. */
    if (ctx->proxy_jump != NULL)
    {
      GError *wrap =
          g_error_new(SK_CONNECT_ERROR, SK_CONNECT_ERROR_PROXY, "Tab SSH via proxy '%s' failed: %s",
                      ctx->proxy_jump, error->message);
      g_error_free(error);
      error = wrap;
    }
    g_task_return_error(task, error);
    return;
  }

  /* Create or attach to tmux session. */
  if (tr->tmux_name != NULL && sk_session_exists(ctx->session_mgr, tr->tmux_name))
  {
    /* Existing session — attach (FR-SESSION-03). */
    tr->tmux_session = sk_session_attach(ctx->session_mgr, tr->tmux_name, &error);
    if (tr->tmux_session == NULL)
    {
      g_task_return_error(task, error);
      return;
    }
  }
  else
  {
    /* New session — create. */
    char *session_name_part = NULL;
    if (tr->state_tab != NULL && tr->state_tab->title != NULL)
    {
      session_name_part = g_strdup(tr->state_tab->title);
    }
    else
    {
      session_name_part = sk_session_generate_name();
    }

    tr->tmux_session = sk_session_create(ctx->session_mgr, ctx->client_id, ctx->environment,
                                         session_name_part, 80, 24, &error);
    g_free(session_name_part);

    if (tr->tmux_session == NULL)
    {
      g_task_return_error(task, error);
      return;
    }

    /* Update tmux_name from created session. */
    g_free(tr->tmux_name);
    tr->tmux_name = g_strdup(sk_tmux_session_get_name(tr->tmux_session));

    /* Update session UUID. */
    const char *uuid = sk_tmux_session_get_uuid(tr->tmux_session);
    if (uuid != NULL)
    {
      g_free(tr->session_uuid);
      tr->session_uuid = g_strdup(uuid);
    }
  }

  /* Enable history capture (FR-HISTORY-01). */
  if (tr->session_uuid != NULL && tr->tmux_name != NULL)
  {
    GError *hist_err = NULL;
    if (!sk_session_enable_history(ctx->session_mgr, tr->tmux_name, tr->session_uuid, &hist_err))
    {
      SK_LOG_WARN(SK_LOG_COMPONENT_TMUX, "failed to enable history for %s: %s", tr->tmux_name,
                  hist_err ? hist_err->message : "unknown");
      g_clear_error(&hist_err);
    }
  }

  /* Open SSH channel with PTY for tmux attach. */
  tr->ssh_channel = sk_ssh_channel_open(tr->ssh_conn, 80, 24, &error);
  if (tr->ssh_channel == NULL)
  {
    g_task_return_error(task, error);
    return;
  }

  /* Execute tmux attach on the channel (FR-SESSION-03). */
  char *attach_cmd = g_strdup_printf("tmux attach-session -t '%s'", tr->tmux_name);
  if (!sk_ssh_channel_exec(tr->ssh_channel, attach_cmd, &error))
  {
    g_free(attach_cmd);
    g_task_return_error(task, error);
    return;
  }
  g_free(attach_cmd);

  g_task_return_boolean(task, TRUE);
}

/**
 * Main thread callback: wire up terminal widget after tab restore.
 */
static void
on_tab_restore_done(GObject *src G_GNUC_UNUSED, GAsyncResult *res, gpointer data)
{
  SkTabRestore *tr = data;
  SkConnectContext *ctx = tr->ctx;
  GError *error = NULL;

  bool success = g_task_propagate_boolean(G_TASK(res), &error);

  if (success)
  {
    /* Create terminal tab widget. */
    SkTerminalConfig term_config = {
      .font_family = ctx->config->font_family,
      .font_size = ctx->config->font_size,
      .scrollback_lines = ctx->config->scrollback_lines,
      .cursor_shape = (SkCursorShape)ctx->config->cursor_shape,
      .cursor_blink = (ctx->config->cursor_blink == SK_CURSOR_BLINK_ON),
      .bold_is_bright = ctx->config->bold_is_bright,
      .allow_hyperlinks = ctx->config->allow_hyperlinks,
      .word_chars = ctx->config->word_chars,
      .audible_bell = (ctx->config->bell == SK_BELL_AUDIBLE),
    };

    tr->terminal = sk_terminal_tab_new(&term_config);
    if (tr->terminal != NULL)
    {
      GError *conn_err = NULL;
      if (sk_terminal_tab_connect(tr->terminal, tr->ssh_conn, tr->ssh_channel, &conn_err))
      {
        /* Add tab to window. */
        const char *title = tr->state_tab != NULL ? tr->state_tab->title : "New Session";
        tr->app_tab =
            sk_app_window_add_tab(tr->app_window, tr->terminal, title ? title : "Session");

        if (tr->app_tab != NULL)
        {
          sk_app_tab_set_indicator(tr->app_tab, SK_CONN_INDICATOR_GREEN);
        }
      }
      else
      {
        SK_LOG_WARN(SK_LOG_COMPONENT_TERMINAL, "failed to connect terminal: %s",
                    conn_err ? conn_err->message : "unknown");
        g_clear_error(&conn_err);
      }
    }
  }
  else
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_UI, "tab %d restore failed: %s", tr->index,
                error ? error->message : "unknown");
    g_clear_error(&error);
  }

  /* Update progress (FR-CONN-16). */
  if (ctx->feedback != NULL)
  {
    sk_conn_feedback_set_progress(ctx->feedback, tr->index + 1, (int)ctx->tab_restores->len);
  }

  /* Check if this batch is complete. */
  int remaining = g_atomic_int_add(&g_active_restores, -1) - 1;
  if (remaining <= 0)
  {
    /* Start next batch. */
    int next = g_atomic_int_get(&g_batch_start) + SK_RESTORE_BATCH_SIZE;
    restore_batch(ctx, next);
  }
}

/* ------------------------------------------------------------------ */
/* Dead session handling (FR-HISTORY-04..12, FR-UI-07..10)             */
/* ------------------------------------------------------------------ */

/**
 * Handle sessions that exist in state but not on the server (dead).
 */
static void
handle_dead_sessions(SkConnectContext *ctx, SkReconcileResult *reconcile)
{
  for (guint i = 0; i < reconcile->dead->len; i++)
  {
    const char *dead_uuid = g_ptr_array_index(reconcile->dead, i);
    SK_LOG_INFO(SK_LOG_COMPONENT_UI, "dead session detected: %s", dead_uuid);

    /* Load history from server (FR-HISTORY-02). */
    char *history_path = g_strdup_printf("%s/%s.raw", SK_REMOTE_HISTORY_DIR, dead_uuid);

    size_t history_len = 0;
    GError *error = NULL;
    char *history_data = read_remote_file(ctx, history_path, &history_len, &error);
    g_free(history_path);

    if (history_data == NULL)
    {
      /* FR-UI-09: History unavailable. */
      SK_LOG_WARN(SK_LOG_COMPONENT_STATE, "history unavailable for dead session %s: %s", dead_uuid,
                  error ? error->message : "file not found");
      g_clear_error(&error);
    }

    /* Find which tab(s) reference this dead UUID and mark them. */
    for (guint t = 0; t < ctx->tab_restores->len; t++)
    {
      SkTabRestore *tr = g_ptr_array_index(ctx->tab_restores, t);
      if (g_strcmp0(tr->session_uuid, dead_uuid) == 0)
      {
        tr->is_dead = true;

        /* FR-UI-07: Dead session will be shown in read-only mode
         * with history. The terminal setup happens after the
         * restore batch creates the terminal widgets. We prepare
         * here. Create the terminal tab immediately for dead
         * sessions since they don't need SSH. */
        SkTerminalConfig term_config = {
          .font_family = ctx->config->font_family,
          .font_size = ctx->config->font_size,
          .scrollback_lines = ctx->config->scrollback_lines,
          .cursor_shape = (SkCursorShape)ctx->config->cursor_shape,
          .cursor_blink = false,
          .bold_is_bright = ctx->config->bold_is_bright,
          .allow_hyperlinks = ctx->config->allow_hyperlinks,
          .word_chars = ctx->config->word_chars,
          .audible_bell = false,
        };

        tr->terminal = sk_terminal_tab_new(&term_config);
        if (tr->terminal != NULL)
        {
          /* FR-UI-07: Feed history and show dead overlay.
           * INV-DEAD-1: Dead session never accepts input. */
          sk_terminal_tab_set_dead(tr->terminal, history_data,
                                   history_data ? (gssize)history_len : 0,
                                   "This session was terminated on the server. "
                                   "Output history is preserved below.");

          /* FR-UI-08: "Create new session" callback. */
          /* TODO: Wire up new session callback when
           * full UI integration is ready. */

          /* Add to window. */
          const char *title = tr->state_tab != NULL ? tr->state_tab->title : "Dead Session";
          tr->app_tab =
              sk_app_window_add_tab(tr->app_window, tr->terminal, title ? title : "Dead Session");

          if (tr->app_tab != NULL)
          {
            sk_app_tab_set_dead(tr->app_tab, true);
            sk_app_tab_set_indicator(tr->app_tab, SK_CONN_INDICATOR_RED);
          }
        }

        /* Dead tabs skip SSH restoration — remove from batch. */
        /* Mark so tab_restore_worker skips them. */
      }
    }

    g_free(history_data);
  }
}

/* ------------------------------------------------------------------ */
/* Orphaned session handling (FR-SESSION-12)                           */
/* ------------------------------------------------------------------ */

/**
 * Handle sessions present on the server but not in any window.
 * Creates an "Ungrouped sessions" window.
 */
static void
handle_orphaned_sessions(SkConnectContext *ctx, SkReconcileResult *reconcile)
{
  if (reconcile->orphaned == NULL || reconcile->orphaned->len == 0)
    return;

  SK_LOG_INFO(SK_LOG_COMPONENT_UI, "found %u orphaned sessions, creating Ungrouped window",
              reconcile->orphaned->len);

  /* Create "Ungrouped sessions" window. */
  SkAppWindow *orphan_win =
      sk_app_window_new_from_state(ctx->app, "Ungrouped sessions", -1, -1, 800, 600);

  if (orphan_win == NULL)
  {
    SK_LOG_ERROR(SK_LOG_COMPONENT_UI, "failed to create Ungrouped sessions window");
    return;
  }

  g_ptr_array_add(ctx->app_windows, orphan_win);
  sk_app_window_show(orphan_win);

  /* Add each orphaned session as a tab to restore. */
  for (guint i = 0; i < reconcile->orphaned->len; i++)
  {
    SkSessionInfo *si = g_ptr_array_index(reconcile->orphaned, i);

    SkTabRestore *tr = g_new0(SkTabRestore, 1);
    tr->ctx = ctx;
    tr->session_uuid = g_strdup(si->session_uuid);
    tr->tmux_name = g_strdup(si->name);
    tr->app_window = orphan_win;
    tr->index = (int)ctx->tab_restores->len;

    g_ptr_array_add(ctx->tab_restores, tr);
  }

  /* Also update state to include orphans (add to environment). */
  SkEnvironment *env = find_environment(ctx->state, ctx->environment);
  if (env != NULL)
  {
    /* Create a new window in state for the orphaned sessions. */
    SkWindow *orphan_state_win = sk_window_new(NULL, "Ungrouped sessions");
    orphan_state_win->tabs = g_new0(SkTab *, reconcile->orphaned->len + 1);
    orphan_state_win->n_tabs = (int)reconcile->orphaned->len;

    for (guint i = 0; i < reconcile->orphaned->len; i++)
    {
      SkSessionInfo *si = g_ptr_array_index(reconcile->orphaned, i);
      orphan_state_win->tabs[i] = sk_tab_new(
          si->session_uuid, si->name, si->session_name ? si->session_name : si->name, (int)i);
    }

    /* Append window to environment. */
    int n = env->n_windows;
    env->windows = g_renew(SkWindow *, env->windows, n + 2);
    env->windows[n] = orphan_state_win;
    env->windows[n + 1] = NULL;
    env->n_windows = n + 1;
  }
}

/* ------------------------------------------------------------------ */
/* Flow completion / failure                                           */
/* ------------------------------------------------------------------ */

static void
flow_complete(SkConnectContext *ctx, bool success, GError *error)
{
  ctx->connected = success;

  /* FR-CONN-16: Dismiss feedback overlay. */
  if (ctx->feedback != NULL)
  {
    if (success)
    {
      sk_conn_feedback_set_phase(ctx->feedback, SK_CONN_PHASE_DONE);
    }
    else if (error != NULL)
    {
      sk_conn_feedback_set_error(ctx->feedback, error->message);
    }
    sk_conn_feedback_free(ctx->feedback);
    ctx->feedback = NULL;
  }

  /* Save state after successful restore. */
  if (success && ctx->state != NULL)
  {
    GError *save_err = NULL;
    /* Save local cache (FR-STATE-01). */
    sk_state_save_local_cache(ctx->state, ctx->host_fingerprint, &save_err);
    g_clear_error(&save_err);

    /* Save to server. */
    save_state_to_server(ctx, &save_err);
    g_clear_error(&save_err);
  }

  /* Update recent connections (Appendix A.3). */
  if (success)
  {
    GError *recent_err = NULL;
    SkRecentConnections *recent = sk_recent_load(&recent_err);
    g_clear_error(&recent_err);
    if (recent == NULL)
    {
      recent = sk_recent_new();
    }
    sk_recent_add(recent, ctx->hostname, ctx->username, ctx->port, NULL, ctx->host_fingerprint);
    sk_recent_save(recent, NULL);
    sk_recent_free(recent);
  }

  SK_LOG_INFO(SK_LOG_COMPONENT_UI, "connection flow %s for %s", success ? "completed" : "failed",
              ctx->hostname);

  /* Invoke callback. */
  if (ctx->done_cb != NULL)
  {
    ctx->done_cb(ctx, success, error, ctx->done_user_data);
  }

  g_clear_error(&error);
}

static void
flow_fail(SkConnectContext *ctx, GError *error)
{
  SK_LOG_ERROR(SK_LOG_COMPONENT_UI, "connection flow failed: %s",
               error ? error->message : "unknown");

  /* Show error dialog (FR-CONN-17, FR-CONN-18). */
  if (ctx->parent_window != NULL && error != NULL)
  {
    sk_dialog_error(ctx->parent_window, "Connection Failed", error->message);
  }

  flow_complete(ctx, false, error);
}

/* ------------------------------------------------------------------ */
/* Helper: build SSH options from context                              */
/* ------------------------------------------------------------------ */

static SkSshOptions
build_ssh_options(SkConnectContext *ctx)
{
  SkSshOptions opts = {
    .hostname = ctx->hostname,
    .port = ctx->port,
    .username = ctx->username,
    .identity_file = ctx->identity_file,
    .auth_methods = SK_AUTH_METHOD_ALL,
    .keepalive_interval = ctx->config->ssh_keepalive_interval,
    .keepalive_count_max = ctx->config->ssh_keepalive_count_max,
    .connect_timeout = ctx->config->ssh_connect_timeout,

    /* UI callbacks for dialogs. */
    .host_key_unknown_cb = ui_host_key_unknown_cb,
    .host_key_other_cb = ui_host_key_other_cb,
    .password_cb = ui_password_cb,
    .kbd_interactive_cb = ui_kbd_interactive_cb,
    .passphrase_cb = ui_passphrase_cb,
    .cb_user_data = ctx,
  };
  return opts;
}

/* ------------------------------------------------------------------ */
/* Helper: ProxyJump support (FR-PROXY-01..02)                         */
/* ------------------------------------------------------------------ */

/**
 * Set up ProxyCommand for single-hop ProxyJump.
 * Delegates to the system ssh binary for the intermediate hop.
 * FR-PROXY-01: Single-hop via SSH_OPTIONS_PROXYCOMMAND.
 * INV-CONN-3: Only exception for invoking ssh binary is ProxyCommand.
 */
static void
setup_proxy_command(SkSshOptions *opts G_GNUC_UNUSED, const char *proxy_jump G_GNUC_UNUSED)
{
  /* ProxyJump is translated to ProxyCommand by libssh when
   * SSH_OPTIONS_PROXYCOMMAND is set. The ssh_config parsing in
   * libssh handles ProxyJump -> ProxyCommand translation natively.
   *
   * If the user specifies proxy_jump explicitly (not from ssh_config),
   * we construct the ProxyCommand here.
   *
   * TODO: implement when libssh SSH_OPTIONS_PROXYCOMMAND integration
   * is ready. The command would be:
   *   ssh -W %h:%p <proxy_jump>
   */
  SK_LOG_DEBUG(SK_LOG_COMPONENT_SSH, "ProxyJump configured via: %s",
               proxy_jump ? proxy_jump : "(ssh_config)");
}

/* ------------------------------------------------------------------ */
/* Helper: remote state file path                                      */
/* ------------------------------------------------------------------ */

static char *
build_remote_state_path(const char *client_id)
{
  return g_strdup_printf("~/.terminal-state/%s.json", client_id);
}

/* ------------------------------------------------------------------ */
/* Helper: remote file I/O with SFTP/shell fallback (FR-CONN-20)       */
/* ------------------------------------------------------------------ */

static char *
read_remote_file(SkConnectContext *ctx, const char *path, size_t *out_len, GError **error)
{
  char *data = NULL;

  if (ctx->sftp != NULL)
  {
    /* FR-STATE-06: Use SFTP for async I/O. */
    if (sk_sftp_read_file(ctx->sftp, path, &data, out_len, error))
    {
      return data;
    }
    /* If SFTP read failed, fall through to shell fallback. */
    g_clear_error(error);
  }

  /* FR-CONN-20: Shell fallback. */
  if (ctx->control_conn != NULL)
  {
    if (sk_ssh_shell_read_file(ctx->control_conn, path, &data, out_len, error))
    {
      return data;
    }
  }

  return NULL;
}

static bool
write_remote_file(SkConnectContext *ctx, const char *path, const char *data, size_t len,
                  GError **error)
{
  if (ctx->sftp != NULL)
  {
    return sk_sftp_write_file(ctx->sftp, path, data, len, SK_FILE_PERMISSIONS, error);
  }

  /* FR-CONN-20: Shell fallback. */
  if (ctx->control_conn != NULL)
  {
    return sk_ssh_shell_write_file(ctx->control_conn, path, data, len, SK_FILE_PERMISSIONS, error);
  }

  g_set_error(error, SK_CONNECT_ERROR, SK_CONNECT_ERROR_SFTP,
              "No connection available for file write");
  return false;
}

/* ------------------------------------------------------------------ */
/* Helper: load state from server (FR-STATE-01..02)                    */
/* ------------------------------------------------------------------ */

static SkStateFile *
load_state_from_server(SkConnectContext *ctx, GError **error)
{
  /* Ensure remote state directory exists. */
  if (ctx->sftp != NULL)
  {
    GError *mkdir_err = NULL;
    sk_sftp_mkdir_p(ctx->sftp, SK_REMOTE_STATE_DIR, SK_DIR_PERMISSIONS, &mkdir_err);
    g_clear_error(&mkdir_err);
  }

  char *state_path = build_remote_state_path(ctx->client_id);
  size_t data_len = 0;
  char *data = read_remote_file(ctx, state_path, &data_len, error);
  g_free(state_path);

  if (data == NULL)
  {
    return NULL;
  }

  /* Parse the JSON state (FR-STATE-08: schema versioning). */
  SkStateFile *state = sk_state_from_json(data, error);
  g_free(data);

  return state;
}

/* ------------------------------------------------------------------ */
/* Helper: save state to server                                        */
/* ------------------------------------------------------------------ */

static bool
save_state_to_server(SkConnectContext *ctx, GError **error)
{
  if (ctx->state == NULL)
    return true;

  /* Serialize state to JSON. */
  char *json = sk_state_to_json(ctx->state);
  if (json == NULL)
  {
    g_set_error(error, SK_CONNECT_ERROR, SK_CONNECT_ERROR_STATE_CORRUPT,
                "Failed to serialize state to JSON");
    return false;
  }

  /* Write to server (INV-STATE-1: atomic via tmp+rename). */
  char *state_path = build_remote_state_path(ctx->client_id);
  bool ok = write_remote_file(ctx, state_path, json, strlen(json), error);
  g_free(state_path);
  g_free(json);

  if (ok)
  {
    /* Also update local cache (FR-STATE-01). */
    GError *cache_err = NULL;
    sk_state_save_local_cache(ctx->state, ctx->host_fingerprint, &cache_err);
    g_clear_error(&cache_err);
  }

  return ok;
}

/* ------------------------------------------------------------------ */
/* Helper: free a tab restore context                                  */
/* ------------------------------------------------------------------ */

static void
tab_restore_free(SkTabRestore *tr)
{
  if (tr == NULL)
    return;

  /* Terminal ownership transferred to app window, but clean up
   * if it was never added. */
  if (tr->terminal != NULL && tr->app_tab == NULL)
  {
    sk_terminal_tab_free(tr->terminal);
  }

  if (tr->tmux_session != NULL)
  {
    sk_tmux_session_free(tr->tmux_session);
  }

  if (tr->ssh_channel != NULL)
  {
    sk_ssh_channel_free(tr->ssh_channel);
  }

  if (tr->ssh_conn != NULL)
  {
    sk_ssh_connection_free(tr->ssh_conn);
  }

  g_free(tr->session_uuid);
  g_free(tr->tmux_name);
  g_free(tr);
}

/* ------------------------------------------------------------------ */
/* Signal handlers for graceful shutdown                                */
/* ------------------------------------------------------------------ */

/** Global context pointer for signal handler (single-instance app). */
static SkConnectContext *g_signal_ctx = NULL;

static gboolean
on_signal_shutdown(gpointer user_data G_GNUC_UNUSED)
{
  if (g_signal_ctx != NULL)
  {
    SK_LOG_WARN(SK_LOG_COMPONENT_UI, "signal received, initiating emergency shutdown");
    sk_connect_emergency_shutdown(g_signal_ctx);
  }
  /* Return FALSE to let default handler run (terminate). */
  return FALSE;
}

static void
install_signal_handlers(SkConnectContext *ctx)
{
  g_signal_ctx = ctx;
  /* Use GLib's signal source for safe main-loop handling. */
  ctx->sigterm_handler_id = g_unix_signal_add(SIGTERM, on_signal_shutdown, ctx);
  ctx->sigint_handler_id = g_unix_signal_add(SIGINT, on_signal_shutdown, ctx);
}

static void
remove_signal_handlers(SkConnectContext *ctx)
{
  if (ctx->sigterm_handler_id > 0)
  {
    g_source_remove(ctx->sigterm_handler_id);
    ctx->sigterm_handler_id = 0;
  }
  if (ctx->sigint_handler_id > 0)
  {
    g_source_remove(ctx->sigint_handler_id);
    ctx->sigint_handler_id = 0;
  }
  if (g_signal_ctx == ctx)
  {
    g_signal_ctx = NULL;
  }
}

/* ------------------------------------------------------------------ */
/* UI callback adapters — run dialogs on main thread                   */
/* ------------------------------------------------------------------ */

/**
 * Data struct for passing dialog calls to the main thread and
 * waiting for the result via a mutex/cond.
 */
typedef struct
{
  GMutex mutex;
  GCond cond;
  bool done;

  /* Dialog-specific data. */
  GtkWindow *parent;
  const char *hostname;
  const char *fingerprint;
  const char *key_type;
  const char *old_key_type;
  const char *prompt;

  /* MFA-specific. */
  const char *mfa_name;
  const char *mfa_instruction;
  const char **mfa_prompts;
  const gboolean *mfa_show_input;
  int mfa_n_prompts;

  /* Result. */
  gboolean result_bool;
  char *result_string;
  char **result_strings;
} DialogCallData;

static gboolean
run_host_key_unknown_dialog(gpointer data)
{
  DialogCallData *d = data;

  SkHostKeyDialogResult result =
      sk_dialog_host_key_unknown(d->parent, d->hostname, d->fingerprint, d->key_type);

  d->result_bool = (result == SK_HOST_KEY_ACCEPT_SAVE || result == SK_HOST_KEY_CONNECT_ONCE);

  g_mutex_lock(&d->mutex);
  d->done = true;
  g_cond_signal(&d->cond);
  g_mutex_unlock(&d->mutex);

  return G_SOURCE_REMOVE;
}

static gboolean
ui_host_key_unknown_cb(SkSshConnection *conn G_GNUC_UNUSED, const char *fingerprint,
                       const char *key_type, gpointer user_data)
{
  SkConnectContext *ctx = user_data;

  DialogCallData d = { 0 };
  g_mutex_init(&d.mutex);
  g_cond_init(&d.cond);
  d.parent = ctx->parent_window;
  d.hostname = ctx->hostname;
  d.fingerprint = fingerprint;
  d.key_type = key_type;

  g_idle_add(run_host_key_unknown_dialog, &d);

  g_mutex_lock(&d.mutex);
  while (!d.done)
  {
    g_cond_wait(&d.cond, &d.mutex);
  }
  g_mutex_unlock(&d.mutex);

  g_mutex_clear(&d.mutex);
  g_cond_clear(&d.cond);

  return d.result_bool;
}

static gboolean
run_host_key_other_dialog(gpointer data)
{
  DialogCallData *d = data;

  /* FR-CONN-04: Show warning for different key type. */
  SkHostKeyDialogResult result =
      sk_dialog_host_key_unknown(d->parent, d->hostname, d->fingerprint, d->key_type);
  d->result_bool = (result != SK_HOST_KEY_REJECT);

  g_mutex_lock(&d->mutex);
  d->done = true;
  g_cond_signal(&d->cond);
  g_mutex_unlock(&d->mutex);

  return G_SOURCE_REMOVE;
}

static gboolean
ui_host_key_other_cb(SkSshConnection *conn G_GNUC_UNUSED, const char *fingerprint,
                     const char *old_type G_GNUC_UNUSED, const char *new_type, gpointer user_data)
{
  SkConnectContext *ctx = user_data;

  DialogCallData d = { 0 };
  g_mutex_init(&d.mutex);
  g_cond_init(&d.cond);
  d.parent = ctx->parent_window;
  d.hostname = ctx->hostname;
  d.fingerprint = fingerprint;
  d.key_type = new_type;

  g_idle_add(run_host_key_other_dialog, &d);

  g_mutex_lock(&d.mutex);
  while (!d.done)
  {
    g_cond_wait(&d.cond, &d.mutex);
  }
  g_mutex_unlock(&d.mutex);

  g_mutex_clear(&d.mutex);
  g_cond_clear(&d.cond);

  return d.result_bool;
}

static gboolean
run_password_dialog(gpointer data)
{
  DialogCallData *d = data;
  d->result_string = sk_dialog_auth_password(d->parent, d->prompt);

  g_mutex_lock(&d->mutex);
  d->done = true;
  g_cond_signal(&d->cond);
  g_mutex_unlock(&d->mutex);

  return G_SOURCE_REMOVE;
}

static char *
ui_password_cb(SkSshConnection *conn G_GNUC_UNUSED, const char *prompt, gpointer user_data)
{
  SkConnectContext *ctx = user_data;

  DialogCallData d = { 0 };
  g_mutex_init(&d.mutex);
  g_cond_init(&d.cond);
  d.parent = ctx->parent_window;
  d.prompt = prompt;

  g_idle_add(run_password_dialog, &d);

  g_mutex_lock(&d.mutex);
  while (!d.done)
  {
    g_cond_wait(&d.cond, &d.mutex);
  }
  g_mutex_unlock(&d.mutex);

  g_mutex_clear(&d.mutex);
  g_cond_clear(&d.cond);

  return d.result_string;
}

static gboolean
run_mfa_dialog(gpointer data)
{
  DialogCallData *d = data;
  d->result_strings = sk_dialog_auth_mfa(d->parent, d->mfa_name, d->mfa_instruction, d->mfa_prompts,
                                         d->mfa_show_input, d->mfa_n_prompts);

  g_mutex_lock(&d->mutex);
  d->done = true;
  g_cond_signal(&d->cond);
  g_mutex_unlock(&d->mutex);

  return G_SOURCE_REMOVE;
}

static char **
ui_kbd_interactive_cb(SkSshConnection *conn G_GNUC_UNUSED, const char *name,
                      const char *instruction, const char **prompts, const gboolean *show_input,
                      int n_prompts, gpointer user_data)
{
  SkConnectContext *ctx = user_data;

  DialogCallData d = { 0 };
  g_mutex_init(&d.mutex);
  g_cond_init(&d.cond);
  d.parent = ctx->parent_window;
  d.mfa_name = name;
  d.mfa_instruction = instruction;
  d.mfa_prompts = prompts;
  d.mfa_show_input = show_input;
  d.mfa_n_prompts = n_prompts;

  g_idle_add(run_mfa_dialog, &d);

  g_mutex_lock(&d.mutex);
  while (!d.done)
  {
    g_cond_wait(&d.cond, &d.mutex);
  }
  g_mutex_unlock(&d.mutex);

  g_mutex_clear(&d.mutex);
  g_cond_clear(&d.cond);

  return d.result_strings;
}

static gboolean
run_passphrase_dialog(gpointer data)
{
  DialogCallData *d = data;
  d->result_string = sk_dialog_auth_password(d->parent, d->prompt);

  g_mutex_lock(&d->mutex);
  d->done = true;
  g_cond_signal(&d->cond);
  g_mutex_unlock(&d->mutex);

  return G_SOURCE_REMOVE;
}

static char *
ui_passphrase_cb(SkSshConnection *conn G_GNUC_UNUSED, const char *key_path, gpointer user_data)
{
  SkConnectContext *ctx = user_data;

  char *prompt = g_strdup_printf("Enter passphrase for key '%s':", key_path);

  DialogCallData d = { 0 };
  g_mutex_init(&d.mutex);
  g_cond_init(&d.cond);
  d.parent = ctx->parent_window;
  d.prompt = prompt;

  g_idle_add(run_passphrase_dialog, &d);

  g_mutex_lock(&d.mutex);
  while (!d.done)
  {
    g_cond_wait(&d.cond, &d.mutex);
  }
  g_mutex_unlock(&d.mutex);

  g_mutex_clear(&d.mutex);
  g_cond_clear(&d.cond);
  g_free(prompt);

  return d.result_string;
}
