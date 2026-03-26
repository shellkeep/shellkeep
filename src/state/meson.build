# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# State layer — persistence, XDG paths, history, recent connections.

state_sources = files(
  'sk_state.c',
  'sk_state_debounce.c',
  'sk_paths.c',
  'sk_permissions.c',
  'sk_recent.c',
  'sk_history.c',
)

state_deps = [
  dependency('glib-2.0', version: '>= 2.66'),
  dependency('gio-2.0'),
  dependency('json-glib-1.0', version: '>= 1.6'),
]

state_inc = include_directories('../../include')

state_lib = static_library(
  'sk_state',
  state_sources,
  dependencies: state_deps,
  include_directories: state_inc,
  c_args: ['-std=c11', '-Wall', '-Wextra', '-Werror'],
  install: false,
)

state_dep = declare_dependency(
  link_with: state_lib,
  dependencies: state_deps,
  include_directories: state_inc,
)
