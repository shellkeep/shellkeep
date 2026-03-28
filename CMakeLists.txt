# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

cmake_minimum_required(VERSION 3.22)

project(shellkeep
  VERSION 0.2.0
  LANGUAGES C CXX
  DESCRIPTION "Persistent SSH terminal manager"
)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# ------------------------------------------------------------------
# Build options
# ------------------------------------------------------------------

option(SK_BUILD_TESTS  "Build unit/integration/edge tests"  OFF)
option(SK_BUILD_QT_UI  "Build the Qt6 UI frontend"          ON)
option(SK_JOURNALD     "Enable journald logging"             OFF)

# ------------------------------------------------------------------
# Compiler flags
# ------------------------------------------------------------------

# Prevent GLib 'signals' field from conflicting with Qt 'signals' keyword
add_compile_definitions(QT_NO_KEYWORDS)

if(MSVC)
  add_compile_options(/W4 /WX)
else()
  add_compile_options(
    -Wall -Wextra -Werror
    -Wformat=2 -Wformat-security
    -Wnull-dereference -Wstack-protector
    -Wshadow -Wpointer-arith -Wcast-align
    -Wwrite-strings -Wimplicit-fallthrough
    -fstack-protector-strong
  )
  if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    add_compile_options(-D_DEFAULT_SOURCE)
  endif()
endif()

if(CMAKE_BUILD_TYPE STREQUAL "Debug" AND NOT MSVC)
  add_compile_options(-fsanitize=address -fsanitize=undefined -fno-omit-frame-pointer)
  add_link_options(-fsanitize=address -fsanitize=undefined)
endif()

# ------------------------------------------------------------------
# Find dependencies (backend — pure C)
# ------------------------------------------------------------------

find_package(PkgConfig REQUIRED)
pkg_check_modules(GLIB REQUIRED IMPORTED_TARGET glib-2.0>=2.66.0)
pkg_check_modules(GIO  REQUIRED IMPORTED_TARGET gio-2.0>=2.66.0)
pkg_check_modules(JSON_GLIB REQUIRED IMPORTED_TARGET json-glib-1.0>=1.6.0)
pkg_check_modules(LIBSSH REQUIRED IMPORTED_TARGET libssh>=0.9.6)

# Optional: journald
if(SK_JOURNALD)
  pkg_check_modules(SYSTEMD IMPORTED_TARGET libsystemd)
  if(SYSTEMD_FOUND)
    add_compile_definitions(HAVE_JOURNALD=1)
  endif()
endif()

# ------------------------------------------------------------------
# i18n definitions
# ------------------------------------------------------------------

add_compile_definitions(
  GETTEXT_PACKAGE="shellkeep"
  LOCALEDIR="${CMAKE_INSTALL_PREFIX}/${CMAKE_INSTALL_LOCALEDIR}"
)

# ------------------------------------------------------------------
# Platform detection
# ------------------------------------------------------------------

include(CheckIncludeFile)
check_include_file("sys/prctl.h" HAVE_PRCTL)
if(HAVE_PRCTL)
  add_compile_definitions(HAVE_PRCTL=1)
endif()

check_include_file("execinfo.h" HAVE_EXECINFO)
if(HAVE_EXECINFO)
  add_compile_definitions(HAVE_EXECINFO=1)
endif()

# ------------------------------------------------------------------
# Include directories
# ------------------------------------------------------------------

set(SK_INCLUDE_DIR ${CMAKE_SOURCE_DIR}/include)

# ------------------------------------------------------------------
# Backend static library: libshellkeep_core
# ------------------------------------------------------------------

set(SK_CORE_SOURCES
  # Types
  src/sk_types.c

  # Config layer
  src/config/sk_config.c
  src/config/sk_config_client_id.c
  src/config/sk_config_hotreload.c
  src/config/sk_config_theme.c

  # State layer
  src/state/sk_state.c
  src/state/sk_history.c
  src/state/sk_paths.c
  src/state/sk_permissions.c
  src/state/sk_recent.c
  src/state/sk_state_debounce.c

  # SSH layer
  src/ssh/sk_ssh_connection.c
  src/ssh/sk_ssh_hostkey.c
  src/ssh/sk_ssh_auth.c
  src/ssh/sk_ssh_channel.c
  src/ssh/sk_ssh_sftp.c
  src/ssh/sk_ssh_shell_fallback.c
  src/ssh/sk_ssh_crypto.c
  src/ssh/sk_ssh_keepalive.c
  src/ssh/reconnect.c

  # Session layer
  src/session/sk_session_manager.c
  src/session/sk_tmux_detect.c
  src/session/sk_tmux_control.c
  src/session/sk_session_ops.c
  src/session/sk_session_history.c
  src/session/sk_session_reconcile.c
  src/session/sk_lock.c

  # Log layer
  src/log/sk_log.c
  src/log/sk_crash.c

  # Connect layer (refactored to use UI bridge)
  src/connect/sk_connect.c
  src/connect/sk_ui_bridge.c
)

add_library(shellkeep_core STATIC ${SK_CORE_SOURCES})
target_include_directories(shellkeep_core PUBLIC
  ${SK_INCLUDE_DIR}
  PRIVATE
  ${CMAKE_SOURCE_DIR}/src/ssh
  ${CMAKE_SOURCE_DIR}/src/session
)
target_link_libraries(shellkeep_core PUBLIC
  PkgConfig::GLIB
  PkgConfig::GIO
  PkgConfig::JSON_GLIB
  PkgConfig::LIBSSH
)
if(UNIX)
  target_link_libraries(shellkeep_core PUBLIC m pthread)
endif()

if(SYSTEMD_FOUND)
  target_link_libraries(shellkeep_core PUBLIC PkgConfig::SYSTEMD)
endif()

# ------------------------------------------------------------------
# Log stub library (for tests that don't need full logging)
# ------------------------------------------------------------------

add_library(shellkeep_log_stub STATIC
  src/log/sk_log_stub.c
  src/sk_types.c
)
target_include_directories(shellkeep_log_stub PUBLIC ${SK_INCLUDE_DIR})
target_link_libraries(shellkeep_log_stub PUBLIC PkgConfig::GLIB)

# ------------------------------------------------------------------
# Qt6 UI frontend (Phase 1+)
# ------------------------------------------------------------------

if(SK_BUILD_QT_UI)
  find_package(Qt6 REQUIRED COMPONENTS Core Widgets Network)
  qt_standard_project_setup()

  # QTermWidget — required for terminal emulation
  find_package(qtermwidget6 REQUIRED)

  set(SK_QT_UI_SOURCES
    src/main_qt.cpp
    src/ui_qt/SkMainWindow.cpp
    src/ui_qt/SkDialogs.cpp
    src/ui_qt/SkWelcomeWidget.cpp
    src/ui_qt/SkToast.cpp
    src/ui_qt/SkConnFeedback.cpp
    src/ui_qt/SkTrayIcon.cpp
    src/ui_qt/SkStyleSheet.cpp
    src/ui_qt/SkUiBridgeQt.cpp
    src/terminal_qt/SkTerminalWidget.cpp
    src/terminal_qt/SkTerminalSearch.cpp
    src/terminal_qt/SkTerminalDead.cpp
    src/terminal_qt/SkTerminalTheme.cpp
    src/ui_qt/SkConnectFlow.cpp
  )

  set(SK_QT_UI_HEADERS
    include/shellkeep/sk_ui_qt.h
    include/shellkeep/sk_terminal_qt.h
    src/ui_qt/SkMainWindow.h
    src/ui_qt/SkDialogs.h
    src/ui_qt/SkWelcomeWidget.h
    src/ui_qt/SkToast.h
    src/ui_qt/SkConnFeedback.h
    src/ui_qt/SkTrayIcon.h
    src/ui_qt/SkStyleSheet.h
    src/ui_qt/SkUiBridgeQt.h
    src/ui_qt/SkConnectFlow.h
    src/terminal_qt/SkTerminalWidget.h
    src/terminal_qt/SkTerminalSearch.h
    src/terminal_qt/SkTerminalDead.h
    src/terminal_qt/SkTerminalTheme.h
  )

  add_executable(shellkeep ${SK_QT_UI_SOURCES} ${SK_QT_UI_HEADERS})
  target_include_directories(shellkeep PRIVATE
    ${SK_INCLUDE_DIR}
    ${CMAKE_SOURCE_DIR}/src
  )
  target_link_libraries(shellkeep PRIVATE
    shellkeep_core
    Qt6::Core
    Qt6::Widgets
    Qt6::Network
  )

  target_link_libraries(shellkeep PRIVATE qtermwidget6)
  target_compile_definitions(shellkeep PRIVATE HAVE_QTERMWIDGET=1)

  # macOS bundle
  if(APPLE)
    set_target_properties(shellkeep PROPERTIES
      MACOSX_BUNDLE TRUE
      MACOSX_BUNDLE_GUI_IDENTIFIER org.shellkeep.ShellKeep
      MACOSX_BUNDLE_BUNDLE_VERSION ${PROJECT_VERSION}
      MACOSX_BUNDLE_SHORT_VERSION_STRING ${PROJECT_VERSION}
    )
  endif()

  install(TARGETS shellkeep
    BUNDLE DESTINATION .
    RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR}
  )
endif()

# ------------------------------------------------------------------
# Tests (CTest)
# ------------------------------------------------------------------

if(SK_BUILD_TESTS)
  enable_testing()
  pkg_check_modules(CMOCKA REQUIRED IMPORTED_TARGET cmocka)

  # Helper: state test sources
  set(STATE_TEST_SRCS
    src/state/sk_state.c
    src/state/sk_history.c
    src/state/sk_paths.c
    src/state/sk_permissions.c
    src/state/sk_recent.c
    src/state/sk_state_debounce.c
    src/log/sk_log_stub.c
    src/sk_types.c
  )

  # Helper: config test sources
  set(CONFIG_TEST_SRCS
    src/config/sk_config.c
    src/config/sk_config_client_id.c
    src/config/sk_config_hotreload.c
    src/config/sk_config_theme.c
    src/log/sk_log_stub.c
    src/sk_types.c
  )

  # Helper: session test sources
  set(SESSION_TEST_SRCS
    src/session/sk_session_ops.c
    src/session/sk_session_manager.c
    src/session/sk_session_reconcile.c
    src/session/sk_session_history.c
    src/session/sk_tmux_detect.c
    src/session/sk_tmux_control.c
    src/session/sk_lock.c
    src/ssh/sk_ssh_connection.c
    src/ssh/sk_ssh_auth.c
    src/ssh/sk_ssh_channel.c
    src/ssh/sk_ssh_hostkey.c
    src/ssh/sk_ssh_crypto.c
    src/ssh/sk_ssh_keepalive.c
    src/ssh/sk_ssh_sftp.c
    src/ssh/sk_ssh_shell_fallback.c
    ${CONFIG_TEST_SRCS}
  )

  # Helper: reconnect test sources
  set(RECONNECT_TEST_SRCS
    src/ssh/reconnect.c
    src/ssh/sk_ssh_connection.c
    src/ssh/sk_ssh_auth.c
    src/ssh/sk_ssh_channel.c
    src/ssh/sk_ssh_hostkey.c
    src/ssh/sk_ssh_crypto.c
    src/ssh/sk_ssh_keepalive.c
    src/ssh/sk_ssh_sftp.c
    src/ssh/sk_ssh_shell_fallback.c
    ${CONFIG_TEST_SRCS}
  )

  # Common test includes
  set(TEST_INCLUDE_DIR ${CMAKE_SOURCE_DIR}/tests/unit)
  set(SSH_INCLUDE_DIR ${CMAKE_SOURCE_DIR}/src/ssh)
  set(SESSION_INCLUDE_DIR ${CMAKE_SOURCE_DIR}/src/session)

  # Macro for unit tests
  macro(sk_add_test TEST_NAME TEST_SOURCE TEST_SRCS TEST_DEPS TEST_INCS SUITE)
    add_executable(${TEST_NAME} ${TEST_SOURCE} ${TEST_SRCS})
    target_include_directories(${TEST_NAME} PRIVATE
      ${SK_INCLUDE_DIR} ${TEST_INCLUDE_DIR} ${TEST_INCS})
    target_link_libraries(${TEST_NAME} PRIVATE ${TEST_DEPS} PkgConfig::CMOCKA)
    add_test(NAME ${SUITE}-${TEST_NAME} COMMAND ${TEST_NAME})
    set_tests_properties(${SUITE}-${TEST_NAME} PROPERTIES LABELS "${SUITE}")
  endmacro()

  # --- Unit tests ---
  set(STATE_DEPS "PkgConfig::GLIB;PkgConfig::GIO;PkgConfig::JSON_GLIB")
  set(SESSION_DEPS "PkgConfig::GLIB;PkgConfig::GIO;PkgConfig::JSON_GLIB;PkgConfig::LIBSSH")

  sk_add_test(test_state tests/unit/test_state.c
    "${STATE_TEST_SRCS}" "${STATE_DEPS}" "" "unit")
  sk_add_test(test_config tests/unit/test_config.c
    "${CONFIG_TEST_SRCS}" "${STATE_DEPS}" "" "unit")
  sk_add_test(test_ssh tests/unit/test_ssh.c
    "src/ssh/sk_ssh_connection.c;src/ssh/sk_ssh_auth.c;src/ssh/sk_ssh_channel.c;src/ssh/sk_ssh_hostkey.c;src/ssh/sk_ssh_crypto.c;src/ssh/sk_ssh_keepalive.c;src/ssh/sk_ssh_sftp.c;src/ssh/sk_ssh_shell_fallback.c;${CONFIG_TEST_SRCS}"
    "${SESSION_DEPS}" "${SSH_INCLUDE_DIR}" "unit")
  sk_add_test(test_session tests/unit/test_session.c
    "${SESSION_TEST_SRCS}" "${SESSION_DEPS}" "${SESSION_INCLUDE_DIR};${SSH_INCLUDE_DIR}" "unit")
  sk_add_test(test_log tests/unit/test_log.c
    "src/log/sk_log_stub.c;src/sk_types.c" "PkgConfig::GLIB" "" "unit")
  sk_add_test(test_history tests/unit/test_history.c
    "${STATE_TEST_SRCS}" "${STATE_DEPS}" "" "unit")
  sk_add_test(test_reconnect tests/unit/test_reconnect.c
    "${RECONNECT_TEST_SRCS}" "${SESSION_DEPS};m" "${SSH_INCLUDE_DIR}" "unit")
  sk_add_test(test_recent tests/unit/test_recent.c
    "${STATE_TEST_SRCS}" "${STATE_DEPS}" "" "unit")
  sk_add_test(test_paths tests/unit/test_paths.c
    "${STATE_TEST_SRCS}" "${STATE_DEPS}" "" "unit")
  sk_add_test(test_permissions tests/unit/test_permissions.c
    "${STATE_TEST_SRCS}" "${STATE_DEPS}" "" "unit")
  sk_add_test(test_theme tests/unit/test_theme.c
    "${CONFIG_TEST_SRCS}" "${STATE_DEPS}" "" "unit")
  sk_add_test(test_control_parse tests/unit/test_control_parse.c
    "${SESSION_TEST_SRCS}" "${SESSION_DEPS}" "${SESSION_INCLUDE_DIR};${SSH_INCLUDE_DIR}" "unit")
  sk_add_test(test_lock tests/unit/test_lock.c
    "${SESSION_TEST_SRCS}" "${SESSION_DEPS}" "${SESSION_INCLUDE_DIR};${SSH_INCLUDE_DIR}" "unit")
  sk_add_test(test_debounce tests/unit/test_debounce.c
    "${STATE_TEST_SRCS}" "${STATE_DEPS}" "" "unit")
  sk_add_test(test_types tests/unit/test_types.c
    "${STATE_TEST_SRCS}" "${STATE_DEPS}" "" "unit")

  # --- Integration tests ---
  set(INTEGRATION_SRCS
    src/ssh/sk_ssh_connection.c src/ssh/sk_ssh_auth.c
    src/ssh/sk_ssh_channel.c src/ssh/sk_ssh_hostkey.c
    src/ssh/sk_ssh_crypto.c src/ssh/sk_ssh_keepalive.c
    src/ssh/sk_ssh_sftp.c src/ssh/sk_ssh_shell_fallback.c
    src/session/sk_session_ops.c src/session/sk_session_manager.c
    src/session/sk_session_reconcile.c src/session/sk_session_history.c
    src/session/sk_tmux_detect.c src/session/sk_tmux_control.c
    src/session/sk_lock.c
    ${STATE_TEST_SRCS}
  )

  foreach(INTEG_TEST test_integration_connect test_integration_lock test_integration_state)
    add_executable(${INTEG_TEST} tests/integration/${INTEG_TEST}.c ${INTEGRATION_SRCS})
    target_include_directories(${INTEG_TEST} PRIVATE
      ${SK_INCLUDE_DIR} ${TEST_INCLUDE_DIR} ${SSH_INCLUDE_DIR} ${SESSION_INCLUDE_DIR})
    target_link_libraries(${INTEG_TEST} PRIVATE ${SESSION_DEPS} PkgConfig::CMOCKA)
    add_test(NAME integration-${INTEG_TEST} COMMAND ${INTEG_TEST})
    set_tests_properties(integration-${INTEG_TEST} PROPERTIES
      LABELS "integration"
      ENVIRONMENT "ASAN_OPTIONS=detect_leaks=0:halt_on_error=1:abort_on_error=1:print_summary=1"
    )
  endforeach()

  # --- Edge-case tests ---
  sk_add_test(test_edge_state tests/edge/test_edge_state.c
    "${STATE_TEST_SRCS}" "${STATE_DEPS}" "" "edge")
  sk_add_test(test_edge_limits tests/edge/test_edge_limits.c
    "${STATE_TEST_SRCS};${CONFIG_TEST_SRCS}" "${STATE_DEPS}" "" "edge")
  sk_add_test(test_edge_tmux tests/edge/test_edge_tmux.c
    "${SESSION_TEST_SRCS}" "${SESSION_DEPS}" "${SESSION_INCLUDE_DIR};${SSH_INCLUDE_DIR}" "edge")
  sk_add_test(test_edge_ssh tests/edge/test_edge_ssh.c
    "src/ssh/sk_ssh_connection.c;src/ssh/sk_ssh_auth.c;src/ssh/sk_ssh_channel.c;src/ssh/sk_ssh_hostkey.c;src/ssh/sk_ssh_crypto.c;src/ssh/sk_ssh_keepalive.c;src/ssh/sk_ssh_sftp.c;src/ssh/sk_ssh_shell_fallback.c;${CONFIG_TEST_SRCS}"
    "${SESSION_DEPS}" "${SSH_INCLUDE_DIR}" "edge")
  sk_add_test(test_edge_signals tests/edge/test_edge_signals.c
    "${STATE_TEST_SRCS}" "${STATE_DEPS}" "" "edge")

  # --- Upgrade test ---
  sk_add_test(test_upgrade tests/upgrade/test_upgrade.c
    "${STATE_TEST_SRCS}" "${STATE_DEPS}" "" "upgrade")
  set_tests_properties(upgrade-test_upgrade PROPERTIES
    ENVIRONMENT "MESON_SOURCE_ROOT=${CMAKE_SOURCE_DIR}")
endif()

# ------------------------------------------------------------------
# Install targets (non-Qt — data files)
# ------------------------------------------------------------------

include(GNUInstallDirs)

install(FILES data/shellkeep.desktop
  DESTINATION ${CMAKE_INSTALL_DATADIR}/applications)

install(FILES man/shellkeep.1
  DESTINATION ${CMAKE_INSTALL_MANDIR}/man1)

install(FILES data/icons/shellkeep.svg
  DESTINATION ${CMAKE_INSTALL_DATADIR}/icons/hicolor/scalable/apps)

install(FILES data/icons/shellkeep-48.png
  DESTINATION ${CMAKE_INSTALL_DATADIR}/icons/hicolor/48x48/apps
  RENAME shellkeep.png)

install(FILES data/icons/shellkeep-128.png
  DESTINATION ${CMAKE_INSTALL_DATADIR}/icons/hicolor/128x128/apps
  RENAME shellkeep.png)

install(FILES data/icons/shellkeep-256.png
  DESTINATION ${CMAKE_INSTALL_DATADIR}/icons/hicolor/256x256/apps
  RENAME shellkeep.png)

install(FILES data/shellkeep-autostart.desktop
  DESTINATION ${CMAKE_INSTALL_DATADIR}/shellkeep)

install(FILES data/org.shellkeep.ShellKeep.appdata.xml
  DESTINATION ${CMAKE_INSTALL_DATADIR}/metainfo)
