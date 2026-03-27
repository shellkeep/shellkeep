// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#include "SkConnFeedback.h"

#include <QEvent>
#include <QPainter>
#include <QVBoxLayout>

#include "SkStyleSheet.h"

static const char *kSpinnerFrames[] = {
    "\xe2\xa0\x8b", "\xe2\xa0\x99", "\xe2\xa0\xb9", "\xe2\xa0\xb8",
    "\xe2\xa0\xbc", "\xe2\xa0\xb4", "\xe2\xa0\xa6", "\xe2\xa0\xa7",
    "\xe2\xa0\x87", "\xe2\xa0\x8f"  /* braille spinner: ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ */
};
static constexpr int kSpinnerFrameCount = 10;
static constexpr int kSpinnerIntervalMs = 80;

SkConnFeedback::SkConnFeedback(QWidget *parent)
    : QWidget(parent)
{
    setWindowFlags(Qt::FramelessWindowHint | Qt::SubWindow);
    setAttribute(Qt::WA_TranslucentBackground);

    auto *layout = new QVBoxLayout(this);
    layout->setAlignment(Qt::AlignCenter);
    layout->setSpacing(12);

    /* Spinner label */
    m_spinnerLabel = new QLabel(this);
    m_spinnerLabel->setAlignment(Qt::AlignCenter);
    QFont spinnerFont = m_spinnerLabel->font();
    spinnerFont.setPointSize(32);
    m_spinnerLabel->setFont(spinnerFont);
    m_spinnerLabel->setStyleSheet(
        QStringLiteral("color: %1;").arg(SkStyleSheet::kBlue));
    layout->addWidget(m_spinnerLabel);

    /* Phase text */
    m_phaseLabel = new QLabel(this);
    m_phaseLabel->setAlignment(Qt::AlignCenter);
    QFont phaseFont = m_phaseLabel->font();
    phaseFont.setPointSize(14);
    m_phaseLabel->setFont(phaseFont);
    m_phaseLabel->setStyleSheet(
        QStringLiteral("color: %1;").arg(SkStyleSheet::kText));
    layout->addWidget(m_phaseLabel);

    /* Progress bar (hidden by default) */
    m_progressBar = new QProgressBar(this);
    m_progressBar->setMinimum(0);
    m_progressBar->setMaximum(100);
    m_progressBar->setVisible(false);
    m_progressBar->setFixedWidth(300);
    m_progressBar->setStyleSheet(
        QStringLiteral(
            "QProgressBar {"
            "  background-color: %1;"
            "  border: 1px solid %2;"
            "  border-radius: 4px;"
            "  height: 8px;"
            "  text-align: center;"
            "  color: transparent;"
            "}"
            "QProgressBar::chunk {"
            "  background-color: %3;"
            "  border-radius: 3px;"
            "}")
            .arg(SkStyleSheet::kSurface0, SkStyleSheet::kSurface1,
                 SkStyleSheet::kBlue));
    layout->addWidget(m_progressBar, 0, Qt::AlignCenter);

    /* Error label (hidden by default) */
    m_errorLabel = new QLabel(this);
    m_errorLabel->setAlignment(Qt::AlignCenter);
    m_errorLabel->setWordWrap(true);
    m_errorLabel->setMaximumWidth(400);
    m_errorLabel->setVisible(false);
    m_errorLabel->setStyleSheet(
        QStringLiteral("color: %1; font-size: 13px;").arg(SkStyleSheet::kRed));
    layout->addWidget(m_errorLabel);

    /* Spinner animation timer */
    m_spinnerTimer = new QTimer(this);
    m_spinnerTimer->setInterval(kSpinnerIntervalMs);
    connect(m_spinnerTimer, &QTimer::timeout, this, &SkConnFeedback::updateSpinner);
    m_spinnerTimer->start();
    updateSpinner();

    /* Watch parent for resize */
    if (parent) {
        parent->installEventFilter(this);
    }

    reposition();
    show();
    raise();
}

SkConnFeedback::~SkConnFeedback()
{
    m_spinnerTimer->stop();
}

void SkConnFeedback::setPhase(SkBridgeConnPhase phase)
{
    m_currentPhase = phase;
    m_errorMode = false;

    m_phaseLabel->setText(phaseText(phase));
    m_spinnerLabel->setVisible(true);
    m_errorLabel->setVisible(false);

    if (phase == SK_BRIDGE_PHASE_DONE) {
        m_spinnerTimer->stop();
        m_spinnerLabel->setText(QStringLiteral("\xe2\x9c\x93")); /* checkmark */
        m_spinnerLabel->setStyleSheet(
            QStringLiteral("color: %1;").arg(SkStyleSheet::kGreen));
    } else if (phase == SK_BRIDGE_PHASE_ERROR) {
        m_spinnerTimer->stop();
        m_spinnerLabel->setText(QStringLiteral("\xe2\x9c\x97")); /* cross */
        m_spinnerLabel->setStyleSheet(
            QStringLiteral("color: %1;").arg(SkStyleSheet::kRed));
    } else {
        if (!m_spinnerTimer->isActive()) {
            m_spinnerTimer->start();
            m_spinnerLabel->setStyleSheet(
                QStringLiteral("color: %1;").arg(SkStyleSheet::kBlue));
        }
    }

    /* Hide progress bar unless restoring */
    if (phase != SK_BRIDGE_PHASE_RESTORING) {
        m_progressBar->setVisible(false);
    }
}

void SkConnFeedback::setProgress(int current, int total)
{
    m_progressBar->setVisible(true);
    if (total > 0) {
        m_progressBar->setMaximum(total);
        m_progressBar->setValue(current);
        m_phaseLabel->setText(
            tr("Restoring sessions... (%1/%2)").arg(current).arg(total));
    }
}

void SkConnFeedback::setError(const QString &message)
{
    m_errorMode = true;
    m_spinnerTimer->stop();
    m_spinnerLabel->setText(QStringLiteral("\xe2\x9c\x97")); /* cross */
    m_spinnerLabel->setStyleSheet(
        QStringLiteral("color: %1;").arg(SkStyleSheet::kRed));
    m_phaseLabel->setText(tr("Connection failed"));
    m_errorLabel->setText(message);
    m_errorLabel->setVisible(true);
    m_progressBar->setVisible(false);
}

bool SkConnFeedback::eventFilter(QObject *obj, QEvent *event)
{
    if (obj == parent() && event->type() == QEvent::Resize) {
        reposition();
    }
    return QWidget::eventFilter(obj, event);
}

void SkConnFeedback::paintEvent(QPaintEvent * /*event*/)
{
    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing);

    /* Semi-transparent dark background overlay */
    painter.fillRect(rect(), QColor(0, 0, 0, 180));

    /* Central rounded card */
    QRect card(width() / 2 - 200, height() / 2 - 100, 400, 200);
    painter.setPen(Qt::NoPen);
    painter.setBrush(QColor(SkStyleSheet::kSurface0));
    painter.drawRoundedRect(card, 12, 12);

    painter.setPen(QColor(SkStyleSheet::kSurface1));
    painter.setBrush(Qt::NoBrush);
    painter.drawRoundedRect(card, 12, 12);
}

void SkConnFeedback::reposition()
{
    auto *p = parentWidget();
    if (!p)
        return;
    setGeometry(0, 0, p->width(), p->height());
}

void SkConnFeedback::updateSpinner()
{
    if (m_errorMode)
        return;
    m_spinnerFrame = (m_spinnerFrame + 1) % kSpinnerFrameCount;
    m_spinnerLabel->setText(QString::fromUtf8(kSpinnerFrames[m_spinnerFrame]));
}

QString SkConnFeedback::phaseText(SkBridgeConnPhase phase) const
{
    switch (phase) {
    case SK_BRIDGE_PHASE_IDLE:
        return tr("Preparing...");
    case SK_BRIDGE_PHASE_CONNECTING:
        return tr("Connecting...");
    case SK_BRIDGE_PHASE_AUTHENTICATING:
        return tr("Authenticating...");
    case SK_BRIDGE_PHASE_CHECKING_TMUX:
        return tr("Checking tmux...");
    case SK_BRIDGE_PHASE_LOADING_STATE:
        return tr("Loading state...");
    case SK_BRIDGE_PHASE_RESTORING:
        return tr("Restoring sessions...");
    case SK_BRIDGE_PHASE_DONE:
        return tr("Connected");
    case SK_BRIDGE_PHASE_ERROR:
        return tr("Error");
    }
    return {};
}
