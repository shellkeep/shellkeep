// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_ui_qt.h
 * @brief Qt6 UI layer public API.
 *
 * Declares the Qt6 implementation of the UI layer for shellkeep.
 * This header is included by main_qt.cpp to initialize the Qt UI
 * and register it with the toolkit-agnostic bridge (sk_ui_bridge.h).
 *
 * The Qt UI layer provides: SkMainWindow, SkDialogs, SkWelcomeWidget,
 * SkToast, SkConnFeedback, SkTrayIcon, SkStyleSheet, and SkUiBridgeQt.
 */

#ifndef SK_UI_QT_H
#define SK_UI_QT_H

#ifdef __cplusplus

#include <QApplication>

extern "C" {
#include "shellkeep/sk_ui_bridge.h"
}

/**
 * Initialize the Qt UI layer and register with the bridge.
 *
 * Creates the SkUiBridgeQt singleton, populates the SkUiBridge vtable,
 * and calls sk_ui_bridge_set(). Must be called after QApplication is
 * constructed and before any connect flow begins.
 *
 * @param app  The QApplication instance.
 * @return true on success, false on failure.
 */
bool sk_ui_qt_init(QApplication *app);

/**
 * Shut down the Qt UI layer.
 *
 * Destroys the bridge singleton and frees resources.
 * Call before QApplication is destroyed.
 */
void sk_ui_qt_shutdown(void);

/**
 * Get the global SkUiBridge vtable populated by the Qt implementation.
 *
 * @return Pointer to the bridge vtable, or nullptr if not initialized.
 */
const SkUiBridge *sk_ui_qt_get_bridge(void);

#else
/* C callers should use sk_ui_bridge.h directly */
#error "sk_ui_qt.h requires C++"
#endif /* __cplusplus */

#endif /* SK_UI_QT_H */
