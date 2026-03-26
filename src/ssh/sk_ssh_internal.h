// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ssh_internal.h
 * @brief Internal declarations shared across src/ssh/ source files.
 *
 * NOT part of the public API.  Never install this header.
 */

#ifndef SHELLKEEP_SK_SSH_INTERNAL_H
#define SHELLKEEP_SK_SSH_INTERNAL_H

#include "shellkeep/sk_ssh.h"

#include <libssh/libssh.h>
#include <libssh/sftp.h>

G_BEGIN_DECLS

/* --- Connection internals --- */

/**
 * Get the raw libssh session from a connection handle.
 * Internal use only — other src/ssh/ files need this.
 */
ssh_session sk_ssh_connection_get_session(SkSshConnection *conn);

/**
 * Get the options struct from a connection handle.
 */
const SkSshOptions *sk_ssh_connection_get_opts(SkSshConnection *conn);

/* --- Authentication (implemented in sk_ssh_auth.c) --- */

/**
 * Run the full authentication sequence (agent -> pubkey -> password -> kbd).
 * Blocking — must run in a worker thread.
 *
 * @param conn   Connected (but not authenticated) SSH connection.
 * @param error  Return location for a GError.
 * @return SK_AUTH_SUCCESS on success, other SkAuthResult values on failure.
 */
SkAuthResult sk_ssh_authenticate(SkSshConnection *conn, GError **error);

G_END_DECLS

#endif /* SHELLKEEP_SK_SSH_INTERNAL_H */
