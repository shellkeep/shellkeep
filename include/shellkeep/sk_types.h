// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_types.h
 * @brief Common types and forward declarations shared across all layers.
 *
 * This header provides opaque type declarations so that layers can refer
 * to each other's objects without pulling in full definitions.
 */

#ifndef SK_TYPES_H
#define SK_TYPES_H

#include <glib.h>

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C"
{
#endif

  /* ------------------------------------------------------------------ */
  /* Version constants                                                   */
  /* ------------------------------------------------------------------ */

#define SK_VERSION_MAJOR 0
#define SK_VERSION_MINOR 1
#define SK_VERSION_PATCH 0
#define SK_VERSION_STRING "0.1.0"

  /* ------------------------------------------------------------------ */
  /* Application constants                                               */
  /* ------------------------------------------------------------------ */

#define SK_APPLICATION_ID "org.shellkeep.ShellKeep"
#define SK_APPLICATION_NAME "shellkeep"

  /* ------------------------------------------------------------------ */
  /* Opaque types — SSH layer (NFR-ARCH-01, NFR-ARCH-02)                */
  /* ------------------------------------------------------------------ */

  /** SSH connection handle (wraps libssh session). */
  typedef struct _SkSshConnection SkSshConnection;

  /** SSH channel handle (wraps libssh channel). */
  typedef struct _SkSshChannel SkSshChannel;

  /** SFTP session handle. */
  typedef struct _SkSftpSession SkSftpSession;

  /* ------------------------------------------------------------------ */
  /* Opaque types — Session layer                                        */
  /* ------------------------------------------------------------------ */

  /** Tmux session manager. */
  typedef struct _SkSessionManager SkSessionManager;

  /** Single tmux session. */
  typedef struct _SkTmuxSession SkTmuxSession;

  /** Control mode connection handle. */
  typedef struct _SkControlMode SkControlMode;

  /* ------------------------------------------------------------------ */
  /* Opaque types — State layer                                          */
  /* ------------------------------------------------------------------ */

  /** Persistent state file handle. */
  typedef struct _SkStateFile SkStateFile;

  /** Configuration store. */
  typedef struct _SkConfig SkConfig;

  /* ------------------------------------------------------------------ */
  /* Opaque types — Terminal layer                                       */
  /* ------------------------------------------------------------------ */

  /** VTE terminal wrapper. */
  typedef struct _SkTerminal SkTerminal;

  /* ------------------------------------------------------------------ */
  /* Opaque types — UI layer                                             */
  /* ------------------------------------------------------------------ */

  /** Main application window. */
  typedef struct _SkWindow SkWindow;

  /** Tab within a window. */
  typedef struct _SkTab SkTab;

  /* ------------------------------------------------------------------ */
  /* Common enums                                                        */
  /* ------------------------------------------------------------------ */

  /** Generic result codes used across layers. */
  typedef enum
  {
    SK_OK = 0,
    SK_ERROR_GENERIC = -1,
    SK_ERROR_IO = -2,
    SK_ERROR_TIMEOUT = -3,
    SK_ERROR_AUTH = -4,
    SK_ERROR_ALLOC = -5,
  } SkResult;

  /** Connection states. */
  typedef enum
  {
    SK_CONN_STATE_DISCONNECTED = 0,
    SK_CONN_STATE_CONNECTING,
    SK_CONN_STATE_AUTHENTICATING,
    SK_CONN_STATE_CONNECTED,
    SK_CONN_STATE_RECONNECTING,
    SK_CONN_STATE_ERROR,
  } SkConnectionState;

  /* ------------------------------------------------------------------ */
  /* GError domain                                                       */
  /* ------------------------------------------------------------------ */

#define SK_ERROR (sk_error_quark())
  GQuark sk_error_quark(void);

#ifdef __cplusplus
}
#endif

#endif /* SK_TYPES_H */
