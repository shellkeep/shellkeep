// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_session_reconcile.c
 * @brief Session reconciliation: compare state UUIDs vs live tmux sessions.
 *
 * Implements FR-SESSION-07..08:
 * - UUID is the primary stable identifier for sessions.
 * - On restore, compare UUIDs from saved state against live tmux sessions.
 * - Detect dead sessions (UUID in state but no matching live session).
 * - Detect renamed sessions (UUID matches but tmux name diverged).
 * - Detect orphaned sessions (live sessions not in saved state).
 */

#include "sk_session_internal.h"
#include <string.h>

/* ------------------------------------------------------------------ */
/* Reconciliation (FR-SESSION-07, FR-SESSION-08)                       */
/* ------------------------------------------------------------------ */

SkReconcileResult *
sk_session_reconcile(SkSessionManager *mgr, GPtrArray *state_sessions, const char *client_id,
                     const char *environment, GError **error)
{
  g_return_val_if_fail(mgr != NULL, NULL);

  /* Fetch live sessions from the server. */
  GPtrArray *live = sk_session_list(mgr, client_id, environment, error);
  if (live == NULL)
  {
    return NULL;
  }

  SkReconcileResult *result = g_new0(SkReconcileResult, 1);
  result->alive = g_ptr_array_new_with_free_func((GDestroyNotify)sk_session_info_free);
  result->dead = g_ptr_array_new_with_free_func(g_free);
  result->orphaned = g_ptr_array_new_with_free_func((GDestroyNotify)sk_session_info_free);
  result->renamed = g_ptr_array_new_with_free_func((GDestroyNotify)sk_session_info_free);

  /* Build a hash table of live sessions by UUID for quick lookup. */
  GHashTable *live_by_uuid = g_hash_table_new(g_str_hash, g_str_equal);
  /* Track which live sessions are matched to state. */
  GHashTable *matched_live = g_hash_table_new(g_direct_hash, g_direct_equal);

  for (guint i = 0; i < live->len; i++)
  {
    SkSessionInfo *info = g_ptr_array_index(live, i);
    if (info->session_uuid != NULL && info->session_uuid[0] != '\0')
    {
      g_hash_table_insert(live_by_uuid, info->session_uuid, info);
    }
  }

  /* Compare each state session against live sessions. */
  if (state_sessions != NULL)
  {
    for (guint i = 0; i < state_sessions->len; i++)
    {
      SkSessionInfo *state_info = g_ptr_array_index(state_sessions, i);

      if (state_info->session_uuid == NULL || state_info->session_uuid[0] == '\0')
      {
        /* No UUID — treat as dead. */
        g_ptr_array_add(result->dead, g_strdup("(no-uuid)"));
        continue;
      }

      /* FR-SESSION-07: look up by UUID. */
      SkSessionInfo *live_info = g_hash_table_lookup(live_by_uuid, state_info->session_uuid);

      if (live_info == NULL)
      {
        /* Session UUID exists in state but not live — dead. */
        g_ptr_array_add(result->dead, g_strdup(state_info->session_uuid));
      }
      else
      {
        /* Mark this live session as matched. */
        g_hash_table_insert(matched_live, live_info, live_info);

        /* FR-SESSION-08: check if tmux name has diverged. */
        if (state_info->name != NULL && live_info->name != NULL &&
            strcmp(state_info->name, live_info->name) != 0)
        {
          /* Name diverged — copy the live info into renamed. */
          SkSessionInfo *renamed = g_new0(SkSessionInfo, 1);
          renamed->name = g_strdup(live_info->name);
          renamed->session_uuid = g_strdup(live_info->session_uuid);
          renamed->client_id = g_strdup(live_info->client_id);
          renamed->environment = g_strdup(live_info->environment);
          renamed->session_name = g_strdup(live_info->session_name);
          renamed->num_windows = live_info->num_windows;
          renamed->attached = live_info->attached;
          g_ptr_array_add(result->renamed, renamed);
        }

        /* Copy live info into alive list. */
        SkSessionInfo *alive = g_new0(SkSessionInfo, 1);
        alive->name = g_strdup(live_info->name);
        alive->session_uuid = g_strdup(live_info->session_uuid);
        alive->client_id = g_strdup(live_info->client_id);
        alive->environment = g_strdup(live_info->environment);
        alive->session_name = g_strdup(live_info->session_name);
        alive->num_windows = live_info->num_windows;
        alive->attached = live_info->attached;
        g_ptr_array_add(result->alive, alive);
      }
    }
  }

  /* Find orphaned live sessions (not matched to any state entry). */
  for (guint i = 0; i < live->len; i++)
  {
    SkSessionInfo *info = g_ptr_array_index(live, i);
    if (!g_hash_table_contains(matched_live, info))
    {
      /* Skip lock sessions — they are not user sessions. */
      if (g_str_has_prefix(info->name, SK_LOCK_SESSION_PREFIX))
      {
        continue;
      }

      SkSessionInfo *orphan = g_new0(SkSessionInfo, 1);
      orphan->name = g_strdup(info->name);
      orphan->session_uuid = g_strdup(info->session_uuid);
      orphan->client_id = g_strdup(info->client_id);
      orphan->environment = g_strdup(info->environment);
      orphan->session_name = g_strdup(info->session_name);
      orphan->num_windows = info->num_windows;
      orphan->attached = info->attached;
      g_ptr_array_add(result->orphaned, orphan);
    }
  }

  g_hash_table_unref(live_by_uuid);
  g_hash_table_unref(matched_live);
  g_ptr_array_unref(live);

  return result;
}

/* ------------------------------------------------------------------ */
/* Cleanup                                                             */
/* ------------------------------------------------------------------ */

void
sk_reconcile_result_free(SkReconcileResult *result)
{
  if (result == NULL)
    return;

  if (result->alive != NULL)
  {
    g_ptr_array_unref(result->alive);
  }
  if (result->dead != NULL)
  {
    g_ptr_array_unref(result->dead);
  }
  if (result->orphaned != NULL)
  {
    g_ptr_array_unref(result->orphaned);
  }
  if (result->renamed != NULL)
  {
    g_ptr_array_unref(result->renamed);
  }

  g_free(result);
}
