// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ssh.h
 * @brief SSH connection layer for shellkeep.
 *
 * Provides connection management, host key verification, authentication,
 * channel/PTY handling, SFTP file operations, keepalive, and crypto
 * configuration — all built on libssh.
 *
 * Threading model (INV-IO-1): All blocking operations run in GTask worker
 * threads.  Data I/O uses g_io_add_watch() on the SSH file descriptor.
 * The GTK main thread is NEVER blocked.
 */

#ifndef SHELLKEEP_SK_SSH_H
#define SHELLKEEP_SK_SSH_H

#include <glib.h>

#include <gio/gio.h>
#include <stdbool.h>
#include <stddef.h>

G_BEGIN_DECLS

/* ------------------------------------------------------------------ */
/*  Opaque types                                                       */
/* ------------------------------------------------------------------ */

/** Opaque SSH connection handle wrapping an ssh_session. */
typedef struct _SkSshConnection SkSshConnection;

/** Opaque SSH channel handle wrapping an ssh_channel. */
typedef struct _SkSshChannel SkSshChannel;

/** Opaque SFTP session handle wrapping an sftp_session. */
typedef struct _SkSftpSession SkSftpSession;

/* ------------------------------------------------------------------ */
/*  Error domain                                                       */
/* ------------------------------------------------------------------ */

#define SK_SSH_ERROR (sk_ssh_error_quark())
GQuark sk_ssh_error_quark(void);

typedef enum
{
  SK_SSH_ERROR_CONNECT,      /**< Connection to host failed */
  SK_SSH_ERROR_HOST_KEY,     /**< Host key verification failed */
  SK_SSH_ERROR_AUTH,         /**< Authentication failed */
  SK_SSH_ERROR_CHANNEL,      /**< Channel open/operation failed */
  SK_SSH_ERROR_SFTP,         /**< SFTP operation failed */
  SK_SSH_ERROR_TIMEOUT,      /**< Operation timed out */
  SK_SSH_ERROR_PROTOCOL,     /**< Protocol-level error */
  SK_SSH_ERROR_DISCONNECTED, /**< Connection has been lost */
  SK_SSH_ERROR_CRYPTO,       /**< No acceptable crypto algorithms */
} SkSshErrorCode;

/* ------------------------------------------------------------------ */
/*  Host key verification  (FR-CONN-01..05)                            */
/* ------------------------------------------------------------------ */

/** Result of host key verification. */
typedef enum
{
  SK_HOST_KEY_OK,      /**< Key matches known_hosts. */
  SK_HOST_KEY_CHANGED, /**< Key differs from known entry (possible MITM). */
  SK_HOST_KEY_UNKNOWN, /**< Host not in known_hosts (TOFU). */
  SK_HOST_KEY_OTHER,   /**< Different key type from known entry. */
  SK_HOST_KEY_ERROR,   /**< Internal error during verification. */
} SkHostKeyStatus;

/**
 * Callback invoked when the host key is unknown (TOFU dialog).
 *
 * @param conn        Connection handle.
 * @param fingerprint SHA-256 fingerprint string.
 * @param key_type    Key type string (e.g. "ssh-ed25519").
 * @param user_data   User-supplied pointer.
 * @return TRUE to accept and save, FALSE to reject.
 */
typedef gboolean (*SkHostKeyUnknownCb)(SkSshConnection *conn, const char *fingerprint,
                                       const char *key_type, gpointer user_data);

/**
 * Callback invoked when the host key type changed (OTHER dialog).
 *
 * @param conn            Connection handle.
 * @param fingerprint     SHA-256 fingerprint of the new key.
 * @param old_key_type    Previous key type.
 * @param new_key_type    Current key type.
 * @param user_data       User-supplied pointer.
 * @return TRUE to accept new type, FALSE to reject.
 */
typedef gboolean (*SkHostKeyOtherCb)(SkSshConnection *conn, const char *fingerprint,
                                     const char *old_key_type, const char *new_key_type,
                                     gpointer user_data);

/* ------------------------------------------------------------------ */
/*  Authentication  (FR-CONN-06..12)                                   */
/* ------------------------------------------------------------------ */

/** Result of an authentication attempt. */
typedef enum
{
  SK_AUTH_SUCCESS,   /**< Authenticated. */
  SK_AUTH_DENIED,    /**< All methods exhausted. */
  SK_AUTH_PARTIAL,   /**< More auth rounds needed (MFA). */
  SK_AUTH_ERROR,     /**< Internal error. */
  SK_AUTH_CANCELLED, /**< User cancelled the dialog. */
} SkAuthResult;

/** Which authentication methods to enable (bitmask). */
typedef enum
{
  SK_AUTH_METHOD_AGENT = 1 << 0,
  SK_AUTH_METHOD_PUBKEY = 1 << 1,
  SK_AUTH_METHOD_PASSWORD = 1 << 2,
  SK_AUTH_METHOD_KEYBOARD_INTERACTIVE = 1 << 3,
  SK_AUTH_METHOD_ALL = 0x0F,
} SkAuthMethod;

/**
 * Callback to request a password from the user (GTK masked dialog).
 *
 * @param conn       Connection handle.
 * @param prompt     Prompt string from the server.
 * @param user_data  User-supplied pointer.
 * @return Newly allocated password string, or NULL if cancelled.
 *         Caller will explicit_bzero + g_free.
 */
typedef char *(*SkPasswordCb)(SkSshConnection *conn, const char *prompt, gpointer user_data);

/**
 * Callback for keyboard-interactive prompts (MFA/2FA).
 *
 * @param conn         Connection handle.
 * @param name         Name of the auth request (may be empty).
 * @param instruction  Instruction string from the server.
 * @param prompts      Array of prompt strings.
 * @param show_input   Array of booleans; FALSE means masked input.
 * @param n_prompts    Number of prompts.
 * @param user_data    User-supplied pointer.
 * @return Newly allocated array of response strings, or NULL if cancelled.
 *         Each element will be explicit_bzero + g_free'd by the caller.
 */
typedef char **(*SkKeyboardInteractiveCb)(SkSshConnection *conn, const char *name,
                                          const char *instruction, const char **prompts,
                                          const gboolean *show_input, int n_prompts,
                                          gpointer user_data);

/**
 * Callback invoked for passphrase-protected private keys.
 *
 * @param conn       Connection handle.
 * @param key_path   Path to the key file.
 * @param user_data  User-supplied pointer.
 * @return Newly allocated passphrase, or NULL if cancelled.
 */
typedef char *(*SkPassphraseCb)(SkSshConnection *conn, const char *key_path, gpointer user_data);

/* ------------------------------------------------------------------ */
/*  Connection options                                                 */
/* ------------------------------------------------------------------ */

/** Configuration for a new SSH connection. */
typedef struct
{
  const char *hostname;      /**< Remote hostname or IP. */
  int port;                  /**< Port (0 = default 22). */
  const char *username;      /**< Remote username (NULL = current). */
  const char *identity_file; /**< Explicit key path, or NULL. */

  /** Bitmask of SkAuthMethod. Default: SK_AUTH_METHOD_ALL. */
  unsigned int auth_methods;

  /** Keepalive interval in seconds (0 = use ssh_config or default 15). */
  int keepalive_interval;
  /** Max keepalive misses (0 = use ssh_config or default 3). */
  int keepalive_count_max;

  /** Connection timeout in seconds (0 = use ssh_config or default 30). */
  int connect_timeout;

  /* UI callbacks — all may be NULL (non-interactive mode). */
  SkHostKeyUnknownCb host_key_unknown_cb;
  SkHostKeyOtherCb host_key_other_cb;
  SkPasswordCb password_cb;
  SkKeyboardInteractiveCb kbd_interactive_cb;
  SkPassphraseCb passphrase_cb;
  gpointer cb_user_data;
} SkSshOptions;

/* ------------------------------------------------------------------ */
/*  Connection lifecycle                                               */
/* ------------------------------------------------------------------ */

/**
 * Create a new SSH connection object.
 * Does NOT connect — call sk_ssh_connection_connect() afterwards.
 *
 * @param opts   Connection options (copied internally).
 * @param error  Return location for a GError.
 * @return New connection, or NULL on error.
 */
SkSshConnection *sk_ssh_connection_new(const SkSshOptions *opts, GError **error);

/**
 * Connect, verify host key, and authenticate (blocking).
 * MUST be called from a GTask worker thread (INV-IO-1).
 *
 * @param conn   Connection handle.
 * @param error  Return location for a GError.
 * @return TRUE on success.
 */
gboolean sk_ssh_connection_connect(SkSshConnection *conn, GError **error);

/**
 * Disconnect gracefully.  Safe to call from any thread.
 *
 * @param conn  Connection handle.
 */
void sk_ssh_connection_disconnect(SkSshConnection *conn);

/**
 * Free all resources associated with the connection.
 * Disconnects if still connected.
 *
 * @param conn  Connection handle (may be NULL).
 */
void sk_ssh_connection_free(SkSshConnection *conn);

/**
 * Get the underlying file descriptor for g_io_add_watch().
 *
 * @param conn  Connection handle.
 * @return fd, or -1 if not connected.
 */
int sk_ssh_connection_get_fd(SkSshConnection *conn);

/**
 * Check whether the connection is currently established.
 *
 * @param conn  Connection handle.
 * @return TRUE if connected.
 */
gboolean sk_ssh_connection_is_connected(SkSshConnection *conn);

/**
 * Get the last error message from libssh.
 *
 * @param conn  Connection handle.
 * @return Error string (owned by libssh; do not free).
 */
const char *sk_ssh_connection_get_error(SkSshConnection *conn);

/**
 * Perform host key verification only (FR-CONN-01..05).
 *
 * @param conn  Connection handle (must be connected but not yet verified).
 * @return Host key status enum.
 */
SkHostKeyStatus sk_ssh_verify_host_key(SkSshConnection *conn);

/**
 * Accept and save the current host key to known_hosts.
 * Used after SK_HOST_KEY_UNKNOWN when user chooses "Accept and save".
 *
 * @param conn   Connection handle.
 * @param error  Return location for a GError.
 * @return TRUE on success.
 */
gboolean sk_ssh_accept_host_key(SkSshConnection *conn, GError **error);

/**
 * Get the fingerprint of the server's host key.
 *
 * @param conn  Connection handle (must be connected).
 * @return Newly allocated fingerprint string (SHA-256), or NULL on error.
 *         Caller must g_free().
 */
char *sk_ssh_get_host_fingerprint(SkSshConnection *conn);

/**
 * Get the host key type as a human-readable string.
 *
 * @param conn  Connection handle (must be connected).
 * @return Newly allocated string (e.g. "ssh-ed25519"), or NULL.
 *         Caller must g_free().
 */
char *sk_ssh_get_host_key_type(SkSshConnection *conn);

/* ------------------------------------------------------------------ */
/*  Channel / PTY  (FR-TERMINAL-16)                                    */
/* ------------------------------------------------------------------ */

/**
 * Open a new session channel with a PTY.
 *
 * @param conn   Connection handle (authenticated).
 * @param cols   Initial terminal columns.
 * @param rows   Initial terminal rows.
 * @param error  Return location for a GError.
 * @return New channel, or NULL on error.
 */
SkSshChannel *sk_ssh_channel_open(SkSshConnection *conn, int cols, int rows, GError **error);

/**
 * Request a shell on the channel (after PTY allocation).
 *
 * @param channel  Channel handle.
 * @param error    Return location for a GError.
 * @return TRUE on success.
 */
gboolean sk_ssh_channel_request_shell(SkSshChannel *channel, GError **error);

/**
 * Execute a command on the channel (alternative to shell).
 *
 * @param channel  Channel handle.
 * @param command  Command string.
 * @param error    Return location for a GError.
 * @return TRUE on success.
 */
gboolean sk_ssh_channel_exec(SkSshChannel *channel, const char *command, GError **error);

/**
 * Change PTY size (sends SIGWINCH to remote).
 *
 * @param channel  Channel handle.
 * @param cols     New column count.
 * @param rows     New row count.
 * @param error    Return location for a GError.
 * @return TRUE on success.
 */
gboolean sk_ssh_channel_resize_pty(SkSshChannel *channel, int cols, int rows, GError **error);

/**
 * Non-blocking read from channel.
 *
 * @param channel  Channel handle.
 * @param buf      Output buffer.
 * @param bufsize  Size of output buffer.
 * @return Number of bytes read, 0 if no data available, or -1 on error/EOF.
 */
int sk_ssh_channel_read_nonblocking(SkSshChannel *channel, void *buf, size_t bufsize);

/**
 * Write data to channel.
 *
 * @param channel  Channel handle.
 * @param data     Data to write.
 * @param len      Length of data.
 * @return Number of bytes written, or -1 on error.
 */
int sk_ssh_channel_write(SkSshChannel *channel, const void *data, size_t len);

/**
 * Check if channel is open and not at EOF.
 *
 * @param channel  Channel handle.
 * @return TRUE if channel is usable.
 */
gboolean sk_ssh_channel_is_open(SkSshChannel *channel);

/**
 * Get the exit status of the remote process (-1 if not yet exited).
 *
 * @param channel  Channel handle.
 * @return Exit status, or -1.
 */
int sk_ssh_channel_get_exit_status(SkSshChannel *channel);

/**
 * Close and free a channel.
 *
 * @param channel  Channel handle (may be NULL).
 */
void sk_ssh_channel_free(SkSshChannel *channel);

/* ------------------------------------------------------------------ */
/*  SFTP  (FR-STATE-06, FR-CONN-20, FR-COMPAT-10)                     */
/* ------------------------------------------------------------------ */

/**
 * Open an SFTP session on an existing SSH connection (blocking).
 *
 * @param conn   Authenticated SSH connection.
 * @param error  Return location for a GError.
 * @return New SFTP session, or NULL if SFTP is unavailable.
 */
SkSftpSession *sk_sftp_session_new(SkSshConnection *conn, GError **error);

/**
 * Check whether the session supports posix-rename@openssh.com.
 *
 * @param sftp  SFTP session.
 * @return TRUE if the extension is available.
 */
gboolean sk_sftp_has_posix_rename(SkSftpSession *sftp);

/**
 * Read an entire remote file into memory.
 *
 * @param sftp       SFTP session.
 * @param path       Remote file path.
 * @param out_data   Return location for file contents (g_free() when done).
 * @param out_len    Return location for file length.
 * @param error      Return location for a GError.
 * @return TRUE on success.
 */
gboolean sk_sftp_read_file(SkSftpSession *sftp, const char *path, char **out_data, size_t *out_len,
                           GError **error);

/**
 * Write data to a remote file atomically (tmp + rename).
 *
 * @param sftp   SFTP session.
 * @param path   Destination file path.
 * @param data   Data to write.
 * @param len    Length of data.
 * @param mode   File permissions (e.g. 0600).
 * @param error  Return location for a GError.
 * @return TRUE on success.
 */
gboolean sk_sftp_write_file(SkSftpSession *sftp, const char *path, const char *data, size_t len,
                            int mode, GError **error);

/**
 * Rename a remote file atomically using posix-rename if available.
 *
 * @param sftp      SFTP session.
 * @param old_path  Source path.
 * @param new_path  Destination path.
 * @param error     Return location for a GError.
 * @return TRUE on success.
 */
gboolean sk_sftp_rename(SkSftpSession *sftp, const char *old_path, const char *new_path,
                        GError **error);

/**
 * Check whether a remote path exists.
 *
 * @param sftp  SFTP session.
 * @param path  Remote path.
 * @return TRUE if the path exists.
 */
gboolean sk_sftp_exists(SkSftpSession *sftp, const char *path);

/**
 * Create a remote directory (and parents if needed).
 *
 * @param sftp   SFTP session.
 * @param path   Remote directory path.
 * @param mode   Directory permissions (e.g. 0700).
 * @param error  Return location for a GError.
 * @return TRUE on success (or if dir already exists).
 */
gboolean sk_sftp_mkdir_p(SkSftpSession *sftp, const char *path, int mode, GError **error);

/**
 * Close and free an SFTP session.
 *
 * @param sftp  SFTP session (may be NULL).
 */
void sk_sftp_session_free(SkSftpSession *sftp);

/* ------------------------------------------------------------------ */
/*  Shell fallback for file ops  (FR-CONN-20)                          */
/* ------------------------------------------------------------------ */

/**
 * Read a remote file via shell commands (fallback when SFTP unavailable).
 *
 * @param conn      SSH connection.
 * @param path      Remote file path.
 * @param out_data  Return location for file contents.
 * @param out_len   Return location for file length.
 * @param error     Return location for a GError.
 * @return TRUE on success.
 */
gboolean sk_ssh_shell_read_file(SkSshConnection *conn, const char *path, char **out_data,
                                size_t *out_len, GError **error);

/**
 * Write a remote file via shell commands (fallback when SFTP unavailable).
 *
 * @param conn   SSH connection.
 * @param path   Remote file path.
 * @param data   Data to write.
 * @param len    Length of data.
 * @param mode   File permissions (e.g. 0600).
 * @param error  Return location for a GError.
 * @return TRUE on success.
 */
gboolean sk_ssh_shell_write_file(SkSshConnection *conn, const char *path, const char *data,
                                 size_t len, int mode, GError **error);

/* ------------------------------------------------------------------ */
/*  Crypto configuration  (FR-COMPAT-10..11)                           */
/* ------------------------------------------------------------------ */

/**
 * Apply the shellkeep-approved cipher/MAC/KEX lists to a connection.
 * Called internally during connection setup.
 * Rejects obsolete algorithms (arcfour, 3des-cbc, etc.).
 *
 * @param conn   Connection handle (before connect).
 * @param error  Return location for a GError.
 * @return TRUE on success.
 */
gboolean sk_ssh_configure_crypto(SkSshConnection *conn, GError **error);

/* ------------------------------------------------------------------ */
/*  Keepalive                                                          */
/* ------------------------------------------------------------------ */

/**
 * Configure SSH and TCP keepalive on the connection.
 * Sets TCP_NODELAY, TCP_KEEPALIVE, and SSH-level keepalive.
 *
 * @param conn   Connection handle (after connect).
 * @param error  Return location for a GError.
 * @return TRUE on success.
 */
gboolean sk_ssh_configure_keepalive(SkSshConnection *conn, GError **error);

/* ------------------------------------------------------------------ */
/*  Async helpers  (INV-IO-1)                                          */
/* ------------------------------------------------------------------ */

/**
 * Connect asynchronously via GTask.
 *
 * @param conn         Connection handle.
 * @param cancellable  Optional GCancellable.
 * @param callback     Callback when done.
 * @param user_data    Data for callback.
 */
void sk_ssh_connection_connect_async(SkSshConnection *conn, GCancellable *cancellable,
                                     GAsyncReadyCallback callback, gpointer user_data);

/**
 * Finish an async connect.
 *
 * @param conn    Connection handle.
 * @param result  The GAsyncResult.
 * @param error   Return location for a GError.
 * @return TRUE on success.
 */
gboolean sk_ssh_connection_connect_finish(SkSshConnection *conn, GAsyncResult *result,
                                          GError **error);

/**
 * Set up a GLib IO watch on the SSH fd for non-blocking data I/O.
 *
 * @param conn       Connection handle (connected).
 * @param callback   GIOFunc to invoke on data available.
 * @param user_data  Data for callback.
 * @return GSource ID, or 0 on error.
 */
guint sk_ssh_connection_add_io_watch(SkSshConnection *conn, GIOFunc callback, gpointer user_data);

G_END_DECLS

#endif /* SHELLKEEP_SK_SSH_H */
