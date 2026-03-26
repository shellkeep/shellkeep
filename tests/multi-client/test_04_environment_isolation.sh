#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Multi-Client Test 4: Environment isolation
#
# Verifies that each client-id has independent environments:
#   - "desktop" creates environment "Project" with sessions
#   - "laptop" connects -> has its own environments, does not see "desktop"'s
#   - tmux list-sessions shows ALL sessions from both clients on the server
#   - Environment names are scoped by client-id in session naming
#   - Each client can have same-named environments without conflict
#   - Sessions in one client's environment are invisible to the other
#
# Requirements tested:
#   FR-ENV-01, FR-ENV-02, FR-ENV-05, FR-SESSION-04, FR-STATE-01

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=mc_helpers.sh
source "$SCRIPT_DIR/mc_helpers.sh"

CLIENT_DESKTOP="desktop"
CLIENT_LAPTOP="laptop"

# ---- Setup ---------------------------------------------------------------- #

e2e_check_prereqs
e2e_build_image
mc_register_cleanup
mc_start_container "enviso"

e2e_section "Test 4: Environment isolation"

# ---- Desktop creates "Project" environment -------------------------------- #

e2e_section "4.1 Desktop creates 'Project' environment (FR-ENV-01)"

e2e_lock_acquire "$CLIENT_DESKTOP" "desktop-workstation"

ENV_PROJECT="Project"

# Desktop creates sessions in "Project" environment.
DESK_P1="${CLIENT_DESKTOP}--${ENV_PROJECT}--backend"
DESK_P2="${CLIENT_DESKTOP}--${ENV_PROJECT}--frontend"
DESK_P3="${CLIENT_DESKTOP}--${ENV_PROJECT}--database"

DESK_P1_UUID="d1000001-0001-4001-8001-000000000001"
DESK_P2_UUID="d1000002-0002-4002-8002-000000000002"
DESK_P3_UUID="d1000003-0003-4003-8003-000000000003"

for sname in "$DESK_P1" "$DESK_P2" "$DESK_P3"; do
  e2e_tmux_create "$sname"
done
e2e_tmux_set_env "$DESK_P1" "SHELLKEEP_SESSION_UUID" "$DESK_P1_UUID"
e2e_tmux_set_env "$DESK_P2" "SHELLKEEP_SESSION_UUID" "$DESK_P2_UUID"
e2e_tmux_set_env "$DESK_P3" "SHELLKEEP_SESSION_UUID" "$DESK_P3_UUID"

# Desktop also has a "Default" environment with one session.
ENV_DEFAULT="Default"
DESK_D1="${CLIENT_DESKTOP}--${ENV_DEFAULT}--misc"
DESK_D1_UUID="d2000001-0001-4001-8001-000000000001"
e2e_tmux_create "$DESK_D1"
e2e_tmux_set_env "$DESK_D1" "SHELLKEEP_SESSION_UUID" "$DESK_D1_UUID"

for sname in "$DESK_P1" "$DESK_P2" "$DESK_P3" "$DESK_D1"; do
  assert_ok "Desktop session $sname exists" e2e_tmux_has_session "$sname"
done

# Write desktop state with both environments.
DESK_STATE=$(cat <<EOF
{
  "schema_version": 1,
  "last_modified": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "client_id": "${CLIENT_DESKTOP}",
  "environments": {
    "${ENV_PROJECT}": {
      "windows": [{
        "id": "w1111111-1111-4111-8111-111111111111",
        "title": "Project Window",
        "visible": true,
        "active_tab": 0,
        "geometry": { "x": 0, "y": 0, "width": 1920, "height": 1080 },
        "tabs": [
          { "session_uuid": "${DESK_P1_UUID}", "tmux_session_name": "${DESK_P1}", "title": "backend", "position": 0 },
          { "session_uuid": "${DESK_P2_UUID}", "tmux_session_name": "${DESK_P2}", "title": "frontend", "position": 1 },
          { "session_uuid": "${DESK_P3_UUID}", "tmux_session_name": "${DESK_P3}", "title": "database", "position": 2 }
        ]
      }]
    },
    "${ENV_DEFAULT}": {
      "windows": [{
        "id": "w2222222-2222-4222-8222-222222222222",
        "title": "Default Window",
        "visible": true,
        "active_tab": 0,
        "geometry": { "x": 0, "y": 0, "width": 1920, "height": 1080 },
        "tabs": [
          { "session_uuid": "${DESK_D1_UUID}", "tmux_session_name": "${DESK_D1}", "title": "misc", "position": 0 }
        ]
      }]
    }
  },
  "last_environment": "${ENV_PROJECT}"
}
EOF
)
e2e_write_state "$CLIENT_DESKTOP" "$DESK_STATE"
assert_ok "Desktop state written" e2e_state_exists "$CLIENT_DESKTOP"

# ---- Laptop connects with its own environments --------------------------- #

e2e_section "4.2 Laptop has independent environments (FR-ENV-02)"

e2e_lock_acquire "$CLIENT_LAPTOP" "laptop-portable"

# Laptop creates sessions in its own "Default" environment.
LAP_D1="${CLIENT_LAPTOP}--${ENV_DEFAULT}--terminal"
LAP_D2="${CLIENT_LAPTOP}--${ENV_DEFAULT}--monitor"

LAP_D1_UUID="e1000001-0001-4001-8001-000000000001"
LAP_D2_UUID="e1000002-0002-4002-8002-000000000002"

e2e_tmux_create "$LAP_D1"
e2e_tmux_set_env "$LAP_D1" "SHELLKEEP_SESSION_UUID" "$LAP_D1_UUID"
e2e_tmux_create "$LAP_D2"
e2e_tmux_set_env "$LAP_D2" "SHELLKEEP_SESSION_UUID" "$LAP_D2_UUID"

assert_ok "Laptop session 1 exists" e2e_tmux_has_session "$LAP_D1"
assert_ok "Laptop session 2 exists" e2e_tmux_has_session "$LAP_D2"

# Write laptop state (only has Default environment).
LAP_STATE=$(cat <<EOF
{
  "schema_version": 1,
  "last_modified": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "client_id": "${CLIENT_LAPTOP}",
  "environments": {
    "${ENV_DEFAULT}": {
      "windows": [{
        "id": "w3333333-3333-4333-8333-333333333333",
        "title": "Laptop Window",
        "visible": true,
        "active_tab": 0,
        "geometry": { "x": 0, "y": 0, "width": 1366, "height": 768 },
        "tabs": [
          { "session_uuid": "${LAP_D1_UUID}", "tmux_session_name": "${LAP_D1}", "title": "terminal", "position": 0 },
          { "session_uuid": "${LAP_D2_UUID}", "tmux_session_name": "${LAP_D2}", "title": "monitor", "position": 1 }
        ]
      }]
    }
  },
  "last_environment": "${ENV_DEFAULT}"
}
EOF
)
e2e_write_state "$CLIENT_LAPTOP" "$LAP_STATE"
assert_ok "Laptop state written" e2e_state_exists "$CLIENT_LAPTOP"

# ---- Laptop does NOT see desktop's environments in its state -------------- #

e2e_section "4.3 Laptop state excludes desktop environments"

lap_state=$(e2e_read_state "$CLIENT_LAPTOP")
assert_not_contains "Laptop state has no 'Project' env" "$lap_state" "\"${ENV_PROJECT}\""
assert_not_contains "Laptop state has no desktop sessions" "$lap_state" "${CLIENT_DESKTOP}--"
assert_not_contains "Laptop state has no desktop UUIDs" "$lap_state" "$DESK_P1_UUID"
assert_contains "Laptop state has its own sessions" "$lap_state" "${LAP_D1_UUID}"

# ---- Desktop does NOT see laptop's sessions in its state ------------------ #

e2e_section "4.4 Desktop state excludes laptop sessions"

desk_state=$(e2e_read_state "$CLIENT_DESKTOP")
assert_not_contains "Desktop state has no laptop sessions" "$desk_state" "${CLIENT_LAPTOP}--"
assert_not_contains "Desktop state has no laptop UUIDs" "$desk_state" "$LAP_D1_UUID"
assert_contains "Desktop state has its Project sessions" "$desk_state" "$DESK_P1_UUID"

# ---- tmux list-sessions shows ALL sessions -------------------------------- #

e2e_section "4.5 Server tmux shows all sessions from both clients"

all_sessions=$(e2e_tmux_list)
total=$(echo "$all_sessions" | wc -l)

# Expected: 4 desktop + 2 laptop + 2 locks = 8.
assert_num_eq "8 total tmux sessions on server" "$total" 8

# Desktop sessions visible in global listing.
assert_contains "Server shows desktop Project backend" "$all_sessions" "$DESK_P1"
assert_contains "Server shows desktop Project frontend" "$all_sessions" "$DESK_P2"
assert_contains "Server shows desktop Project database" "$all_sessions" "$DESK_P3"
assert_contains "Server shows desktop Default misc" "$all_sessions" "$DESK_D1"

# Laptop sessions visible in global listing.
assert_contains "Server shows laptop terminal" "$all_sessions" "$LAP_D1"
assert_contains "Server shows laptop monitor" "$all_sessions" "$LAP_D2"

# Lock sessions visible.
assert_contains "Server shows desktop lock" "$all_sessions" "${E2E_LOCK_PREFIX}${CLIENT_DESKTOP}"
assert_contains "Server shows laptop lock" "$all_sessions" "${E2E_LOCK_PREFIX}${CLIENT_LAPTOP}"

# ---- Same-named environment in different clients -------------------------- #

e2e_section "4.6 Same environment name in different clients (no conflict)"

# Both clients have "Default" environment. Sessions are isolated by client-id prefix.
desktop_default=$(e2e_tmux_list | grep "^${CLIENT_DESKTOP}--${ENV_DEFAULT}--" || true)
laptop_default=$(e2e_tmux_list | grep "^${CLIENT_LAPTOP}--${ENV_DEFAULT}--" || true)

desktop_default_count=$(echo "$desktop_default" | grep -c . || echo "0")
laptop_default_count=$(echo "$laptop_default" | grep -c . || echo "0")

assert_num_eq "Desktop has 1 Default session" "$desktop_default_count" 1
assert_num_eq "Laptop has 2 Default sessions" "$laptop_default_count" 2

# No overlap.
assert_not_contains "Desktop Default has no laptop sessions" "$desktop_default" "$CLIENT_LAPTOP"
assert_not_contains "Laptop Default has no desktop sessions" "$laptop_default" "$CLIENT_DESKTOP"

# ---- Environment-scoped filtering ----------------------------------------- #

e2e_section "4.7 Client-scoped environment listing"

# Simulate what shellkeep does: list sessions matching client-id prefix,
# then extract unique environment names from the session naming pattern.
desktop_envs=$(e2e_tmux_list | grep "^${CLIENT_DESKTOP}--" | \
  grep -v "^${E2E_LOCK_PREFIX}" | \
  sed "s/^${CLIENT_DESKTOP}--//" | cut -d'-' -f1 | sort -u)

laptop_envs=$(e2e_tmux_list | grep "^${CLIENT_LAPTOP}--" | \
  grep -v "^${E2E_LOCK_PREFIX}" | \
  sed "s/^${CLIENT_LAPTOP}--//" | cut -d'-' -f1 | sort -u)

assert_contains "Desktop sees Project env" "$desktop_envs" "Project"
assert_contains "Desktop sees Default env" "$desktop_envs" "Default"

assert_contains "Laptop sees Default env" "$laptop_envs" "Default"
assert_not_contains "Laptop does not see Project env" "$laptop_envs" "Project"

# ---- Interaction isolation within environments ---------------------------- #

e2e_section "4.8 Interaction isolation across environments"

# Send commands to desktop Project sessions and laptop Default sessions.
e2e_ssh_cmd "tmux send-keys -t '${DESK_P1}' 'echo DESKTOP_PROJECT_BACKEND' Enter"
e2e_ssh_cmd "tmux send-keys -t '${LAP_D1}' 'echo LAPTOP_DEFAULT_TERMINAL' Enter"
sleep 0.5

desk_p1_out=$(e2e_ssh_cmd "tmux capture-pane -t '${DESK_P1}' -p" || true)
lap_d1_out=$(e2e_ssh_cmd "tmux capture-pane -t '${LAP_D1}' -p" || true)

assert_contains "Desktop Project has its output" "$desk_p1_out" "DESKTOP_PROJECT_BACKEND"
assert_not_contains "Desktop Project has no laptop output" "$desk_p1_out" "LAPTOP_DEFAULT_TERMINAL"

assert_contains "Laptop Default has its output" "$lap_d1_out" "LAPTOP_DEFAULT_TERMINAL"
assert_not_contains "Laptop Default has no desktop output" "$lap_d1_out" "DESKTOP_PROJECT_BACKEND"

# ---- Deleting an environment does not affect other client ----------------- #

e2e_section "4.9 Deleting desktop environment does not affect laptop"

# Desktop deletes "Default" environment (kills its sessions).
e2e_tmux_kill "$DESK_D1"
assert_fail "Desktop Default session killed" e2e_tmux_has_session "$DESK_D1"

# Laptop Default sessions still alive.
assert_ok "Laptop Default session 1 survives" e2e_tmux_has_session "$LAP_D1"
assert_ok "Laptop Default session 2 survives" e2e_tmux_has_session "$LAP_D2"

# Desktop Project sessions still alive.
assert_ok "Desktop Project backend survives" e2e_tmux_has_session "$DESK_P1"
assert_ok "Desktop Project frontend survives" e2e_tmux_has_session "$DESK_P2"
assert_ok "Desktop Project database survives" e2e_tmux_has_session "$DESK_P3"

# ---- Cleanup ------------------------------------------------------------- #

e2e_lock_release "$CLIENT_DESKTOP"
e2e_lock_release "$CLIENT_LAPTOP"

# ---- Summary ------------------------------------------------------------- #

e2e_section "Test 4 complete: environment isolation"
e2e_summary
