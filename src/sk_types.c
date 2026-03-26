// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_types.c
 * @brief Implementation of common type utilities.
 */

#include "shellkeep/sk_types.h"

GQuark
sk_error_quark(void)
{
  return g_quark_from_static_string("sk-error-quark");
}
