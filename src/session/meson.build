# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Session layer — tmux interaction, lock mechanism, history, reconciliation.

session_sources = files(
    'sk_session_manager.c',
    'sk_tmux_detect.c',
    'sk_tmux_control.c',
    'sk_session_ops.c',
    'sk_lock.c',
    'sk_session_history.c',
    'sk_session_reconcile.c',
)

session_deps = [
    dependency('glib-2.0', version: '>= 2.66'),
    dependency('gio-2.0'),
    dependency('libssh', version: '>= 0.10.0'),
]

session_inc = include_directories('../../include')

session_lib = static_library(
    'sk_session',
    session_sources,
    dependencies: session_deps,
    include_directories: session_inc,
    c_args: ['-std=c11', '-Wall', '-Wextra', '-Werror'],
    install: false,
)

session_dep = declare_dependency(
    link_with: session_lib,
    dependencies: session_deps,
    include_directories: session_inc,
)
