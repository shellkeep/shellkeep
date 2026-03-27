// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file SkTerminalSearch.cpp
 * @brief Search overlay bar for the Qt terminal widget.
 *
 * FR-TERMINAL-07: Overlay search bar with dark theme styling, text input,
 * next/prev navigation, match status, and close-on-Esc behavior.
 */

#include "SkTerminalSearch.h"

#include <QHBoxLayout>
#include <QKeyEvent>
#include <QStyle>

/* ------------------------------------------------------------------ */
/* Construction / Destruction                                          */
/* ------------------------------------------------------------------ */

SkTerminalSearch::SkTerminalSearch(QWidget *parent)
    : QWidget(parent)
{
    setupUi();
    applyStyleSheet();
}

SkTerminalSearch::~SkTerminalSearch() = default;

/* ------------------------------------------------------------------ */
/* UI setup                                                            */
/* ------------------------------------------------------------------ */

void SkTerminalSearch::setupUi()
{
    auto *layout = new QHBoxLayout(this);
    layout->setContentsMargins(8, 4, 8, 4);
    layout->setSpacing(4);

    /* Search input field. */
    m_searchInput = new QLineEdit(this);
    m_searchInput->setPlaceholderText(tr("Search scrollback..."));
    m_searchInput->setClearButtonEnabled(true);
    m_searchInput->setSizePolicy(QSizePolicy::Expanding,
                                  QSizePolicy::Preferred);
    layout->addWidget(m_searchInput);

    /* Match status label. */
    m_statusLabel = new QLabel(this);
    m_statusLabel->setMinimumWidth(60);
    m_statusLabel->setAlignment(Qt::AlignCenter);
    layout->addWidget(m_statusLabel);

    /* Previous match button. */
    m_prevButton = new QPushButton(this);
    m_prevButton->setIcon(style()->standardIcon(QStyle::SP_ArrowUp));
    m_prevButton->setToolTip(tr("Previous match (Shift+Enter)"));
    m_prevButton->setFixedSize(28, 28);
    m_prevButton->setFocusPolicy(Qt::NoFocus);
    layout->addWidget(m_prevButton);

    /* Next match button. */
    m_nextButton = new QPushButton(this);
    m_nextButton->setIcon(style()->standardIcon(QStyle::SP_ArrowDown));
    m_nextButton->setToolTip(tr("Next match (Enter)"));
    m_nextButton->setFixedSize(28, 28);
    m_nextButton->setFocusPolicy(Qt::NoFocus);
    layout->addWidget(m_nextButton);

    /* Close button. */
    m_closeButton = new QPushButton(this);
    m_closeButton->setIcon(style()->standardIcon(QStyle::SP_TitleBarCloseButton));
    m_closeButton->setToolTip(tr("Close search (Esc)"));
    m_closeButton->setFixedSize(28, 28);
    m_closeButton->setFocusPolicy(Qt::NoFocus);
    layout->addWidget(m_closeButton);

    /* Connect signals. */
    connect(m_searchInput, &QLineEdit::textChanged,
            this, &SkTerminalSearch::searchRequested);

    connect(m_nextButton, &QPushButton::clicked,
            this, &SkTerminalSearch::nextMatch);

    connect(m_prevButton, &QPushButton::clicked,
            this, &SkTerminalSearch::prevMatch);

    connect(m_closeButton, &QPushButton::clicked,
            this, &SkTerminalSearch::closed);
}

/* ------------------------------------------------------------------ */
/* Dark theme styling                                                  */
/* ------------------------------------------------------------------ */

void SkTerminalSearch::applyStyleSheet()
{
    setStyleSheet(QStringLiteral(
        "SkTerminalSearch {"
        "  background-color: #313244;"
        "  border-bottom: 1px solid #45475a;"
        "  border-radius: 0 0 6px 6px;"
        "}"
        "QLineEdit {"
        "  background-color: #1e1e2e;"
        "  color: #cdd6f4;"
        "  border: 1px solid #45475a;"
        "  border-radius: 4px;"
        "  padding: 4px 8px;"
        "  selection-background-color: #585b70;"
        "  selection-color: #cdd6f4;"
        "  font-size: 13px;"
        "}"
        "QLineEdit:focus {"
        "  border-color: #89b4fa;"
        "}"
        "QLabel {"
        "  color: #a6adc8;"
        "  font-size: 12px;"
        "}"
        "QPushButton {"
        "  background-color: #45475a;"
        "  color: #cdd6f4;"
        "  border: none;"
        "  border-radius: 4px;"
        "  padding: 2px;"
        "}"
        "QPushButton:hover {"
        "  background-color: #585b70;"
        "}"
        "QPushButton:pressed {"
        "  background-color: #6c7086;"
        "}"
    ));
}

/* ------------------------------------------------------------------ */
/* Public methods                                                      */
/* ------------------------------------------------------------------ */

QString SkTerminalSearch::searchText() const
{
    return m_searchInput != nullptr ? m_searchInput->text() : QString();
}

void SkTerminalSearch::setStatusText(const QString &text)
{
    if (m_statusLabel != nullptr) {
        m_statusLabel->setText(text);
    }
}

void SkTerminalSearch::clear()
{
    if (m_searchInput != nullptr) {
        m_searchInput->clear();
    }
    if (m_statusLabel != nullptr) {
        m_statusLabel->clear();
    }
}

void SkTerminalSearch::focusInput()
{
    if (m_searchInput != nullptr) {
        m_searchInput->setFocus();
        m_searchInput->selectAll();
    }
}

/* ------------------------------------------------------------------ */
/* Key handling                                                        */
/* ------------------------------------------------------------------ */

void SkTerminalSearch::keyPressEvent(QKeyEvent *event)
{
    switch (event->key()) {
    case Qt::Key_Escape:
        Q_EMIT closed();
        event->accept();
        return;

    case Qt::Key_Return:
    case Qt::Key_Enter:
        if (event->modifiers() & Qt::ShiftModifier) {
            Q_EMIT prevMatch();
        } else {
            Q_EMIT nextMatch();
        }
        event->accept();
        return;

    default:
        break;
    }

    QWidget::keyPressEvent(event);
}
