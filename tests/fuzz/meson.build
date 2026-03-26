# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Fuzz targets for shellkeep parsers.
#
# These targets use libFuzzer (built-in to clang). Build with:
#   CC=clang meson setup build-fuzz -Dfuzz=true -Db_sanitize=address,undefined
#   ninja -C build-fuzz
#
# Run a target:
#   ./build-fuzz/tests/fuzz/fuzz_state_load tests/fuzz/corpus/state/ -max_total_time=600

# --- State layer sources needed for fuzz targets ---
fuzz_state_sources = files(
  '../../src/state/sk_state.c',
  '../../src/state/sk_history.c',
  '../../src/state/sk_paths.c',
  '../../src/state/sk_permissions.c',
  '../../src/state/sk_recent.c',
  '../../src/state/sk_state_debounce.c',
  '../../src/log/sk_log_stub.c',
  '../../src/sk_types.c',
)

fuzz_config_sources = files(
  '../../src/config/sk_config.c',
  '../../src/config/sk_config_client_id.c',
  '../../src/config/sk_config_hotreload.c',
  '../../src/config/sk_config_theme.c',
  '../../src/log/sk_log_stub.c',
  '../../src/sk_types.c',
)

fuzz_session_sources = files(
  '../../src/session/sk_session_ops.c',
  '../../src/session/sk_session_manager.c',
  '../../src/session/sk_session_reconcile.c',
  '../../src/session/sk_session_history.c',
  '../../src/session/sk_tmux_detect.c',
  '../../src/session/sk_tmux_control.c',
  '../../src/session/sk_lock.c',
  '../../src/ssh/sk_ssh_connection.c',
  '../../src/ssh/sk_ssh_auth.c',
  '../../src/ssh/sk_ssh_channel.c',
  '../../src/ssh/sk_ssh_hostkey.c',
  '../../src/ssh/sk_ssh_crypto.c',
  '../../src/ssh/sk_ssh_keepalive.c',
  '../../src/ssh/sk_ssh_sftp.c',
  '../../src/ssh/sk_ssh_shell_fallback.c',
  '../../src/config/sk_config.c',
  '../../src/config/sk_config_client_id.c',
  '../../src/config/sk_config_hotreload.c',
  '../../src/config/sk_config_theme.c',
  '../../src/log/sk_log_stub.c',
  '../../src/sk_types.c',
)

fuzz_inc = [
  shellkeep_inc,
  include_directories('../../src/session'),
  include_directories('../../src/ssh'),
]

# libFuzzer link arg — clang provides this with -fsanitize=fuzzer
fuzzer_link_args = ['-fsanitize=fuzzer']

# --- fuzz_state_load ---
executable(
  'fuzz_state_load',
  'fuzz_state_load.c',
  fuzz_state_sources,
  include_directories: [shellkeep_inc],
  dependencies: [glib_dep, gio_dep, json_glib_dep],
  link_args: fuzzer_link_args,
)

# --- fuzz_config_load ---
executable(
  'fuzz_config_load',
  'fuzz_config_load.c',
  fuzz_config_sources,
  include_directories: [shellkeep_inc],
  dependencies: [glib_dep, gio_dep, json_glib_dep],
  link_args: fuzzer_link_args,
)

# --- fuzz_history_read ---
executable(
  'fuzz_history_read',
  'fuzz_history_read.c',
  fuzz_state_sources,
  include_directories: [shellkeep_inc],
  dependencies: [glib_dep, gio_dep, json_glib_dep],
  link_args: fuzzer_link_args,
)

# --- fuzz_tmux_control_parse ---
executable(
  'fuzz_tmux_control_parse',
  'fuzz_tmux_control_parse.c',
  fuzz_session_sources,
  include_directories: fuzz_inc,
  dependencies: [glib_dep, gio_dep, json_glib_dep, libssh_dep],
  link_args: fuzzer_link_args,
)

# --- fuzz_ssh_config_parse ---
executable(
  'fuzz_ssh_config_parse',
  'fuzz_ssh_config_parse.c',
  fuzz_config_sources,
  fuzz_state_sources,
  include_directories: [shellkeep_inc],
  dependencies: [glib_dep, gio_dep, json_glib_dep],
  link_args: fuzzer_link_args,
)
