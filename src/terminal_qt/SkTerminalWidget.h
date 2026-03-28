// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file SkTerminalWidget.h
 * @brief Qt6 terminal widget with SSH I/O routing.
 *
 * Wraps QTermWidget (when available) or falls back to a QPlainTextEdit-based
 * terminal emulator. Routes SSH channel data via QSocketNotifier on the SSH
 * file descriptor. Key input is forwarded to the SSH channel.
 *
 * FR-TERMINAL-10..18: Font, scrollback, cursor, theme, resize, search, dead.
 */

#ifndef SK_TERMINAL_WIDGET_H
#define SK_TERMINAL_WIDGET_H

#include <QColor>
#include <QFont>
#include <QPlainTextEdit>
#include <QSocketNotifier>
#include <QWidget>

#ifdef HAVE_QTERMWIDGET
#include <qtermwidget5/qtermwidget.h>
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
 * Contains either a QTermWidget (if available) or a QPlainTextEdit-based
 * fallback terminal emulator. Handles SSH I/O, PTY resize, search overlay,
 * and dead session rendering.
 */
class SkTerminalWidget : public QWidget
{
    Q_OBJECT

public:
    explicit SkTerminalWidget(QWidget *parent = nullptr);
    ~SkTerminalWidget() override;

    /* ---- SSH I/O ---- */

    /**
     * Connect to an SSH channel for bidirectional I/O.
     *
     * @param fd       SSH connection file descriptor for QSocketNotifier.
     * @param channel  SSH channel handle (not owned).
     */
    void connectSsh(int fd, SkSshChannel *channel);

    /**
     * Disconnect SSH I/O. Widget content is preserved.
     */
    void disconnect();

    /**
     * Feed raw terminal data into the display.
     *
     * @param buf   Raw bytes.
     * @param len   Number of bytes.
     */
    void feed(const char *buf, int len);

    /**
     * Enter dead session mode with history replay and overlay banner.
     *
     * @param history  Raw history data (may be nullptr).
     * @param len      Length of history data.
     * @param message  Banner message text.
     */
    void setDead(const char *history, int len, const QString &message);

    /* ---- Properties ---- */

    /** Whether SSH I/O is currently active. */
    [[nodiscard]] bool isConnected() const;

    /** Whether in dead session (read-only) mode. */
    [[nodiscard]] bool isDead() const;

    /** Current terminal dimensions. */
    void terminalSize(int *cols, int *rows) const;

    /* ---- Configuration ---- */

    /** Set terminal font. */
    void setTerminalFont(const QFont &font);

    /** Set scrollback line count. */
    void setScrollbackLines(int lines);

    /** Set cursor shape: 0=block, 1=ibeam, 2=underline. */
    void setCursorShape(int shape);

    /* ---- Search ---- */

    /** Toggle the search overlay bar. */
    void toggleSearch();

    /** Whether the search bar is currently visible. */
    [[nodiscard]] bool isSearchVisible() const;

    /* ---- Theme ---- */

    /** Apply a theme object to the terminal. */
    void applyTheme(const SkTerminalTheme *theme);

Q_SIGNALS:
    /** Emitted when data is received from the SSH channel. */
    void dataReceived();

    /** Emitted when the SSH channel disconnects or encounters an error. */
    void disconnected();

    /** Emitted when the terminal is resized. */
    void sizeChanged(int cols, int rows);

    /** Emitted when the dead overlay "new session" button is clicked. */
    void newSessionRequested();

protected:
    void resizeEvent(QResizeEvent *event) override;
    bool eventFilter(QObject *obj, QEvent *event) override;

private Q_SLOTS:
    /** Called by QSocketNotifier when SSH fd has data available. */
    void onSshDataAvailable();

    /** Called by search overlay signals. */
    void onSearchRequested(const QString &text);
    void onSearchNext();
    void onSearchPrev();
    void onSearchClosed();

private:
    /** Calculate terminal dimensions from widget size and font metrics. */
    void recalculateSize();

    /** Set up the fallback plain-text terminal. */
    void setupFallbackTerminal();

#ifdef HAVE_QTERMWIDGET
    /** Set up the QTermWidget terminal. */
    void setupQTermWidget();
#endif

    /** Forward key input to the SSH channel. */
    void sendToChannel(const QByteArray &buf);

    /* ---- Members ---- */

#ifdef HAVE_QTERMWIDGET
    QTermWidget *m_qtermWidget = nullptr;
#endif
    QPlainTextEdit *m_fallbackTerminal = nullptr;

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
