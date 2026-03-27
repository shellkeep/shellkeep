// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#include "SkToast.h"

#include <QEvent>
#include <QHBoxLayout>

#include "SkStyleSheet.h"

static constexpr int kDefaultTimeoutMs = 5000;
static constexpr int kFadeDurationMs = 300;
static constexpr int kBottomMargin = 32;

SkToast::SkToast(QWidget *parent, const QString &message, int timeoutMs)
    : QWidget(parent)
{
    setWindowFlags(Qt::FramelessWindowHint | Qt::SubWindow);
    setAttribute(Qt::WA_TransparentForMouseEvents, false);
    setAttribute(Qt::WA_DeleteOnClose);

    /* Layout */
    auto *layout = new QHBoxLayout(this);
    layout->setContentsMargins(16, 10, 16, 10);

    m_label = new QLabel(message, this);
    m_label->setWordWrap(true);
    m_label->setStyleSheet(
        QStringLiteral("color: %1; font-size: 13px;").arg(SkStyleSheet::kText));
    layout->addWidget(m_label);

    /* Styling */
    setStyleSheet(
        QStringLiteral(
            "SkToast {"
            "  background-color: %1;"
            "  border: 1px solid %2;"
            "  border-radius: 8px;"
            "}")
            .arg(SkStyleSheet::kSurface0, SkStyleSheet::kSurface1));

    setMinimumWidth(200);
    setMaximumWidth(500);
    adjustSize();

    /* Opacity effect for fade animation */
    m_opacityEffect = new QGraphicsOpacityEffect(this);
    m_opacityEffect->setOpacity(0.0);
    setGraphicsEffect(m_opacityEffect);

    /* Fade animation */
    m_fadeAnimation = new QPropertyAnimation(m_opacityEffect, "opacity", this);
    m_fadeAnimation->setDuration(kFadeDurationMs);

    /* Dismiss timer */
    int timeout = (timeoutMs > 0) ? timeoutMs : kDefaultTimeoutMs;
    m_dismissTimer = new QTimer(this);
    m_dismissTimer->setSingleShot(true);
    m_dismissTimer->setInterval(timeout);
    connect(m_dismissTimer, &QTimer::timeout, this, &SkToast::fadeOut);

    /* Watch parent for resize events so we can reposition */
    if (parent) {
        parent->installEventFilter(this);
    }

    reposition();
    QWidget::show();
    raise();
    fadeIn();
}

void SkToast::show(QWidget *parent, const QString &message, int timeoutMs)
{
    if (!parent)
        return;

    /* The toast self-destructs after fadeout via WA_DeleteOnClose */
    new SkToast(parent, message, timeoutMs);
}

bool SkToast::eventFilter(QObject *obj, QEvent *event)
{
    if (obj == parent() && event->type() == QEvent::Resize) {
        reposition();
    }
    return QWidget::eventFilter(obj, event);
}

void SkToast::reposition()
{
    auto *p = parentWidget();
    if (!p)
        return;

    int x = (p->width() - width()) / 2;
    int y = p->height() - height() - kBottomMargin;
    move(x, y);
}

void SkToast::fadeIn()
{
    m_fadeAnimation->stop();
    m_fadeAnimation->setStartValue(0.0);
    m_fadeAnimation->setEndValue(1.0);
    m_fadeAnimation->start();
    m_dismissTimer->start();
}

void SkToast::fadeOut()
{
    m_fadeAnimation->stop();
    m_fadeAnimation->setStartValue(1.0);
    m_fadeAnimation->setEndValue(0.0);

    connect(m_fadeAnimation, &QPropertyAnimation::finished,
            this, &QWidget::close);

    m_fadeAnimation->start();
}
