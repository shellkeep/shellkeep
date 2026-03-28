// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file SkTerminalWidget.cpp
 * @brief Qt6 terminal widget with SSH I/O routing.
 *
 * Wraps QTermWidget for full VT100/xterm terminal emulation.
 * Routes SSH channel data via QSocketNotifier on the SSH file descriptor.
 * Key input is forwarded to the SSH channel via sk_ssh_channel_write().
 * PTY resize via sk_ssh_channel_resize_pty() on resizeEvent.
 *
 * FR-TERMINAL-10..18
 */

#include "SkTerminalWidget.h"

#include "SkTerminalDead.h"
#include "SkTerminalSearch.h"
#include "SkTerminalTheme.h"

#include <QApplication>
#include <QFontDatabase>
#include <QKeyEvent>
#include <QResizeEvent>
#include <QScrollBar>
#include <QVBoxLayout>

#include <cstring>

/* Read buffer size for SSH channel data. */
static constexpr int SSH_READ_BUFSIZE = 65536;

/* ------------------------------------------------------------------ */
/* Construction / Destruction                                          */
/* ------------------------------------------------------------------ */

SkTerminalWidget::SkTerminalWidget(QWidget *parent)
    : QWidget(parent)
{
    /* Set a reasonable default monospace font. */
    m_font = QFontDatabase::systemFont(QFontDatabase::FixedFont);
    m_font.setPointSize(12);

    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);

    setupQTermWidget();

    setFocusPolicy(Qt::StrongFocus);
}

SkTerminalWidget::~SkTerminalWidget()
{
    disconnect();
}

/* ------------------------------------------------------------------ */
/* QTermWidget setup                                                   */
/* ------------------------------------------------------------------ */

void SkTerminalWidget::setupQTermWidget()
{
    m_qtermWidget = new QTermWidget(0, this);
    m_qtermWidget->setTerminalFont(m_font);
    m_qtermWidget->setScrollBarPosition(QTermWidget::ScrollBarRight);
    m_qtermWidget->setHistorySize(m_scrollbackLines);

    /* Install event filter for key capture. */
    m_qtermWidget->installEventFilter(this);

    layout()->addWidget(m_qtermWidget);
}

/* ------------------------------------------------------------------ */
/* SSH I/O connection                                                   */
/* ------------------------------------------------------------------ */

void SkTerminalWidget::connectSsh(int fd, SkSshChannel *channel)
{
    if (m_connected) {
        disconnect();
    }

    m_sshFd = fd;
    m_channel = channel;

    /* Set up QSocketNotifier to watch the SSH fd for incoming data. */
    m_sshNotifier = new QSocketNotifier(fd, QSocketNotifier::Read, this);
    QObject::connect(m_sshNotifier, &QSocketNotifier::activated,
                     this, &SkTerminalWidget::onSshDataAvailable);
    m_sshNotifier->setEnabled(true);

    m_connected = true;
    m_dead = false;

    /* Send initial PTY size. */
    recalculateSize();
    GError *err = nullptr;
    sk_ssh_channel_resize_pty(m_channel, m_cols, m_rows, &err);
    if (err != nullptr) {
        g_error_free(err);
    }
}

void SkTerminalWidget::disconnect()
{
    if (!m_connected && m_sshNotifier == nullptr) {
        return;
    }

    if (m_sshNotifier != nullptr) {
        m_sshNotifier->setEnabled(false);
        delete m_sshNotifier;
        m_sshNotifier = nullptr;
    }

    m_channel = nullptr;
    m_sshFd = -1;
    m_connected = false;

    Q_EMIT disconnected();
}

/* ------------------------------------------------------------------ */
/* Data feeding                                                        */
/* ------------------------------------------------------------------ */

void SkTerminalWidget::feed(const char *buf, int len)
{
    if (buf == nullptr || len <= 0 || m_qtermWidget == nullptr) {
        return;
    }

    m_qtermWidget->sendText(QString::fromUtf8(buf, len));
}

/* ------------------------------------------------------------------ */
/* Dead session mode                                                   */
/* ------------------------------------------------------------------ */

void SkTerminalWidget::setDead(const char *history, int len,
                               const QString &message)
{
    if (m_connected) {
        disconnect();
    }

    /* Feed history data to reconstruct the session display. */
    if (history != nullptr && len > 0) {
        static constexpr int CHUNK_SIZE = 65536;
        int offset = 0;
        while (offset < len) {
            int chunk = std::min(CHUNK_SIZE, len - offset);
            feed(history + offset, chunk);
            offset += chunk;
        }
    }

    m_dead = true;

    /* Remove existing dead overlay if any. */
    if (m_deadOverlay != nullptr) {
        m_deadOverlay->deleteLater();
        m_deadOverlay = nullptr;
    }

    /* Create and show the dead session overlay. */
    m_deadOverlay = new SkTerminalDead(message, this);
    QObject::connect(m_deadOverlay, &SkTerminalDead::newSessionRequested,
                     this, &SkTerminalWidget::newSessionRequested);
    m_deadOverlay->resize(size());
    m_deadOverlay->show();
    m_deadOverlay->raise();
}

/* ------------------------------------------------------------------ */
/* Properties                                                          */
/* ------------------------------------------------------------------ */

bool SkTerminalWidget::isConnected() const
{
    return m_connected;
}

bool SkTerminalWidget::isDead() const
{
    return m_dead;
}

void SkTerminalWidget::terminalSize(int *cols, int *rows) const
{
    if (cols != nullptr) {
        *cols = m_cols;
    }
    if (rows != nullptr) {
        *rows = m_rows;
    }
}

/* ------------------------------------------------------------------ */
/* Configuration                                                       */
/* ------------------------------------------------------------------ */

void SkTerminalWidget::setTerminalFont(const QFont &font)
{
    m_font = font;

    if (m_qtermWidget != nullptr) {
        m_qtermWidget->setTerminalFont(font);
    }

    recalculateSize();
}

void SkTerminalWidget::setScrollbackLines(int lines)
{
    m_scrollbackLines = lines;

    if (m_qtermWidget != nullptr) {
        m_qtermWidget->setHistorySize(lines);
    }
}

void SkTerminalWidget::setCursorShape(int shape)
{
    m_cursorShape = shape;

    if (m_qtermWidget != nullptr) {
        /* QTermWidget uses Konsole key mode enum:
         * 0=block, 1=underline, 2=ibeam. Map our values. */
        int konsoleShape = 0;
        switch (shape) {
        case 0: konsoleShape = 0; break; /* block */
        case 1: konsoleShape = 2; break; /* ibeam */
        case 2: konsoleShape = 1; break; /* underline */
        default: break;
        }
        m_qtermWidget->setKeyboardCursorShape(konsoleShape);
    }
}

/* ------------------------------------------------------------------ */
/* Search                                                              */
/* ------------------------------------------------------------------ */

void SkTerminalWidget::toggleSearch()
{
    if (m_searchBar == nullptr) {
        m_searchBar = new SkTerminalSearch(this);

        QObject::connect(m_searchBar, &SkTerminalSearch::searchRequested,
                         this, &SkTerminalWidget::onSearchRequested);
        QObject::connect(m_searchBar, &SkTerminalSearch::nextMatch,
                         this, &SkTerminalWidget::onSearchNext);
        QObject::connect(m_searchBar, &SkTerminalSearch::prevMatch,
                         this, &SkTerminalWidget::onSearchPrev);
        QObject::connect(m_searchBar, &SkTerminalSearch::closed,
                         this, &SkTerminalWidget::onSearchClosed);

        m_searchBar->setFixedWidth(width());
        m_searchBar->move(0, 0);
    }

    if (m_searchBar->isVisible()) {
        m_searchBar->hide();
        m_searchBar->clear();

        if (m_qtermWidget != nullptr) {
            m_qtermWidget->setFocus();
        }
    } else {
        m_searchBar->show();
        m_searchBar->raise();
        m_searchBar->focusInput();
    }
}

bool SkTerminalWidget::isSearchVisible() const
{
    return m_searchBar != nullptr && m_searchBar->isVisible();
}

/* ------------------------------------------------------------------ */
/* Theme                                                               */
/* ------------------------------------------------------------------ */

void SkTerminalWidget::applyTheme(const SkTerminalTheme *theme)
{
    if (theme == nullptr) {
        return;
    }

    theme->applyToTerminal(this);
}

/* ------------------------------------------------------------------ */
/* Resize handling                                                     */
/* ------------------------------------------------------------------ */

void SkTerminalWidget::resizeEvent(QResizeEvent *event)
{
    QWidget::resizeEvent(event);

    int oldCols = m_cols;
    int oldRows = m_rows;
    recalculateSize();

    if (m_searchBar != nullptr) {
        m_searchBar->setFixedWidth(width());
    }

    if (m_deadOverlay != nullptr) {
        m_deadOverlay->resize(size());
    }

    if (m_connected && m_channel != nullptr &&
        (m_cols != oldCols || m_rows != oldRows)) {
        GError *err = nullptr;
        sk_ssh_channel_resize_pty(m_channel, m_cols, m_rows, &err);
        if (err != nullptr) {
            g_error_free(err);
        }
        Q_EMIT sizeChanged(m_cols, m_rows);
    }
}

void SkTerminalWidget::recalculateSize()
{
    if (m_qtermWidget != nullptr) {
        m_cols = m_qtermWidget->screenColumnsCount();
        m_rows = m_qtermWidget->screenLinesCount();
    }
}

/* ------------------------------------------------------------------ */
/* Event filter: key input forwarding to SSH channel                   */
/* ------------------------------------------------------------------ */

bool SkTerminalWidget::eventFilter(QObject *obj, QEvent *event)
{
    if (event->type() == QEvent::KeyPress && m_connected && !m_dead) {
        auto *keyEvent = static_cast<QKeyEvent *>(event);

        QByteArray rawData;

        switch (keyEvent->key()) {
        case Qt::Key_Return:
        case Qt::Key_Enter:
            rawData = QByteArray("\r", 1);
            break;
        case Qt::Key_Backspace:
            rawData = QByteArray("\x7f", 1);
            break;
        case Qt::Key_Tab:
            rawData = QByteArray("\t", 1);
            break;
        case Qt::Key_Escape:
            rawData = QByteArray("\x1b", 1);
            break;
        case Qt::Key_Up:
            rawData = QByteArray("\x1b[A", 3);
            break;
        case Qt::Key_Down:
            rawData = QByteArray("\x1b[B", 3);
            break;
        case Qt::Key_Right:
            rawData = QByteArray("\x1b[C", 3);
            break;
        case Qt::Key_Left:
            rawData = QByteArray("\x1b[D", 3);
            break;
        case Qt::Key_Home:
            rawData = QByteArray("\x1b[H", 3);
            break;
        case Qt::Key_End:
            rawData = QByteArray("\x1b[F", 3);
            break;
        case Qt::Key_Insert:
            rawData = QByteArray("\x1b[2~", 4);
            break;
        case Qt::Key_Delete:
            rawData = QByteArray("\x1b[3~", 4);
            break;
        case Qt::Key_PageUp:
            rawData = QByteArray("\x1b[5~", 4);
            break;
        case Qt::Key_PageDown:
            rawData = QByteArray("\x1b[6~", 4);
            break;
        case Qt::Key_F1:  rawData = QByteArray("\x1bOP", 3); break;
        case Qt::Key_F2:  rawData = QByteArray("\x1bOQ", 3); break;
        case Qt::Key_F3:  rawData = QByteArray("\x1bOR", 3); break;
        case Qt::Key_F4:  rawData = QByteArray("\x1bOS", 3); break;
        case Qt::Key_F5:  rawData = QByteArray("\x1b[15~", 5); break;
        case Qt::Key_F6:  rawData = QByteArray("\x1b[17~", 5); break;
        case Qt::Key_F7:  rawData = QByteArray("\x1b[18~", 5); break;
        case Qt::Key_F8:  rawData = QByteArray("\x1b[19~", 5); break;
        case Qt::Key_F9:  rawData = QByteArray("\x1b[20~", 5); break;
        case Qt::Key_F10: rawData = QByteArray("\x1b[21~", 5); break;
        case Qt::Key_F11: rawData = QByteArray("\x1b[23~", 5); break;
        case Qt::Key_F12: rawData = QByteArray("\x1b[24~", 5); break;
        default:
            if (keyEvent->modifiers() & Qt::ControlModifier &&
                keyEvent->key() >= Qt::Key_A &&
                keyEvent->key() <= Qt::Key_Z) {
                char ctrl = static_cast<char>(keyEvent->key() - Qt::Key_A + 1);
                rawData = QByteArray(&ctrl, 1);
            } else {
                QString text = keyEvent->text();
                if (!text.isEmpty()) {
                    rawData = text.toUtf8();
                }
            }
            break;
        }

        if (!rawData.isEmpty()) {
            sendToChannel(rawData);
        }
    }

    return QWidget::eventFilter(obj, event);
}

/* ------------------------------------------------------------------ */
/* SSH data slots                                                      */
/* ------------------------------------------------------------------ */

void SkTerminalWidget::onSshDataAvailable()
{
    if (!m_connected || m_channel == nullptr) {
        return;
    }

    char buf[SSH_READ_BUFSIZE];

    for (;;) {
        int nbytes = sk_ssh_channel_read_nonblocking(m_channel, buf,
                                                      sizeof(buf));
        if (nbytes > 0) {
            feed(buf, nbytes);
            Q_EMIT dataReceived();
        } else if (nbytes == 0) {
            break;
        } else {
            disconnect();
            break;
        }
    }
}

void SkTerminalWidget::sendToChannel(const QByteArray &buf)
{
    if (!m_connected || m_channel == nullptr || buf.isEmpty()) {
        return;
    }

    int written = sk_ssh_channel_write(m_channel, buf.constData(),
                                        static_cast<size_t>(buf.size()));
    if (written < 0) {
        disconnect();
    }
}

/* ------------------------------------------------------------------ */
/* Search slots                                                        */
/* ------------------------------------------------------------------ */

void SkTerminalWidget::onSearchRequested(const QString &text)
{
    if (text.isEmpty()) {
        if (m_searchBar != nullptr) {
            m_searchBar->setStatusText(QString());
        }
        return;
    }

    if (m_qtermWidget != nullptr) {
        m_qtermWidget->search(text, true, false);
        m_searchBar->setStatusText(tr("Searching..."));
    }
}

void SkTerminalWidget::onSearchNext()
{
    QString text = m_searchBar != nullptr ? m_searchBar->searchText()
                                          : QString();
    if (text.isEmpty()) {
        return;
    }

    if (m_qtermWidget != nullptr) {
        m_qtermWidget->search(text, true, false);
    }
}

void SkTerminalWidget::onSearchPrev()
{
    QString text = m_searchBar != nullptr ? m_searchBar->searchText()
                                          : QString();
    if (text.isEmpty()) {
        return;
    }

    if (m_qtermWidget != nullptr) {
        m_qtermWidget->search(text, false, false);
    }
}

void SkTerminalWidget::onSearchClosed()
{
    if (m_searchBar != nullptr) {
        m_searchBar->hide();
        m_searchBar->clear();
    }

    if (m_qtermWidget != nullptr) {
        m_qtermWidget->setFocus();
    }
}

/* ------------------------------------------------------------------ */
/* C bridge implementation                                             */
/* ------------------------------------------------------------------ */

#include "shellkeep/sk_terminal_qt.h"

struct SkTerminalQtHandle
{
    SkTerminalWidget *widget;
    SkTerminalQtNewSessionCb newSessionCb;
    void *newSessionData;
};

SkTerminalQtHandle *sk_terminal_qt_new(void)
{
    auto *handle = new SkTerminalQtHandle();
    handle->widget = new SkTerminalWidget();
    handle->newSessionCb = nullptr;
    handle->newSessionData = nullptr;

    QObject::connect(handle->widget, &SkTerminalWidget::newSessionRequested,
                     [handle]() {
                         if (handle->newSessionCb != nullptr) {
                             handle->newSessionCb(handle->newSessionData);
                         }
                     });

    return handle;
}

void sk_terminal_qt_free(SkTerminalQtHandle *handle)
{
    if (handle == nullptr) {
        return;
    }
    delete handle->widget;
    delete handle;
}

bool sk_terminal_qt_connect_ssh(SkTerminalQtHandle *handle, int fd,
                                struct _SkSshChannel *channel)
{
    if (handle == nullptr || handle->widget == nullptr ||
        fd < 0 || channel == nullptr) {
        return false;
    }
    handle->widget->connectSsh(fd, channel);
    return true;
}

void sk_terminal_qt_disconnect(SkTerminalQtHandle *handle)
{
    if (handle != nullptr && handle->widget != nullptr) {
        handle->widget->disconnect();
    }
}

void sk_terminal_qt_feed(SkTerminalQtHandle *handle, const char *data,
                         int len)
{
    if (handle != nullptr && handle->widget != nullptr) {
        handle->widget->feed(data, len);
    }
}

void sk_terminal_qt_set_dead(SkTerminalQtHandle *handle,
                             const char *history_data, int history_len,
                             const char *message)
{
    if (handle == nullptr || handle->widget == nullptr) {
        return;
    }
    QString msg = message != nullptr ? QString::fromUtf8(message)
                                     : QObject::tr("This session was terminated on the server");
    handle->widget->setDead(history_data, history_len, msg);
}

void sk_terminal_qt_set_new_session_cb(SkTerminalQtHandle *handle,
                                       SkTerminalQtNewSessionCb cb,
                                       void *user_data)
{
    if (handle == nullptr) {
        return;
    }
    handle->newSessionCb = cb;
    handle->newSessionData = user_data;
}

void sk_terminal_qt_get_size(SkTerminalQtHandle *handle, int *cols,
                             int *rows)
{
    if (handle != nullptr && handle->widget != nullptr) {
        handle->widget->terminalSize(cols, rows);
    } else {
        if (cols != nullptr) *cols = 80;
        if (rows != nullptr) *rows = 24;
    }
}

void sk_terminal_qt_apply_theme(SkTerminalQtHandle *handle,
                                const struct _SkTheme *theme)
{
    if (handle == nullptr || handle->widget == nullptr || theme == nullptr) {
        return;
    }
    SkTerminalTheme qtTheme = SkTerminalTheme::fromSkTheme(theme);
    handle->widget->applyTheme(&qtTheme);
}
