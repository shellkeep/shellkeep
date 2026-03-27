// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#include "SkTrayIcon.h"

#include <QApplication>
#include <QIcon>
#include <QStyle>

SkTrayIcon::SkTrayIcon(QObject *parent)
    : QObject(parent)
{
    m_trayIcon = new QSystemTrayIcon(this);
    m_trayIcon->setIcon(QIcon::fromTheme(
        QStringLiteral("utilities-terminal"),
        QApplication::style()->standardIcon(QStyle::SP_ComputerIcon)));
    m_trayIcon->setToolTip(QStringLiteral("shellkeep"));

    buildMenu();

    connect(m_trayIcon, &QSystemTrayIcon::activated,
            this, &SkTrayIcon::onActivated);
}

SkTrayIcon::~SkTrayIcon()
{
    m_trayIcon->hide();
}

void SkTrayIcon::buildMenu()
{
    m_menu = new QMenu();

    m_showHideAction = m_menu->addAction(tr("Show Window"));
    connect(m_showHideAction, &QAction::triggered,
            this, &SkTrayIcon::onShowHideToggled);

    m_menu->addSeparator();

    m_connectionsMenu = m_menu->addMenu(tr("Connections"));
    m_connectionsMenu->setEnabled(false);

    m_menu->addSeparator();

    m_quitAction = m_menu->addAction(tr("Quit"));
    connect(m_quitAction, &QAction::triggered,
            this, &SkTrayIcon::quitRequested);

    m_trayIcon->setContextMenu(m_menu);
}

void SkTrayIcon::show()
{
    m_trayIcon->show();
}

void SkTrayIcon::hide()
{
    m_trayIcon->hide();
}

void SkTrayIcon::setSessionCount(int count)
{
    m_sessionCount = count;
    if (count > 0) {
        m_trayIcon->setToolTip(
            tr("shellkeep \u2014 %n active session(s)", "", count));
    } else {
        m_trayIcon->setToolTip(QStringLiteral("shellkeep"));
    }
}

void SkTrayIcon::setConnections(const QStringList &hosts)
{
    m_connectionsMenu->clear();
    if (hosts.isEmpty()) {
        m_connectionsMenu->setEnabled(false);
        return;
    }

    m_connectionsMenu->setEnabled(true);
    for (const QString &host : hosts) {
        m_connectionsMenu->addAction(host);
    }
}

bool SkTrayIcon::isAvailable()
{
    return QSystemTrayIcon::isSystemTrayAvailable();
}

void SkTrayIcon::onActivated(QSystemTrayIcon::ActivationReason reason)
{
    if (reason == QSystemTrayIcon::Trigger ||
        reason == QSystemTrayIcon::DoubleClick) {
        Q_EMIT showWindowRequested();
    }
}

void SkTrayIcon::onShowHideToggled()
{
    Q_EMIT showWindowRequested();
}
