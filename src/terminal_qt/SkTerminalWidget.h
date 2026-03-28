// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file SkTerminalWidget.h
 * @brief Qt6 terminal widget with SSH I/O routing.
 *
 * Wraps QTermWidget for full terminal emulation (VT100/xterm).
 * Routes SSH channel data via QSocketNotifier on the SSH file descriptor.
 * Key input is forwarded to the SSH channel.
 *
 * FR-TERMINAL-10..18: Font, scrollback, cursor, theme, resize, search, dead.
 */

#ifndef SK_TERMINAL_WIDGET_H
#define SK_TERMINAL_WIDGET_H

#include <QColor>
#include <QFont>
#include <QSocketNotifier>
#include <QWidget>

#ifdef HAVE_QTERMWIDGET
#include <qtermwidget.h>
#endif

/* Include C backend headers. */
#include "shellkeep/sk_config.h"
#include "shellkeep/sk_ssh.h"
#include "shellkeep/sk_types.h"

class SkTerminalSearch;
class SkTerminalDead;
class SkTerminalTheme;

/**
 * Main terminal widget for shellkeep Qt6 UI.
 *
 * Contains a QTermWidget for full VT100/xterm terminal emulation.
 * Handles SSH I/O, PTY resize, search overlay, and dead session rendering.
 */
class SkTerminalWidget : public QWidget
{
    Q_OBJECT

public:
    explicit SkTerminalWidget(QWidget *parent = nullptr);
    ~SkTerminalWidget() override;

    /* ---- SSH I/O ---- */

    void connectSsh(int fd, SkSshChannel *channel);
    void disconnect();
    void feed(const char *buf, int len);
    void setDead(const char *history, int len, const QString &message);

    /* ---- Properties ---- */

    [[nodiscard]] bool isConnected() const;
    [[nodiscard]] bool isDead() const;
    void terminalSize(int *cols, int *rows) const;

    /* ---- Configuration ---- */

    void setTerminalFont(const QFont &font);
    void setScrollbackLines(int lines);
    void setCursorShape(int shape);

    /* ---- Search ---- */

    void toggleSearch();
    [[nodiscard]] bool isSearchVisible() const;

    /* ---- Theme ---- */

    void applyTheme(const SkTerminalTheme *theme);

Q_SIGNALS:
    void dataReceived();
    void disconnected();
    void sizeChanged(int cols, int rows);
    void newSessionRequested();

protected:
    void resizeEvent(QResizeEvent *event) override;
    bool eventFilter(QObject *obj, QEvent *event) override;

private Q_SLOTS:
    void onSshDataAvailable();
    void onSearchRequested(const QString &text);
    void onSearchNext();
    void onSearchPrev();
    void onSearchClosed();

private:
    void recalculateSize();
#ifdef HAVE_QTERMWIDGET
    void setupQTermWidget();
#endif
    void sendToChannel(const QByteArray &buf);

    /* ---- Members ---- */

#ifdef HAVE_QTERMWIDGET
    QTermWidget *m_qtermWidget = nullptr;
#endif

    SkTerminalSearch *m_searchBar = nullptr;
    SkTerminalDead *m_deadOverlay = nullptr;

    QSocketNotifier *m_sshNotifier = nullptr;
    SkSshChannel *m_channel = nullptr;
    int m_sshFd = -1;

    bool m_connected = false;
    bool m_dead = false;

    int m_cols = 80;
    int m_rows = 24;
    int m_scrollbackLines = 10000;

    QFont m_font;
    int m_cursorShape = 0;
};

#endif /* SK_TERMINAL_WIDGET_H */
