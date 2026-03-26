# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

project(
  'shellkeep',
  'c',
  version: '0.1.0',
  license: 'GPL-3.0-or-later',
  meson_version: '>= 0.59.0',
  default_options: [
    'c_std=c11',
    'warning_level=3',
    'werror=true',
    'b_ndebug=if-release',
  ],
)

# ------------------------------------------------------------------ #
# Compiler flags                                                      #
# ------------------------------------------------------------------ #

cc = meson.get_compiler('c')

add_project_arguments(
  cc.get_supported_arguments(
    '-Wformat=2',
    '-Wformat-security',
    '-Wnull-dereference',
    '-Wstack-protector',
    '-Wstrict-prototypes',
    '-Wmissing-prototypes',
    '-Wshadow',
    '-Wpointer-arith',
    '-Wcast-align',
    '-Wwrite-strings',
    '-Wimplicit-fallthrough',
    '-fstack-protector-strong',
    '-D_DEFAULT_SOURCE',
  ),
  language: 'c',
)

# Sanitizers in debug builds
if get_option('buildtype') == 'debug'
  add_project_arguments(
    cc.get_supported_arguments(
      '-fsanitize=address',
      '-fsanitize=undefined',
      '-fno-omit-frame-pointer',
    ),
    language: 'c',
  )
  add_project_link_arguments(
    cc.get_supported_link_arguments(
      '-fsanitize=address',
      '-fsanitize=undefined',
    ),
    language: 'c',
  )
endif

# ------------------------------------------------------------------ #
# Dependencies                                                        #
# ------------------------------------------------------------------ #

gtk3_dep       = dependency('gtk+-3.0', version: '>= 3.24.0')
vte_dep        = dependency('vte-2.91', version: '>= 0.62.0')
libssh_dep     = dependency('libssh', version: '>= 0.9.6')
glib_dep       = dependency('glib-2.0', version: '>= 2.66.0')
gio_dep        = dependency('gio-2.0', version: '>= 2.66.0')
json_glib_dep  = dependency('json-glib-1.0', version: '>= 1.6.0')
appindicator_dep = dependency(
  'ayatana-appindicator3-0.1',
  required: false,
)

# Optional: journald / systemd logging (NFR-OBS-14)
journald_opt = get_option('journald')
libsystemd_dep = dependency('libsystemd', required: journald_opt)
if libsystemd_dep.found()
  add_project_arguments('-DHAVE_JOURNALD=1', language: 'c')
endif

# Optional: cmocka for unit tests (NFR-BUILD-02)
if get_option('tests')
  cmocka_dep = dependency('cmocka', required: true)
endif

# ------------------------------------------------------------------ #
# Include directories                                                 #
# ------------------------------------------------------------------ #

shellkeep_inc = include_directories('include')

# ------------------------------------------------------------------ #
# i18n                                                                #
# ------------------------------------------------------------------ #

i18n = import('i18n')

# NFR-I18N-06: gettext infrastructure
add_project_arguments(
  '-DGETTEXT_PACKAGE="shellkeep"',
  language: 'c',
)

# NFR-I18N-06: LOCALEDIR for bindtextdomain
localedir = get_option('prefix') / get_option('localedir')
add_project_arguments(
  '-DLOCALEDIR="' + localedir + '"',
  language: 'c',
)

subdir('po')

# ------------------------------------------------------------------ #
# Sources                                                             #
# ------------------------------------------------------------------ #

shellkeep_sources = files(
  'src/main.c',
  'src/sk_types.c',
  'src/log/sk_log_stub.c',
  'src/config/sk_config.c',
  'src/config/sk_config_client_id.c',
  'src/config/sk_config_hotreload.c',
  'src/config/sk_config_theme.c',
  'src/state/sk_state.c',
  'src/state/sk_history.c',
  'src/state/sk_paths.c',
  'src/state/sk_permissions.c',
  'src/state/sk_recent.c',
  'src/state/sk_state_debounce.c',
)

# Layer sources will be added by each layer's meson.build as they
# are implemented.  The log stub provides minimal logging until the
# full logging layer replaces it.

# ------------------------------------------------------------------ #
# Executable                                                          #
# ------------------------------------------------------------------ #

shellkeep_deps = [
  gtk3_dep,
  vte_dep,
  libssh_dep,
  glib_dep,
  gio_dep,
  json_glib_dep,
]

if appindicator_dep.found()
  shellkeep_deps += appindicator_dep
  add_project_arguments('-DHAVE_APPINDICATOR=1', language: 'c')
endif

if libsystemd_dep.found()
  shellkeep_deps += libsystemd_dep
endif

shellkeep_exe = executable(
  'shellkeep',
  shellkeep_sources,
  include_directories: shellkeep_inc,
  dependencies: shellkeep_deps,
  install: true,
)

# ------------------------------------------------------------------ #
# Tests                                                               #
# ------------------------------------------------------------------ #

if get_option('tests')
  # ---- Shared sources for test linking ----

  # State layer sources (for test_state, test_history).
  state_test_sources = files(
    'src/state/sk_state.c',
    'src/state/sk_history.c',
    'src/state/sk_paths.c',
    'src/state/sk_permissions.c',
    'src/state/sk_recent.c',
    'src/state/sk_state_debounce.c',
    'src/log/sk_log_stub.c',
    'src/sk_types.c',
  )

  # Config layer sources (for test_config).
  config_test_sources = files(
    'src/config/sk_config.c',
    'src/config/sk_config_client_id.c',
    'src/config/sk_config_hotreload.c',
    'src/config/sk_config_theme.c',
    'src/log/sk_log_stub.c',
    'src/sk_types.c',
  )

  # Session layer sources (for test_session).
  session_test_sources = files(
    'src/session/sk_session_ops.c',
    'src/session/sk_session_manager.c',
    'src/session/sk_session_reconcile.c',
    'src/session/sk_session_history.c',
    'src/session/sk_tmux_detect.c',
    'src/session/sk_tmux_control.c',
    'src/session/sk_lock.c',
    'src/ssh/sk_ssh_connection.c',
    'src/ssh/sk_ssh_auth.c',
    'src/ssh/sk_ssh_channel.c',
    'src/ssh/sk_ssh_hostkey.c',
    'src/ssh/sk_ssh_crypto.c',
    'src/ssh/sk_ssh_keepalive.c',
    'src/ssh/sk_ssh_sftp.c',
    'src/ssh/sk_ssh_shell_fallback.c',
    'src/config/sk_config.c',
    'src/config/sk_config_client_id.c',
    'src/config/sk_config_hotreload.c',
    'src/config/sk_config_theme.c',
    'src/log/sk_log_stub.c',
    'src/sk_types.c',
  )

  # Reconnect layer sources (for test_reconnect).
  reconnect_test_sources = files(
    'src/ssh/reconnect.c',
    'src/ssh/sk_ssh_connection.c',
    'src/ssh/sk_ssh_auth.c',
    'src/ssh/sk_ssh_channel.c',
    'src/ssh/sk_ssh_hostkey.c',
    'src/ssh/sk_ssh_crypto.c',
    'src/ssh/sk_ssh_keepalive.c',
    'src/ssh/sk_ssh_sftp.c',
    'src/ssh/sk_ssh_shell_fallback.c',
    'src/config/sk_config.c',
    'src/config/sk_config_client_id.c',
    'src/config/sk_config_hotreload.c',
    'src/config/sk_config_theme.c',
    'src/log/sk_log_stub.c',
    'src/sk_types.c',
  )

  # Common test include path (for test_helpers.h).
  test_inc = include_directories('tests/unit')

  # ---- Unit tests (NFR-BUILD-03..05) ----

  test_state_exe = executable(
    'test_state',
    'tests/unit/test_state.c',
    state_test_sources,
    include_directories: [shellkeep_inc, test_inc],
    dependencies: [glib_dep, gio_dep, json_glib_dep, cmocka_dep],
  )
  test('unit-state', test_state_exe, suite: 'unit')

  test_config_exe = executable(
    'test_config',
    'tests/unit/test_config.c',
    config_test_sources,
    include_directories: [shellkeep_inc, test_inc],
    dependencies: [glib_dep, gio_dep, json_glib_dep, cmocka_dep],
  )
  test('unit-config', test_config_exe, suite: 'unit')

  test_ssh_exe = executable(
    'test_ssh',
    'tests/unit/test_ssh.c',
    files('src/ssh/sk_ssh_connection.c', 'src/ssh/sk_ssh_auth.c',
          'src/ssh/sk_ssh_channel.c', 'src/ssh/sk_ssh_hostkey.c',
          'src/ssh/sk_ssh_crypto.c', 'src/ssh/sk_ssh_keepalive.c',
          'src/ssh/sk_ssh_sftp.c', 'src/ssh/sk_ssh_shell_fallback.c',
          'src/config/sk_config_client_id.c',
          'src/config/sk_config.c', 'src/config/sk_config_hotreload.c',
          'src/config/sk_config_theme.c',
          'src/log/sk_log_stub.c', 'src/sk_types.c'),
    include_directories: [shellkeep_inc, test_inc,
                          include_directories('src/ssh')],
    dependencies: [glib_dep, gio_dep, json_glib_dep, libssh_dep,
                   cmocka_dep],
  )
  test('unit-ssh', test_ssh_exe, suite: 'unit')

  test_session_exe = executable(
    'test_session',
    'tests/unit/test_session.c',
    session_test_sources,
    include_directories: [shellkeep_inc, test_inc,
                          include_directories('src/session'),
                          include_directories('src/ssh')],
    dependencies: [glib_dep, gio_dep, json_glib_dep, libssh_dep,
                   cmocka_dep],
  )
  test('unit-session', test_session_exe, suite: 'unit')

  test_log_exe = executable(
    'test_log',
    'tests/unit/test_log.c',
    files('src/log/sk_log_stub.c', 'src/sk_types.c'),
    include_directories: [shellkeep_inc, test_inc],
    dependencies: [glib_dep, cmocka_dep],
  )
  test('unit-log', test_log_exe, suite: 'unit')

  test_history_exe = executable(
    'test_history',
    'tests/unit/test_history.c',
    state_test_sources,
    include_directories: [shellkeep_inc, test_inc],
    dependencies: [glib_dep, gio_dep, json_glib_dep, cmocka_dep],
  )
  test('unit-history', test_history_exe, suite: 'unit')

  math_dep = cc.find_library('m', required: false)

  test_reconnect_exe = executable(
    'test_reconnect',
    'tests/unit/test_reconnect.c',
    reconnect_test_sources,
    include_directories: [shellkeep_inc, test_inc,
                          include_directories('src/ssh')],
    dependencies: [glib_dep, gio_dep, json_glib_dep, libssh_dep,
                   cmocka_dep, math_dep],
  )
  test('unit-reconnect', test_reconnect_exe, suite: 'unit')

  # ---- New unit tests (coverage gaps) ----

  test_recent_exe = executable(
    'test_recent',
    'tests/unit/test_recent.c',
    state_test_sources,
    include_directories: [shellkeep_inc, test_inc],
    dependencies: [glib_dep, gio_dep, json_glib_dep, cmocka_dep],
  )
  test('unit-recent', test_recent_exe, suite: 'unit')

  test_paths_exe = executable(
    'test_paths',
    'tests/unit/test_paths.c',
    state_test_sources,
    include_directories: [shellkeep_inc, test_inc],
    dependencies: [glib_dep, gio_dep, json_glib_dep, cmocka_dep],
  )
  test('unit-paths', test_paths_exe, suite: 'unit')

  test_permissions_exe = executable(
    'test_permissions',
    'tests/unit/test_permissions.c',
    state_test_sources,
    include_directories: [shellkeep_inc, test_inc],
    dependencies: [glib_dep, gio_dep, json_glib_dep, cmocka_dep],
  )
  test('unit-permissions', test_permissions_exe, suite: 'unit')

  test_theme_exe = executable(
    'test_theme',
    'tests/unit/test_theme.c',
    config_test_sources,
    include_directories: [shellkeep_inc, test_inc],
    dependencies: [glib_dep, gio_dep, json_glib_dep, cmocka_dep],
  )
  test('unit-theme', test_theme_exe, suite: 'unit')

  test_control_parse_exe = executable(
    'test_control_parse',
    'tests/unit/test_control_parse.c',
    session_test_sources,
    include_directories: [shellkeep_inc, test_inc,
                          include_directories('src/session'),
                          include_directories('src/ssh')],
    dependencies: [glib_dep, gio_dep, json_glib_dep, libssh_dep,
                   cmocka_dep],
  )
  test('unit-control-parse', test_control_parse_exe, suite: 'unit')

  test_lock_exe = executable(
    'test_lock',
    'tests/unit/test_lock.c',
    session_test_sources,
    include_directories: [shellkeep_inc, test_inc,
                          include_directories('src/session'),
                          include_directories('src/ssh')],
    dependencies: [glib_dep, gio_dep, json_glib_dep, libssh_dep,
                   cmocka_dep],
  )
  test('unit-lock', test_lock_exe, suite: 'unit')

  test_debounce_exe = executable(
    'test_debounce',
    'tests/unit/test_debounce.c',
    state_test_sources,
    include_directories: [shellkeep_inc, test_inc],
    dependencies: [glib_dep, gio_dep, json_glib_dep, cmocka_dep],
  )
  test('unit-debounce', test_debounce_exe, suite: 'unit')

  test_types_exe = executable(
    'test_types',
    'tests/unit/test_types.c',
    state_test_sources,
    include_directories: [shellkeep_inc, test_inc],
    dependencies: [glib_dep, gio_dep, json_glib_dep, cmocka_dep],
  )
  test('unit-types', test_types_exe, suite: 'unit')

  # ---- Integration tests (require running sshd container) ----

  integration_common_sources = files(
    'src/ssh/sk_ssh_connection.c',
    'src/ssh/sk_ssh_auth.c',
    'src/ssh/sk_ssh_channel.c',
    'src/ssh/sk_ssh_hostkey.c',
    'src/ssh/sk_ssh_crypto.c',
    'src/ssh/sk_ssh_keepalive.c',
    'src/ssh/sk_ssh_sftp.c',
    'src/ssh/sk_ssh_shell_fallback.c',
    'src/session/sk_session_ops.c',
    'src/session/sk_session_manager.c',
    'src/session/sk_session_reconcile.c',
    'src/session/sk_session_history.c',
    'src/session/sk_tmux_detect.c',
    'src/session/sk_tmux_control.c',
    'src/session/sk_lock.c',
    'src/state/sk_state.c',
    'src/state/sk_history.c',
    'src/state/sk_paths.c',
    'src/state/sk_permissions.c',
    'src/state/sk_recent.c',
    'src/state/sk_state_debounce.c',
    'src/log/sk_log_stub.c',
    'src/sk_types.c',
  )

  integration_deps = [
    glib_dep, gio_dep, json_glib_dep, libssh_dep, cmocka_dep,
  ]

  integration_inc = [
    shellkeep_inc, test_inc,
    include_directories('src/ssh'),
    include_directories('src/session'),
  ]

  test_integ_connect = executable(
    'test_integration_connect',
    'tests/integration/test_integration_connect.c',
    integration_common_sources,
    include_directories: integration_inc,
    dependencies: integration_deps,
  )
  integration_env = ['ASAN_OPTIONS=detect_leaks=0:halt_on_error=1:abort_on_error=1:print_summary=1']
  test('integration-connect', test_integ_connect,
       suite: 'integration', is_parallel: false, env: integration_env)

  test_integ_lock = executable(
    'test_integration_lock',
    'tests/integration/test_integration_lock.c',
    integration_common_sources,
    include_directories: integration_inc,
    dependencies: integration_deps,
  )
  test('integration-lock', test_integ_lock,
       suite: 'integration', is_parallel: false, env: integration_env)

  test_integ_state = executable(
    'test_integration_state',
    'tests/integration/test_integration_state.c',
    integration_common_sources,
    include_directories: integration_inc,
    dependencies: integration_deps,
  )
  test('integration-state', test_integ_state,
       suite: 'integration', is_parallel: false, env: integration_env)

  # ---- Edge-case tests (Agent 19) ----

  edge_test_inc = include_directories('tests/unit')

  # Edge: corrupted state files
  test_edge_state_exe = executable(
    'test_edge_state',
    'tests/edge/test_edge_state.c',
    state_test_sources,
    include_directories: [shellkeep_inc, edge_test_inc],
    dependencies: [glib_dep, gio_dep, json_glib_dep, cmocka_dep],
  )
  test('edge-state', test_edge_state_exe, suite: 'edge')

  # Edge: boundary limits and Unicode
  # Needs state layer + sk_config_validate_client_id from config layer.
  test_edge_limits_exe = executable(
    'test_edge_limits',
    'tests/edge/test_edge_limits.c',
    state_test_sources,
    files('src/config/sk_config.c', 'src/config/sk_config_client_id.c',
          'src/config/sk_config_hotreload.c', 'src/config/sk_config_theme.c'),
    include_directories: [shellkeep_inc, edge_test_inc],
    dependencies: [glib_dep, gio_dep, json_glib_dep, cmocka_dep],
  )
  test('edge-limits', test_edge_limits_exe, suite: 'edge')

  # Edge: tmux session handling
  test_edge_tmux_exe = executable(
    'test_edge_tmux',
    'tests/edge/test_edge_tmux.c',
    session_test_sources,
    include_directories: [shellkeep_inc, edge_test_inc,
                          include_directories('src/session'),
                          include_directories('src/ssh')],
    dependencies: [glib_dep, gio_dep, json_glib_dep, libssh_dep,
                   cmocka_dep],
  )
  test('edge-tmux', test_edge_tmux_exe, suite: 'edge')

  # Edge: SSH layer
  test_edge_ssh_exe = executable(
    'test_edge_ssh',
    'tests/edge/test_edge_ssh.c',
    files('src/ssh/sk_ssh_connection.c', 'src/ssh/sk_ssh_auth.c',
          'src/ssh/sk_ssh_channel.c', 'src/ssh/sk_ssh_hostkey.c',
          'src/ssh/sk_ssh_crypto.c', 'src/ssh/sk_ssh_keepalive.c',
          'src/ssh/sk_ssh_sftp.c', 'src/ssh/sk_ssh_shell_fallback.c',
          'src/config/sk_config_client_id.c',
          'src/config/sk_config.c', 'src/config/sk_config_hotreload.c',
          'src/config/sk_config_theme.c',
          'src/log/sk_log_stub.c', 'src/sk_types.c'),
    include_directories: [shellkeep_inc, edge_test_inc,
                          include_directories('src/ssh')],
    dependencies: [glib_dep, gio_dep, json_glib_dep, libssh_dep,
                   cmocka_dep],
  )
  test('edge-ssh', test_edge_ssh_exe, suite: 'edge')

  # Edge: signal handling and atomic writes
  test_edge_signals_exe = executable(
    'test_edge_signals',
    'tests/edge/test_edge_signals.c',
    state_test_sources,
    include_directories: [shellkeep_inc, edge_test_inc],
    dependencies: [glib_dep, gio_dep, json_glib_dep, cmocka_dep],
  )
  test('edge-signals', test_edge_signals_exe, suite: 'edge')

  # ---- Upgrade-path tests (Agent 29 — FR-STATE-08) ----

  test_upgrade_exe = executable(
    'test_upgrade',
    'tests/upgrade/test_upgrade.c',
    state_test_sources,
    include_directories: [shellkeep_inc, test_inc],
    dependencies: [glib_dep, gio_dep, json_glib_dep, cmocka_dep],
  )
  test('upgrade-path', test_upgrade_exe, suite: 'upgrade',
       env: ['MESON_SOURCE_ROOT=' + meson.project_source_root()])
endif

# ------------------------------------------------------------------ #
# Fuzz targets (Agent 26)                                            #
# ------------------------------------------------------------------ #

if get_option('fuzz')
  subdir('tests/fuzz')
endif

# ------------------------------------------------------------------ #
# Install targets                                                     #
# ------------------------------------------------------------------ #

# Desktop file
install_data(
  'data/shellkeep.desktop',
  install_dir: get_option('datadir') / 'applications',
  install_tag: 'runtime',
)

# Man page
install_man('man/shellkeep.1')

# Icons — SVG (scalable) + PNG at standard sizes (NFR-DIST-04)
install_data(
  'data/icons/shellkeep.svg',
  install_dir: get_option('datadir') / 'icons' / 'hicolor' / 'scalable' / 'apps',
  install_tag: 'runtime',
)

install_data(
  'data/icons/shellkeep-48.png',
  rename: 'shellkeep.png',
  install_dir: get_option('datadir') / 'icons' / 'hicolor' / '48x48' / 'apps',
  install_tag: 'runtime',
)

install_data(
  'data/icons/shellkeep-128.png',
  rename: 'shellkeep.png',
  install_dir: get_option('datadir') / 'icons' / 'hicolor' / '128x128' / 'apps',
  install_tag: 'runtime',
)

install_data(
  'data/icons/shellkeep-256.png',
  rename: 'shellkeep.png',
  install_dir: get_option('datadir') / 'icons' / 'hicolor' / '256x256' / 'apps',
  install_tag: 'runtime',
)

# Autostart desktop template (NFR-DIST-08)
install_data(
  'data/shellkeep-autostart.desktop',
  install_dir: get_option('datadir') / 'shellkeep',
  install_tag: 'runtime',
)

# AppStream metadata (NFR-DIST-01)
install_data(
  'data/org.shellkeep.ShellKeep.appdata.xml',
  install_dir: get_option('datadir') / 'metainfo',
  install_tag: 'runtime',
)
