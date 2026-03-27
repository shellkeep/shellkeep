// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#ifndef SK_WELCOME_WIDGET_H
#define SK_WELCOME_WIDGET_H

#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QWidget>

/**
 * Welcome screen shown when no connection is active.
 *
 * Displays the shellkeep logo, a host input field, recent connections,
 * and a connect button. On first use, shows a brief welcome message.
 *
 * FR-UI-01..04
 */
class SkWelcomeWidget : public QWidget
{
    Q_OBJECT

public:
    explicit SkWelcomeWidget(QWidget *parent = nullptr);

    /** Set the list of recent connections. */
    void setRecentConnections(const QStringList &connections);

    /** Set whether this is the first time the app is used. */
    void setFirstUse(bool firstUse);

    /** Get the current host input text. */
    QString hostInput() const;

    /** Clear input and reset state. */
    void reset();

Q_SIGNALS:
    /**
     * Emitted when the user requests a connection.
     * @param host  Hostname or IP.
     * @param user  Username (may be empty).
     * @param port  Port number (0 = default 22).
     */
    void connectRequested(const QString &host, const QString &user, int port);

private Q_SLOTS:
    void onConnectClicked();
    void onRecentItemDoubleClicked(QListWidgetItem *item);

private:
    void setupUi();
    void parseHostInput(const QString &input, QString &host, QString &user, int &port);

    QLabel *m_logoLabel = nullptr;
    QLabel *m_titleLabel = nullptr;
    QLabel *m_firstUseLabel = nullptr;
    QLineEdit *m_hostInput = nullptr;
    QPushButton *m_connectButton = nullptr;
    QListWidget *m_recentList = nullptr;
    QLabel *m_recentLabel = nullptr;
    bool m_firstUse = false;
};

#endif /* SK_WELCOME_WIDGET_H */
