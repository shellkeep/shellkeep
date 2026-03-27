// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ui_bridge.c
 * @brief Global UI bridge instance storage.
 */

#include "shellkeep/sk_ui_bridge.h"

#include <stddef.h>

static const SkUiBridge *s_bridge = NULL;
static SkUiHandle s_ui_handle = NULL;

void
sk_ui_bridge_set(const SkUiBridge *bridge, SkUiHandle ui)
{
  s_bridge = bridge;
  s_ui_handle = ui;
}

const SkUiBridge *
sk_ui_bridge_get(void)
{
  return s_bridge;
}

SkUiHandle
sk_ui_bridge_get_handle(void)
{
  return s_ui_handle;
}

bool
sk_ui_bridge_is_set(void)
{
  return s_bridge != NULL;
}
