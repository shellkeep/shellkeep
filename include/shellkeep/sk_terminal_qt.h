// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_terminal_qt.h
 * @brief Qt6 terminal widget layer -- public C/C++ API.
 *
 * Declares the Qt terminal widget classes and provides extern "C" bridge
 * functions so the pure-C backend can create and interact with the Qt
 * terminal without pulling in any C++ headers.
 *
 * C callers: use the sk_terminal_qt_* functions.
 * C++ callers: use the classes directly (SkTerminalWidget, etc.).
 */

#ifndef SHELLKEEP_SK_TERMINAL_QT_H
#define SHELLKEEP_SK_TERMINAL_QT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
/* ------------------------------------------------------------------ */
/* C++ class forward declarations                                      */
/* ------------------------------------------------------------------ */

class SkTerminalWidget;
class SkTerminalSearch;
class SkTerminalDead;
class SkTerminalTheme;

extern "C" {
#endif

/* ------------------------------------------------------------------ */
/* Opaque handle for C callers                                         */
/* ------------------------------------------------------------------ */

/** Opaque handle to the Qt terminal widget (C-side). */
typedef struct SkTerminalQtHandle SkTerminalQtHandle;

/* ------------------------------------------------------------------ */
/* C bridge: lifecycle                                                 */
/* ------------------------------------------------------------------ */

/**
 * Create a new Qt terminal widget.
 *
 * @return Opaque handle, or NULL on failure.
 */
SkTerminalQtHandle *sk_terminal_qt_new(void);

/**
 * Destroy the Qt terminal widget and free all resources.
 *
 * @param handle  Terminal handle (may be NULL).
 */
void sk_terminal_qt_free(SkTerminalQtHandle *handle);

/* ------------------------------------------------------------------ */
/* C bridge: SSH I/O                                                   */
/* ------------------------------------------------------------------ */

struct _SkSshChannel;
struct _SkSshConnection;

/**
 * Connect the terminal widget to an SSH channel for I/O.
 *
 * @param handle   Terminal handle.
 * @param fd       SSH connection file descriptor.
 * @param channel  SSH channel for read/write.
 * @return true on success.
 */
bool sk_terminal_qt_connect_ssh(SkTerminalQtHandle *handle, int fd,
                                struct _SkSshChannel *channel);

/**
 * Disconnect SSH I/O without destroying the widget.
 *
 * @param handle  Terminal handle.
 */
void sk_terminal_qt_disconnect(SkTerminalQtHandle *handle);

/**
 * Feed raw data into the terminal display.
 *
 * @param handle  Terminal handle.
 * @param data    Raw terminal data.
 * @param len     Length of data.
 */
void sk_terminal_qt_feed(SkTerminalQtHandle *handle, const char *data,
                         int len);

/* ------------------------------------------------------------------ */
/* C bridge: dead session                                              */
/* ------------------------------------------------------------------ */

/**
 * Callback for the "Create new session" button in dead overlay.
 */
typedef void (*SkTerminalQtNewSessionCb)(void *user_data);

/**
 * Set the terminal to dead session mode.
 *
 * @param handle        Terminal handle.
 * @param history_data  Raw history to feed (may be NULL).
 * @param history_len   Length of history data.
 * @param message       Banner message.
 */
void sk_terminal_qt_set_dead(SkTerminalQtHandle *handle,
                             const char *history_data, int history_len,
                             const char *message);

/**
 * Set callback for the "Create new session" button.
 */
void sk_terminal_qt_set_new_session_cb(SkTerminalQtHandle *handle,
                                       SkTerminalQtNewSessionCb cb,
                                       void *user_data);

/* ------------------------------------------------------------------ */
/* C bridge: terminal size                                             */
/* ------------------------------------------------------------------ */

/**
 * Get the current terminal dimensions.
 */
void sk_terminal_qt_get_size(SkTerminalQtHandle *handle, int *cols,
                             int *rows);

/* ------------------------------------------------------------------ */
/* C bridge: theme (using SkTheme from sk_config.h)                    */
/* ------------------------------------------------------------------ */

struct _SkTheme;

/**
 * Apply an SkTheme (C struct) to the terminal widget.
 */
void sk_terminal_qt_apply_theme(SkTerminalQtHandle *handle,
                                const struct _SkTheme *theme);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* SHELLKEEP_SK_TERMINAL_QT_H */
