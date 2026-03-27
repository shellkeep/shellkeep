// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file SkTerminalDead.h
 * @brief Dead session overlay for the Qt terminal widget.
 *
 * FR-HISTORY-05..08: Semi-transparent overlay shown when a session has been
 * terminated on the server. Displays a message banner and a "Create new
 * session" button.
 */

#ifndef SK_TERMINAL_DEAD_H
#define SK_TERMINAL_DEAD_H

#include <QLabel>
#include <QPushButton>
#include <QWidget>

/**
 * Semi-transparent overlay displayed on top of a dead terminal session.
 *
 * Shows a warning message and provides a button to create a new session.
 * The overlay allows the terminal scrollback to remain visible underneath.
 */
class SkTerminalDead : public QWidget
{
    Q_OBJECT

public:
    /**
     * Create the dead session overlay.
     *
     * @param message  Banner message to display.
     * @param parent   Parent widget (the terminal).
     */
    explicit SkTerminalDead(const QString &message, QWidget *parent = nullptr);
    ~SkTerminalDead() override;

    /** Update the banner message text. */
    void setMessage(const QString &message);

Q_SIGNALS:
    /** Emitted when the user clicks "Create new session". */
    void newSessionRequested();

protected:
    void paintEvent(QPaintEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;

private:
    QLabel *m_messageLabel = nullptr;
    QLabel *m_infoLabel = nullptr;
    QPushButton *m_newSessionButton = nullptr;
    QWidget *m_bannerWidget = nullptr;

    void setupUi(const QString &message);
    void repositionBanner();
};

#endif /* SK_TERMINAL_DEAD_H */
