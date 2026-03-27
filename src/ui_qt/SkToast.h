// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#ifndef SK_TOAST_H
#define SK_TOAST_H

#include <QGraphicsOpacityEffect>
#include <QLabel>
#include <QPropertyAnimation>
#include <QTimer>
#include <QWidget>

/**
 * Animated toast notification overlay.
 *
 * Appears at the bottom-center of the parent widget, fades in,
 * stays for a configurable timeout, then fades out and self-destructs.
 *
 * FR-UI-08, FR-SESSION-11, FR-TABS-19
 */
class SkToast : public QWidget
{
    Q_OBJECT

public:
    /**
     * Show a toast notification on the given parent widget.
     *
     * @param parent     Widget to overlay the toast on.
     * @param message    Toast message text.
     * @param timeoutMs  Auto-dismiss timeout in ms (0 = default 5000).
     */
    static void show(QWidget *parent, const QString &message, int timeoutMs = 0);

protected:
    bool eventFilter(QObject *obj, QEvent *event) override;

private:
    explicit SkToast(QWidget *parent, const QString &message, int timeoutMs);

    void reposition();
    void fadeIn();
    void fadeOut();

    QLabel *m_label = nullptr;
    QGraphicsOpacityEffect *m_opacityEffect = nullptr;
    QPropertyAnimation *m_fadeAnimation = nullptr;
    QTimer *m_dismissTimer = nullptr;
};

#endif /* SK_TOAST_H */
