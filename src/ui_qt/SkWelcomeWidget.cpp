// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#include "SkWelcomeWidget.h"

#include <QFont>
#include <QHBoxLayout>
#include <QKeyEvent>
#include <QLabel>
#include <QRegularExpression>
#include <QVBoxLayout>

#include "SkStyleSheet.h"

SkWelcomeWidget::SkWelcomeWidget(QWidget *parent)
    : QWidget(parent)
{
    setupUi();
}

void SkWelcomeWidget::setupUi()
{
    auto *outerLayout = new QVBoxLayout(this);
    outerLayout->setAlignment(Qt::AlignCenter);

    /* Centered content container with max width */
    auto *container = new QWidget(this);
    container->setMaximumWidth(480);
    auto *layout = new QVBoxLayout(container);
    layout->setSpacing(16);
    layout->setAlignment(Qt::AlignCenter);

    /* Logo / title */
    m_logoLabel = new QLabel(this);
    m_logoLabel->setText(QStringLiteral("\xF0\x9F\x90\x9A")); /* shell emoji */
    m_logoLabel->setAlignment(Qt::AlignCenter);
    QFont logoFont = m_logoLabel->font();
    logoFont.setPointSize(48);
    m_logoLabel->setFont(logoFont);
    layout->addWidget(m_logoLabel);

    m_titleLabel = new QLabel(QStringLiteral("shellkeep"), this);
    m_titleLabel->setAlignment(Qt::AlignCenter);
    QFont titleFont = m_titleLabel->font();
    titleFont.setPointSize(24);
    titleFont.setBold(true);
    m_titleLabel->setFont(titleFont);
    m_titleLabel->setStyleSheet(
        QStringLiteral("color: %1;").arg(SkStyleSheet::kBlue));
    layout->addWidget(m_titleLabel);

    /* First-use message */
    m_firstUseLabel = new QLabel(this);
    m_firstUseLabel->setWordWrap(true);
    m_firstUseLabel->setAlignment(Qt::AlignCenter);
    m_firstUseLabel->setText(
        tr("Welcome to shellkeep! Enter a host below to get started.\n"
           "Your SSH sessions will survive disconnects, sleep, and reboots."));
    m_firstUseLabel->setStyleSheet(
        QStringLiteral("color: %1; padding: 8px;").arg(SkStyleSheet::kSubtext0));
    m_firstUseLabel->setVisible(false);
    layout->addWidget(m_firstUseLabel);

    /* Spacer */
    layout->addSpacing(16);

    /* Host input + port + connect button */
    auto *inputRow = new QHBoxLayout();
    inputRow->setSpacing(8);

    m_hostInput = new QLineEdit(this);
    m_hostInput->setPlaceholderText(QStringLiteral("user@host"));
    m_hostInput->setMinimumHeight(40);
    QFont inputFont = m_hostInput->font();
    inputFont.setPointSize(14);
    m_hostInput->setFont(inputFont);
    inputRow->addWidget(m_hostInput, 1);

    /* Port label + spinbox */
    auto *portLabel = new QLabel(tr("Port:"), this);
    portLabel->setStyleSheet(
        QStringLiteral("color: %1; font-size: 13px;").arg(SkStyleSheet::kSubtext0));
    inputRow->addWidget(portLabel);

    m_portInput = new QSpinBox(this);
    m_portInput->setRange(1, 65535);
    m_portInput->setValue(22);
    m_portInput->setMinimumHeight(40);
    m_portInput->setFixedWidth(80);
    m_portInput->setButtonSymbols(QAbstractSpinBox::NoButtons);
    QFont portFont = m_portInput->font();
    portFont.setPointSize(14);
    m_portInput->setFont(portFont);
    inputRow->addWidget(m_portInput);

    m_connectButton = new QPushButton(tr("Connect"), this);
    m_connectButton->setMinimumHeight(40);
    m_connectButton->setMinimumWidth(100);
    m_connectButton->setDefault(true);
    m_connectButton->setStyleSheet(
        QStringLiteral(
            "QPushButton {"
            "  background-color: %1;"
            "  color: %2;"
            "  font-weight: bold;"
            "  font-size: 14px;"
            "  border-radius: 6px;"
            "  padding: 8px 16px;"
            "}"
            "QPushButton:hover {"
            "  background-color: %3;"
            "}"
            "QPushButton:pressed {"
            "  background-color: %4;"
            "}"
            "QPushButton:disabled {"
            "  background-color: %5;"
            "  color: %6;"
            "}")
            .arg(SkStyleSheet::kBlue, SkStyleSheet::kCrust,
                 SkStyleSheet::kLavender, SkStyleSheet::kMauve,
                 SkStyleSheet::kSurface1, SkStyleSheet::kOverlay1));
    inputRow->addWidget(m_connectButton);

    layout->addLayout(inputRow);

    /* Recent connections */
    m_recentLabel = new QLabel(tr("Recent Connections"), this);
    m_recentLabel->setStyleSheet(
        QStringLiteral("color: %1; font-size: 12px; margin-top: 16px;")
            .arg(SkStyleSheet::kOverlay1));
    m_recentLabel->setVisible(false);
    layout->addWidget(m_recentLabel);

    m_recentList = new QListWidget(this);
    m_recentList->setMaximumHeight(200);
    m_recentList->setVisible(false);
    m_recentList->setAlternatingRowColors(true);
    m_recentList->setStyleSheet(
        QStringLiteral(
            "QListWidget {"
            "  background-color: %1;"
            "  border: 1px solid %2;"
            "  border-radius: 6px;"
            "  padding: 4px;"
            "}"
            "QListWidget::item {"
            "  padding: 6px 8px;"
            "  border-radius: 4px;"
            "}"
            "QListWidget::item:hover {"
            "  background-color: %3;"
            "}"
            "QListWidget::item:selected {"
            "  background-color: %4;"
            "  color: %5;"
            "}")
            .arg(SkStyleSheet::kSurface0, SkStyleSheet::kSurface1,
                 SkStyleSheet::kSurface1, SkStyleSheet::kBlue,
                 SkStyleSheet::kCrust));
    layout->addWidget(m_recentList);

    outerLayout->addWidget(container);

    /* Connections */
    connect(m_connectButton, &QPushButton::clicked,
            this, &SkWelcomeWidget::onConnectClicked);
    connect(m_hostInput, &QLineEdit::returnPressed,
            this, &SkWelcomeWidget::onConnectClicked);
    connect(m_recentList, &QListWidget::itemDoubleClicked,
            this, &SkWelcomeWidget::onRecentItemDoubleClicked);
}

void SkWelcomeWidget::setRecentConnections(const QStringList &connections)
{
    m_recentList->clear();
    bool hasRecent = !connections.isEmpty();
    m_recentLabel->setVisible(hasRecent);
    m_recentList->setVisible(hasRecent);

    for (const QString &conn : connections) {
        m_recentList->addItem(conn);
    }
}

void SkWelcomeWidget::setFirstUse(bool firstUse)
{
    m_firstUse = firstUse;
    m_firstUseLabel->setVisible(firstUse);
}

QString SkWelcomeWidget::hostInput() const
{
    return m_hostInput->text().trimmed();
}

void SkWelcomeWidget::reset()
{
    m_hostInput->clear();
    m_hostInput->setFocus();
    setConnecting(false);
}

void SkWelcomeWidget::setConnecting(bool connecting)
{
    m_connecting = connecting;
    m_hostInput->setEnabled(!connecting);
    m_portInput->setEnabled(!connecting);
    m_recentList->setEnabled(!connecting);

    if (connecting) {
        m_connectButton->setText(tr("Cancel"));
        m_connectButton->setEnabled(true);
    } else {
        m_connectButton->setText(tr("Connect"));
        m_connectButton->setEnabled(true);
    }
}

void SkWelcomeWidget::onConnectClicked()
{
    if (m_connecting) {
        Q_EMIT cancelRequested();
        return;
    }

    QString input = m_hostInput->text().trimmed();
    if (input.isEmpty())
        return;

    QString host, user;
    int port = 0;
    parseHostInput(input, host, user, port);

    /* Use the port spinbox value if the address didn't contain an explicit port */
    if (port == 0)
        port = m_portInput->value();

    Q_EMIT connectRequested(host, user, port);
}

void SkWelcomeWidget::onRecentItemDoubleClicked(QListWidgetItem *item)
{
    if (!item || m_connecting)
        return;

    QString text = item->text().trimmed();
    if (text.isEmpty())
        return;

    QString host, user;
    int port = 0;
    parseHostInput(text, host, user, port);

    if (port == 0)
        port = m_portInput->value();

    Q_EMIT connectRequested(host, user, port);
}

void SkWelcomeWidget::parseHostInput(const QString &input,
                                     QString &host, QString &user, int &port)
{
    /* Parse user@host:port format */
    QString remaining = input;
    port = 0;

    /* Extract user@ */
    int atIdx = remaining.indexOf('@');
    if (atIdx >= 0) {
        user = remaining.left(atIdx);
        remaining = remaining.mid(atIdx + 1);
    }

    /* Extract :port */
    /* Handle IPv6 [host]:port and regular host:port */
    if (remaining.startsWith('[')) {
        int bracketEnd = remaining.indexOf(']');
        if (bracketEnd >= 0) {
            host = remaining.mid(1, bracketEnd - 1);
            if (bracketEnd + 1 < remaining.size() && remaining[bracketEnd + 1] == ':') {
                bool ok = false;
                port = remaining.mid(bracketEnd + 2).toInt(&ok);
                if (!ok) port = 0;
            }
        } else {
            host = remaining;
        }
    } else {
        int colonIdx = remaining.lastIndexOf(':');
        if (colonIdx >= 0) {
            bool ok = false;
            int maybePort = remaining.mid(colonIdx + 1).toInt(&ok);
            if (ok && maybePort > 0 && maybePort <= 65535) {
                port = maybePort;
                host = remaining.left(colonIdx);
            } else {
                host = remaining;
            }
        } else {
            host = remaining;
        }
    }
}
