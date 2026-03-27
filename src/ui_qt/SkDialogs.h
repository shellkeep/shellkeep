// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#ifndef SK_DIALOGS_H
#define SK_DIALOGS_H

#include <QStringList>
#include <QWidget>

extern "C" {
#include "shellkeep/sk_ui_bridge.h"
}

/**
 * All modal dialogs for shellkeep.
 *
 * Every method is static and thread-safe: when called from a non-GUI
 * thread, the call is dispatched to the UI thread via
 * QMetaObject::invokeMethod(Qt::BlockingQueuedConnection).
 *
 * FR-CONN-01..10, FR-LOCK-05, FR-ENV-03, FR-TABS-17
 */
class SkDialogs
{
public:
    /** TOFU dialog for unknown host key (FR-CONN-03). */
    static SkBridgeHostKeyResult hostKeyUnknown(QWidget *parent,
                                                 const QString &hostname,
                                                 const QString &fingerprint,
                                                 const QString &keyType);

    /** Warning dialog for changed host key (FR-CONN-02). Blocks until dismissed. */
    static void hostKeyChanged(QWidget *parent,
                               const QString &hostname,
                               const QString &oldFingerprint,
                               const QString &newFingerprint,
                               const QString &keyType);

    /** Password input dialog (FR-CONN-09). Returns empty string if cancelled. */
    static QString authPassword(QWidget *parent, const QString &prompt);

    /** Keyboard-interactive / MFA dialog (FR-CONN-10). Returns empty list if cancelled. */
    static QStringList authMfa(QWidget *parent,
                               const QString &name,
                               const QString &instruction,
                               const QStringList &prompts,
                               const QList<bool> &showInput);

    /** Passphrase dialog for encrypted key (FR-CONN-09). Returns empty if cancelled. */
    static QString authPassphrase(QWidget *parent, const QString &keyPath);

    /** Lock conflict dialog (FR-LOCK-05). Returns true to take over. */
    static bool conflictDialog(QWidget *parent,
                               const QString &hostname,
                               const QString &connectedAt);

    /** Environment selection dialog (FR-ENV-03). Returns empty if cancelled. */
    static QString environmentSelect(QWidget *parent,
                                     const QStringList &envNames,
                                     const QString &lastEnv);

    /** Close window dialog (FR-TABS-17). */
    static SkBridgeCloseResult closeWindow(QWidget *parent, int nActive);

    /** Error dialog. */
    static void errorDialog(QWidget *parent,
                            const QString &title,
                            const QString &message);

    /** Info dialog. */
    static void infoDialog(QWidget *parent,
                           const QString &title,
                           const QString &message);

private:
    SkDialogs() = delete;

    /** Helper: ensure function runs on the GUI thread. */
    template <typename Func>
    static auto runOnUiThread(Func &&func) -> decltype(func());
};

#endif /* SK_DIALOGS_H */
