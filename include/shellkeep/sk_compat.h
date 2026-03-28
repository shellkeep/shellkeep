// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_compat.h
 * @brief Cross-platform compatibility shims for POSIX functions.
 *
 * Provides Windows (MinGW) equivalents for POSIX functions used
 * throughout the codebase. Include this header in .c files that
 * use POSIX-specific APIs like mkdir, gmtime_r, O_CLOEXEC, etc.
 */

#ifndef SK_COMPAT_H
#define SK_COMPAT_H

#ifdef _WIN32

#include <direct.h>
#include <io.h>
#include <time.h>

/* mkdir() on Windows takes only 1 argument (no mode). */
#define mkdir(path, mode) _mkdir(path)

/* O_CLOEXEC does not exist on Windows; handles are not inherited by default. */
#ifndef O_CLOEXEC
#define O_CLOEXEC 0
#endif

/* gmtime_r / localtime_r → gmtime_s / localtime_s (note: arg order differs) */
static inline struct tm *sk_gmtime_r(const time_t *t, struct tm *result)
{
    return gmtime_s(result, t) == 0 ? result : NULL;
}
#define gmtime_r(t, res) sk_gmtime_r((t), (res))

static inline struct tm *sk_localtime_r(const time_t *t, struct tm *result)
{
    return localtime_s(result, t) == 0 ? result : NULL;
}
#define localtime_r(t, res) sk_localtime_r((t), (res))

/* fsync → _commit */
#define fsync(fd) _commit(fd)

/* fchmod — no-op on Windows */
static inline int sk_fchmod_noop(int fd, int mode) { (void)fd; (void)mode; return 0; }
#define fchmod(fd, mode) sk_fchmod_noop(fd, mode)

/* gethostname is in winsock2 */
#include <winsock2.h>

#endif /* _WIN32 */

#endif /* SK_COMPAT_H */
