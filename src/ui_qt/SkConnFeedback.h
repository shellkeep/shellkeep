// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#ifndef SK_CONN_FEEDBACK_H
#define SK_CONN_FEEDBACK_H

#include <QLabel>
#include <QProgressBar>
#include <QTimer>
#include <QWidget>

#include "shellkeep/sk_ui_bridge.h"

/**
 * Connection feedback overlay widget.
 *
 * Displays a centered overlay showing the current connection phase,
 * a spinner/progress indicator, and error messages. Shown during
 * the connect flow to keep the user informed.
 *
 * FR-CONN-16
 */
class SkConnFeedback : public QWidget
{
    Q_OBJECT

public:
    explicit SkConnFeedback(QWidget *parent = nullptr);
    ~SkConnFeedback() override;

    /** Set the current connection phase. */
    void setPhase(SkBridgeConnPhase phase);

    /** Set restoration progress (current/total sessions). */
    void setProgress(int current, int total);

    /** Set error message and switch to error display mode. */
    void setError(const QString &message);

protected:
    bool eventFilter(QObject *obj, QEvent *event) override;
    void paintEvent(QPaintEvent *event) override;

private:
    void reposition();
    void updateSpinner();
    QString phaseText(SkBridgeConnPhase phase) const;

    QLabel *m_phaseLabel = nullptr;
    QLabel *m_spinnerLabel = nullptr;
    QProgressBar *m_progressBar = nullptr;
    QLabel *m_errorLabel = nullptr;
    QTimer *m_spinnerTimer = nullptr;
    int m_spinnerFrame = 0;
    SkBridgeConnPhase m_currentPhase = SK_BRIDGE_PHASE_IDLE;
    bool m_errorMode = false;
};

#endif /* SK_CONN_FEEDBACK_H */
