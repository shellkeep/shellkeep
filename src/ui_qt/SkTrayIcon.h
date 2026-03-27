// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#ifndef SK_TRAY_ICON_H
#define SK_TRAY_ICON_H

#include <QAction>
#include <QMenu>
#include <QSystemTrayIcon>

/**
 * System tray icon for shellkeep.
 *
 * Provides show/hide window toggle, active connections list,
 * and quit action. Tooltip shows "shellkeep -- N active sessions".
 */
class SkTrayIcon : public QObject
{
    Q_OBJECT

public:
    explicit SkTrayIcon(QObject *parent = nullptr);
    ~SkTrayIcon() override;

    /** Show the tray icon. */
    void show();

    /** Hide the tray icon. */
    void hide();

    /** Update the session count displayed in the tooltip. */
    void setSessionCount(int count);

    /** Update the connections submenu with current hosts. */
    void setConnections(const QStringList &hosts);

    /** Check if system tray is available. */
    static bool isAvailable();

Q_SIGNALS:
    void showWindowRequested();
    void quitRequested();

private Q_SLOTS:
    void onActivated(QSystemTrayIcon::ActivationReason reason);
    void onShowHideToggled();

private:
    void buildMenu();

    QSystemTrayIcon *m_trayIcon = nullptr;
    QMenu *m_menu = nullptr;
    QAction *m_showHideAction = nullptr;
    QMenu *m_connectionsMenu = nullptr;
    QAction *m_quitAction = nullptr;
    int m_sessionCount = 0;
};

#endif /* SK_TRAY_ICON_H */
