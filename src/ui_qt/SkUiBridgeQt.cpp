// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#include "SkUiBridgeQt.h"

#include "SkConnFeedback.h"
#include "SkDialogs.h"
#include "SkMainWindow.h"
#include "SkStyleSheet.h"
#include "SkToast.h"
#include "SkTrayIcon.h"
#include "SkWelcomeWidget.h"
#include "terminal_qt/SkTerminalWidget.h"

#include <QDialog>
#include <QEventLoop>
#include <QMutex>
#include <QMutexLocker>
#include <QThread>
#include <QPlainTextEdit>
#include <QTextCursor>
#include <QMetaObject>
#include <QTimer>
#include <QVBoxLayout>
#include <QWaitCondition>

#include <cstring>

/* Helper: run a lambda on the Qt main thread, blocking the caller.
 * Uses QTimer::singleShot(0) + QMutex/QWaitCondition to avoid
 * Qt 6.4's aggressive BlockingQueuedConnection deadlock detection. */
template <typename F>
static void runOnMainThread(F &&func)
{
    if (QThread::currentThread() == qApp->thread()) {
        func();
        return;
    }
    QMutex mutex;
    QWaitCondition cond;
    bool done = false;
    mutex.lock();
    QTimer::singleShot(0, qApp, [&]() {
        func();
        QMutexLocker lk(&mutex);
        done = true;
        cond.wakeOne();
    });
    while (!done)
        cond.wait(&mutex);
    mutex.unlock();
}

/* ================================================================== */
/* SkUiBridgeQt singleton                                              */
/* ================================================================== */

SkUiBridgeQt *SkUiBridgeQt::s_instance = nullptr;

SkUiBridgeQt::SkUiBridgeQt(QApplication *app, QObject *parent)
    : QObject(parent), m_app(app)
{
    Q_ASSERT(!s_instance);
    s_instance = this;
    populateVtable();
}

SkUiBridgeQt::~SkUiBridgeQt()
{
    delete m_trayIcon;
    m_trayIcon = nullptr;
    /* Windows are owned by Qt parent hierarchy or explicitly managed */
    s_instance = nullptr;
}

SkUiBridgeQt *SkUiBridgeQt::instance()
{
    return s_instance;
}

SkMainWindow *SkUiBridgeQt::primaryWindow()
{
    if (!m_primaryWindow) {
        m_primaryWindow = new SkMainWindow();
        m_primaryWindow->setStyleSheet(SkStyleSheet::get());
        registerWindow(static_cast<SkBridgeWindow>(m_primaryWindow), m_primaryWindow);
    }
    return m_primaryWindow;
}

SkTrayIcon *SkUiBridgeQt::trayIcon()
{
    if (!m_trayIcon) {
        m_trayIcon = new SkTrayIcon(this);
        connect(m_trayIcon, &SkTrayIcon::showWindowRequested, this, [this]() {
            auto *win = primaryWindow();
            if (win->isVisible()) {
                win->hide();
            } else {
                win->show();
                win->raise();
                win->activateWindow();
            }
        });
        connect(m_trayIcon, &SkTrayIcon::quitRequested, m_app, &QApplication::quit);
    }
    return m_trayIcon;
}

void SkUiBridgeQt::registerWindow(SkBridgeWindow handle, SkMainWindow *window)
{
    m_windows.insert(handle, window);
}

void SkUiBridgeQt::unregisterWindow(SkBridgeWindow handle)
{
    m_windows.remove(handle);
}

SkMainWindow *SkUiBridgeQt::lookupWindow(SkBridgeWindow handle) const
{
    return m_windows.value(handle, nullptr);
}

void SkUiBridgeQt::populateVtable()
{
    memset(&m_bridge, 0, sizeof(m_bridge));

    /* Dialogs */
    m_bridge.host_key_unknown    = sk_qt_host_key_unknown;
    m_bridge.host_key_changed    = sk_qt_host_key_changed;
    m_bridge.auth_password       = sk_qt_auth_password;
    m_bridge.auth_mfa            = sk_qt_auth_mfa;
    m_bridge.auth_passphrase     = sk_qt_auth_passphrase;
    m_bridge.conflict_dialog     = sk_qt_conflict_dialog;
    m_bridge.environment_select  = sk_qt_environment_select;
    m_bridge.close_dialog        = sk_qt_close_dialog;
    m_bridge.error_dialog        = sk_qt_error_dialog;
    m_bridge.info_dialog         = sk_qt_info_dialog;

    /* Feedback */
    m_bridge.feedback_create       = sk_qt_feedback_create;
    m_bridge.feedback_set_phase    = sk_qt_feedback_set_phase;
    m_bridge.feedback_set_progress = sk_qt_feedback_set_progress;
    m_bridge.feedback_set_error    = sk_qt_feedback_set_error;
    m_bridge.feedback_free         = sk_qt_feedback_free;

    /* Window/tab */
    m_bridge.window_new     = sk_qt_window_new;
    m_bridge.window_show    = sk_qt_window_show;
    m_bridge.window_free    = sk_qt_window_free;
    m_bridge.terminal_new   = sk_qt_terminal_new;
    m_bridge.terminal_connect    = sk_qt_terminal_connect;
    m_bridge.terminal_disconnect = sk_qt_terminal_disconnect;
    m_bridge.terminal_feed       = sk_qt_terminal_feed;
    m_bridge.terminal_set_dead   = sk_qt_terminal_set_dead;
    m_bridge.terminal_free       = sk_qt_terminal_free;
    m_bridge.window_add_tab      = sk_qt_window_add_tab;
    m_bridge.tab_set_indicator   = sk_qt_tab_set_indicator;
    m_bridge.tab_set_dead        = sk_qt_tab_set_dead;

    /* Toast */
    m_bridge.toast_show = sk_qt_toast_show;

    /* Welcome */
    m_bridge.welcome_show = sk_qt_welcome_show;
}

/* ================================================================== */
/* Helper: get bridge instance from SkUiHandle                         */
/* ================================================================== */

static SkUiBridgeQt *bridgeFromHandle(SkUiHandle /*ui*/)
{
    return SkUiBridgeQt::instance();
}

static SkMainWindow *windowFromHandle(SkUiHandle ui)
{
    auto *bridge = bridgeFromHandle(ui);
    return bridge ? bridge->primaryWindow() : nullptr;
}

/* ================================================================== */
/* extern "C" vtable wrappers                                          */
/* ================================================================== */

extern "C" {

/* -- Dialogs -------------------------------------------------------- */

SkBridgeHostKeyResult sk_qt_host_key_unknown(SkUiHandle ui, const char *hostname,
                                              const char *fingerprint, const char *key_type)
{
    /* --accept-unknown-hosts: auto-accept for dev/testing */
    auto *inst = SkUiBridgeQt::instance();
    if (inst && inst->autoAcceptHosts())
        return SK_BRIDGE_HOST_KEY_ACCEPT_SAVE;

    auto *win = windowFromHandle(ui);
    return SkDialogs::hostKeyUnknown(
        win,
        QString::fromUtf8(hostname),
        QString::fromUtf8(fingerprint),
        QString::fromUtf8(key_type));
}

void sk_qt_host_key_changed(SkUiHandle ui, const char *hostname, const char *old_fp,
                             const char *new_fp, const char *key_type)
{
    auto *win = windowFromHandle(ui);
    SkDialogs::hostKeyChanged(
        win,
        QString::fromUtf8(hostname),
        QString::fromUtf8(old_fp),
        QString::fromUtf8(new_fp),
        QString::fromUtf8(key_type));
}

char *sk_qt_auth_password(SkUiHandle ui, const char *prompt)
{
    /* --password: auto-fill for dev/testing */
    auto *inst = SkUiBridgeQt::instance();
    if (inst && !inst->autoPassword().isEmpty()) {
        QByteArray utf8 = inst->autoPassword().toUtf8();
        char *copy = static_cast<char *>(g_malloc(utf8.size() + 1));
        memcpy(copy, utf8.constData(), utf8.size() + 1);
        return copy;
    }

    auto *win = windowFromHandle(ui);
    QString result = SkDialogs::authPassword(win, QString::fromUtf8(prompt));
    if (result.isEmpty())
        return nullptr;
    QByteArray utf8 = result.toUtf8();
    char *copy = static_cast<char *>(g_malloc(utf8.size() + 1));
    memcpy(copy, utf8.constData(), utf8.size() + 1);
    return copy;
}

char **sk_qt_auth_mfa(SkUiHandle ui, const char *name, const char *instruction,
                       const char **prompts, const gboolean *show_input, int n_prompts)
{
    auto *win = windowFromHandle(ui);

    QStringList promptList;
    QList<bool> showList;
    for (int i = 0; i < n_prompts; ++i) {
        promptList.append(QString::fromUtf8(prompts[i]));
        showList.append(show_input[i] != FALSE);
    }

    QStringList results = SkDialogs::authMfa(
        win,
        QString::fromUtf8(name),
        QString::fromUtf8(instruction),
        promptList, showList);

    if (results.isEmpty())
        return nullptr;

    auto **arr = static_cast<char **>(g_malloc0(sizeof(char *) * (results.size() + 1)));
    for (int i = 0; i < results.size(); ++i) {
        QByteArray utf8 = results[i].toUtf8();
        arr[i] = static_cast<char *>(g_malloc(utf8.size() + 1));
        memcpy(arr[i], utf8.constData(), utf8.size() + 1);
    }
    return arr;
}

char *sk_qt_auth_passphrase(SkUiHandle ui, const char *key_path)
{
    auto *win = windowFromHandle(ui);
    QString result = SkDialogs::authPassphrase(win, QString::fromUtf8(key_path));
    if (result.isEmpty())
        return nullptr;
    QByteArray utf8 = result.toUtf8();
    char *copy = static_cast<char *>(g_malloc(utf8.size() + 1));
    memcpy(copy, utf8.constData(), utf8.size() + 1);
    return copy;
}

bool sk_qt_conflict_dialog(SkUiHandle ui, const char *hostname, const char *connected_at)
{
    auto *win = windowFromHandle(ui);
    return SkDialogs::conflictDialog(
        win,
        QString::fromUtf8(hostname),
        QString::fromUtf8(connected_at));
}

char *sk_qt_environment_select(SkUiHandle ui, const char **envs, int n_envs,
                                const char *last_env)
{
    auto *win = windowFromHandle(ui);
    QStringList envList;
    for (int i = 0; i < n_envs; ++i) {
        envList.append(QString::fromUtf8(envs[i]));
    }
    QString result = SkDialogs::environmentSelect(
        win, envList, QString::fromUtf8(last_env ? last_env : ""));
    if (result.isEmpty())
        return nullptr;
    QByteArray utf8 = result.toUtf8();
    char *copy = static_cast<char *>(g_malloc(utf8.size() + 1));
    memcpy(copy, utf8.constData(), utf8.size() + 1);
    return copy;
}

SkBridgeCloseResult sk_qt_close_dialog(SkUiHandle ui, int n_active)
{
    auto *win = windowFromHandle(ui);
    return SkDialogs::closeWindow(win, n_active);
}

void sk_qt_error_dialog(SkUiHandle ui, const char *title, const char *message)
{
    auto *win = windowFromHandle(ui);
    SkDialogs::errorDialog(win, QString::fromUtf8(title), QString::fromUtf8(message));
}

void sk_qt_info_dialog(SkUiHandle ui, const char *title, const char *message)
{
    auto *win = windowFromHandle(ui);
    SkDialogs::infoDialog(win, QString::fromUtf8(title), QString::fromUtf8(message));
}

/* -- Feedback ------------------------------------------------------- */

void *sk_qt_feedback_create(SkUiHandle ui)
{
    auto *win = windowFromHandle(ui);
    if (!win)
        return nullptr;
    auto *fb = new SkConnFeedback(win);
    return static_cast<void *>(fb);
}

void sk_qt_feedback_set_phase(void *feedback, SkBridgeConnPhase phase)
{
    if (!feedback)
        return;
    auto *fb = static_cast<SkConnFeedback *>(feedback);
    fb->setPhase(phase);
}

void sk_qt_feedback_set_progress(void *feedback, int current, int total)
{
    if (!feedback)
        return;
    auto *fb = static_cast<SkConnFeedback *>(feedback);
    fb->setProgress(current, total);
}

void sk_qt_feedback_set_error(void *feedback, const char *message)
{
    if (!feedback)
        return;
    auto *fb = static_cast<SkConnFeedback *>(feedback);
    fb->setError(QString::fromUtf8(message));
}

void sk_qt_feedback_free(void *feedback)
{
    if (!feedback)
        return;
    auto *fb = static_cast<SkConnFeedback *>(feedback);
    fb->hide();
    fb->deleteLater();
}

/* -- Window/tab ----------------------------------------------------- */

SkBridgeWindow sk_qt_window_new(SkUiHandle ui, const char *title, int x, int y,
                                 int width, int height)
{
    auto *bridge = bridgeFromHandle(ui);
    if (!bridge)
        return nullptr;

    /* Reuse the primary window for the first window request.
     * This ensures tabs from the connect flow land in the same
     * window that main() created and showed. */
    auto *primary = bridge->primaryWindow();
    auto primaryHandle = static_cast<SkBridgeWindow>(primary);
    if (!bridge->lookupWindow(primaryHandle) || bridge->lookupWindow(primaryHandle)->tabCount() == 0) {
        /* Primary window has no tabs yet — reuse it. */
        if (title)
            primary->setWindowTitle(QString::fromUtf8(title));
        if (width > 0 && height > 0)
            primary->resize(width, height);
        if (x >= 0 && y >= 0)
            primary->move(x, y);
        bridge->registerWindow(primaryHandle, primary);
        return primaryHandle;
    }

    /* Primary already in use — create additional window. */
    auto *win = new SkMainWindow(
        title ? QString::fromUtf8(title) : QString(),
        x, y, width, height);
    win->setStyleSheet(SkStyleSheet::get());

    auto handle = static_cast<SkBridgeWindow>(win);
    bridge->registerWindow(handle, win);
    return handle;
}

void sk_qt_window_show(SkBridgeWindow win)
{
    auto *bridge = SkUiBridgeQt::instance();
    if (!bridge)
        return;
    auto *window = bridge->lookupWindow(win);
    if (window) {
        window->show();
        window->raise();
        window->activateWindow();
    }
}

void sk_qt_window_free(SkBridgeWindow win)
{
    auto *bridge = SkUiBridgeQt::instance();
    if (!bridge)
        return;
    auto *window = bridge->lookupWindow(win);
    if (window) {
        bridge->unregisterWindow(win);
        window->deleteLater();
    }
}

SkBridgeTerminal sk_qt_terminal_new(SkUiHandle /*ui*/, const char * /*font_family*/,
                                     int /*font_size*/, int /*scrollback_lines*/)
{
    QWidget *result = nullptr;
    runOnMainThread([&result]() {
        result = new SkTerminalWidget();
    });
    return static_cast<SkBridgeTerminal>(result);
}

bool sk_qt_terminal_connect(SkBridgeTerminal term, int ssh_fd,
                             SkSshChannel *channel)
{
    if (!term)
        return false;
    auto *tw = qobject_cast<SkTerminalWidget *>(static_cast<QWidget *>(term));
    if (!tw)
        return false;
    bool ok = false;
    runOnMainThread([tw, ssh_fd, channel, &ok]() {
        tw->connectSsh(ssh_fd, channel);
        ok = true;
    });
    return ok;
}

void sk_qt_terminal_disconnect(SkBridgeTerminal term)
{
    if (!term)
        return;
    auto *tw = qobject_cast<SkTerminalWidget *>(static_cast<QWidget *>(term));
    if (!tw)
        return;
    runOnMainThread([tw]() { tw->disconnect(); });
}

void sk_qt_terminal_feed(SkBridgeTerminal term, const char *buf, int len)
{
    if (!term || !buf)
        return;
    auto *tw = qobject_cast<SkTerminalWidget *>(static_cast<QWidget *>(term));
    if (!tw)
        return;
    runOnMainThread([tw, buf, len]() { tw->feed(buf, len); });
}

void sk_qt_terminal_set_dead(SkBridgeTerminal term, const char *history_data,
                              int history_len, const char *message)
{
    if (!term)
        return;
    auto *tw = qobject_cast<SkTerminalWidget *>(static_cast<QWidget *>(term));
    if (!tw)
        return;
    runOnMainThread([tw, history_data, history_len, message]() {
        tw->setDead(history_data, history_len,
                    message ? QString::fromUtf8(message) : QString());
    });
}

void sk_qt_terminal_free(SkBridgeTerminal term)
{
    if (!term)
        return;
    auto *widget = static_cast<QWidget *>(term);
    widget->deleteLater();
}

SkBridgeTab sk_qt_window_add_tab(SkBridgeWindow win, SkBridgeTerminal term, const char *title)
{
    auto *bridge = SkUiBridgeQt::instance();
    if (!bridge)
        return nullptr;
    auto *window = bridge->lookupWindow(win);
    if (!window)
        return nullptr;
    auto *widget = static_cast<QWidget *>(term);
    QString tabTitle = QString::fromUtf8(title ? title : "Terminal");

    /* Must add tab on Qt main thread */
    runOnMainThread([window, widget, tabTitle]() {
        window->addTab(widget, tabTitle);
    });

    return static_cast<SkBridgeTab>(widget);
}

void sk_qt_tab_set_indicator(SkBridgeTab tab, SkBridgeConnIndicator indicator)
{
    if (!tab)
        return;
    auto *bridge = SkUiBridgeQt::instance();
    if (!bridge)
        return;

    /* Find the tab index by widget pointer */
    auto *widget = static_cast<QWidget *>(tab);
    auto *win = bridge->primaryWindow();
    if (!win)
        return;

    for (int i = 0; i < win->tabCount(); ++i) {
        if (win->tabWidget(i) == widget) {
            win->setTabIndicator(i, indicator);
            break;
        }
    }
}

void sk_qt_tab_set_dead(SkBridgeTab tab, bool dead)
{
    if (!tab)
        return;
    auto *bridge = SkUiBridgeQt::instance();
    if (!bridge)
        return;

    auto *widget = static_cast<QWidget *>(tab);
    auto *win = bridge->primaryWindow();
    if (!win)
        return;

    for (int i = 0; i < win->tabCount(); ++i) {
        if (win->tabWidget(i) == widget) {
            win->setTabDead(i, dead);
            break;
        }
    }
}

/* -- Toast ---------------------------------------------------------- */

void sk_qt_toast_show(SkUiHandle ui, const char *message, int timeout_ms)
{
    auto *win = windowFromHandle(ui);
    if (!win)
        return;
    SkToast::show(win, QString::fromUtf8(message), timeout_ms);
}

/* -- Welcome -------------------------------------------------------- */

bool sk_qt_welcome_show(SkUiHandle ui, const char **recent, int n_recent, bool first_use,
                         char **out_host, char **out_user, int *out_port)
{
    auto *win = windowFromHandle(ui);

    QStringList recentList;
    for (int i = 0; i < n_recent; ++i) {
        recentList.append(QString::fromUtf8(recent[i]));
    }

    /* Create a modal dialog containing the welcome widget */
    QDialog dlg(win);
    dlg.setWindowTitle(QStringLiteral("shellkeep"));
    dlg.setStyleSheet(SkStyleSheet::get());
    dlg.setMinimumSize(500, 400);

    auto *layout = new QVBoxLayout(&dlg);
    auto *welcome = new SkWelcomeWidget(&dlg);
    welcome->setRecentConnections(recentList);
    welcome->setFirstUse(first_use);
    layout->addWidget(welcome);

    QString host, user;
    int port = 0;
    bool connected = false;

    QObject::connect(welcome, &SkWelcomeWidget::connectRequested,
                     &dlg, [&](const QString &h, const QString &u, int p) {
                         host = h;
                         user = u;
                         port = p;
                         connected = true;
                         dlg.accept();
                     });

    dlg.exec();

    if (!connected || host.isEmpty())
        return false;

    /* Allocate output strings with g_malloc for C backend compatibility */
    QByteArray hostUtf8 = host.toUtf8();
    *out_host = static_cast<char *>(g_malloc(hostUtf8.size() + 1));
    memcpy(*out_host, hostUtf8.constData(), hostUtf8.size() + 1);

    if (!user.isEmpty()) {
        QByteArray userUtf8 = user.toUtf8();
        *out_user = static_cast<char *>(g_malloc(userUtf8.size() + 1));
        memcpy(*out_user, userUtf8.constData(), userUtf8.size() + 1);
    } else {
        *out_user = nullptr;
    }

    *out_port = port;
    return true;
}

} /* extern "C" */
