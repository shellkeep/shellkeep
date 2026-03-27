// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file SkTerminalDead.cpp
 * @brief Dead session overlay for the Qt terminal widget.
 *
 * FR-HISTORY-05..08: Semi-transparent overlay shown when a session has been
 * terminated on the server. The terminal scrollback remains visible beneath
 * the overlay for the user to scroll and copy text from.
 */

#include "SkTerminalDead.h"

#include <QHBoxLayout>
#include <QPainter>
#include <QResizeEvent>
#include <QVBoxLayout>

/* ------------------------------------------------------------------ */
/* Construction / Destruction                                          */
/* ------------------------------------------------------------------ */

SkTerminalDead::SkTerminalDead(const QString &message, QWidget *parent)
    : QWidget(parent)
{
    setupUi(message);

    /* Fill the parent entirely. */
    if (parent != nullptr) {
        resize(parent->size());
    }

    /* Allow mouse events to pass through to the terminal underneath
     * for scrolling, but intercept clicks on the banner widgets. */
    setAttribute(Qt::WA_TransparentForMouseEvents, false);
}

SkTerminalDead::~SkTerminalDead() = default;

/* ------------------------------------------------------------------ */
/* UI setup                                                            */
/* ------------------------------------------------------------------ */

void SkTerminalDead::setupUi(const QString &message)
{
    /* The banner widget sits at the bottom-center of the overlay. */
    m_bannerWidget = new QWidget(this);
    m_bannerWidget->setObjectName(QStringLiteral("deadBanner"));

    auto *bannerLayout = new QVBoxLayout(m_bannerWidget);
    bannerLayout->setContentsMargins(24, 16, 24, 16);
    bannerLayout->setSpacing(8);
    bannerLayout->setAlignment(Qt::AlignCenter);

    /* Warning message label. */
    m_messageLabel = new QLabel(message, m_bannerWidget);
    m_messageLabel->setObjectName(QStringLiteral("deadMessage"));
    m_messageLabel->setAlignment(Qt::AlignCenter);
    m_messageLabel->setWordWrap(true);
    bannerLayout->addWidget(m_messageLabel);

    /* Informational text. */
    m_infoLabel = new QLabel(
        tr("The terminal content above is preserved from the session history.\n"
           "You can scroll through it and copy text."),
        m_bannerWidget);
    m_infoLabel->setObjectName(QStringLiteral("deadInfo"));
    m_infoLabel->setAlignment(Qt::AlignCenter);
    m_infoLabel->setWordWrap(true);
    bannerLayout->addWidget(m_infoLabel);

    /* "Create new session" button. */
    m_newSessionButton = new QPushButton(tr("Create New Session"),
                                          m_bannerWidget);
    m_newSessionButton->setObjectName(QStringLiteral("deadNewSession"));
    m_newSessionButton->setCursor(Qt::PointingHandCursor);
    m_newSessionButton->setToolTip(
        tr("Start a new terminal session on the server"));

    auto *buttonLayout = new QHBoxLayout();
    buttonLayout->addStretch();
    buttonLayout->addWidget(m_newSessionButton);
    buttonLayout->addStretch();
    bannerLayout->addLayout(buttonLayout);

    connect(m_newSessionButton, &QPushButton::clicked,
            this, &SkTerminalDead::newSessionRequested);

    /* Style the banner with dark semi-transparent background. */
    m_bannerWidget->setStyleSheet(QStringLiteral(
        "#deadBanner {"
        "  background-color: rgba(30, 30, 46, 0.92);"
        "  border-radius: 8px;"
        "  border: 1px solid rgba(205, 214, 244, 0.15);"
        "}"
        "#deadMessage {"
        "  color: #fab387;"
        "  font-weight: bold;"
        "  font-size: 14px;"
        "}"
        "#deadInfo {"
        "  color: rgba(205, 214, 244, 0.7);"
        "  font-size: 12px;"
        "}"
        "#deadNewSession {"
        "  background-color: #89b4fa;"
        "  color: #1e1e2e;"
        "  border: none;"
        "  border-radius: 6px;"
        "  padding: 8px 20px;"
        "  font-weight: bold;"
        "  font-size: 13px;"
        "}"
        "#deadNewSession:hover {"
        "  background-color: #b4d0fb;"
        "}"
        "#deadNewSession:pressed {"
        "  background-color: #74a8f8;"
        "}"
    ));

    repositionBanner();
}

/* ------------------------------------------------------------------ */
/* Banner positioning                                                  */
/* ------------------------------------------------------------------ */

void SkTerminalDead::repositionBanner()
{
    if (m_bannerWidget == nullptr) {
        return;
    }

    m_bannerWidget->adjustSize();

    int bannerW = std::min(m_bannerWidget->sizeHint().width(), width() - 40);
    int bannerH = m_bannerWidget->sizeHint().height();

    /* Center horizontally, position near the bottom. */
    int x = (width() - bannerW) / 2;
    int y = height() - bannerH - 20;
    if (y < 20) {
        y = 20;
    }

    m_bannerWidget->setGeometry(x, y, bannerW, bannerH);
}

/* ------------------------------------------------------------------ */
/* Public methods                                                      */
/* ------------------------------------------------------------------ */

void SkTerminalDead::setMessage(const QString &message)
{
    if (m_messageLabel != nullptr) {
        m_messageLabel->setText(message);
    }
}

/* ------------------------------------------------------------------ */
/* Paint event: semi-transparent overlay                               */
/* ------------------------------------------------------------------ */

void SkTerminalDead::paintEvent(QPaintEvent *event)
{
    Q_UNUSED(event);

    QPainter painter(this);
    painter.fillRect(rect(), QColor(0, 0, 0, 80));
}

/* ------------------------------------------------------------------ */
/* Resize event: reposition banner                                     */
/* ------------------------------------------------------------------ */

void SkTerminalDead::resizeEvent(QResizeEvent *event)
{
    QWidget::resizeEvent(event);
    repositionBanner();
}
