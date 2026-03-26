// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_i18n.h
 * @brief Internationalization macros for shellkeep.
 *
 * Provides _() and ngettext() wrappers for gettext.
 * All user-visible strings MUST be wrapped with _().
 * Counts MUST use ngettext() for proper plural handling.
 *
 * Requirements: NFR-I18N-01, NFR-I18N-03, NFR-I18N-06
 */

#ifndef SK_I18N_H
#define SK_I18N_H

#include <glib/gi18n.h>

/* GETTEXT_PACKAGE is defined by the build system (meson.build).
 * glib/gi18n.h uses it to set up _() and ngettext() macros.
 *
 * If not defined, fall back to "shellkeep" to avoid compile errors
 * in standalone tool invocations (e.g., xgettext). */
#ifndef GETTEXT_PACKAGE
#define GETTEXT_PACKAGE "shellkeep"
#endif

#endif /* SK_I18N_H */
