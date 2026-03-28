// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file SkConnectFlow.cpp
 * @brief Qt wrapper around the C connect layer — implementation.
 *
 * Threading model:
 *   - start() is called on the main thread.  It copies parameters into
 *     stable storage, then calls sk_connect_start() which spawns its own
 *     GTask worker threads internally.  The done-callback fires on the
 *     GLib main context; we forward it to the Qt main thread via
 *     QMetaObject::invokeMethod (queued).
 *   - disconnect() is main-thread safe and delegates to
 *     sk_connect_disconnect() which is documented as safe to call from
 *     any thread.
 *
 * Requirements: FR-CONN-16..22
 */

#include "SkConnectFlow.h"

#include <QCoreApplication>
#include <QMetaObject>

#include "shellkeep/sk_log.h"

#ifndef _WIN32
#include <signal.h>
#include <unistd.h>
#else
#include <windows.h>
#endif

/* ------------------------------------------------------------------ */
/* Static members                                                      */
/* ------------------------------------------------------------------ */

std::atomic<SkConnectFlow *> SkConnectFlow::s_activeFlow{nullptr};

/* ------------------------------------------------------------------ */
/* Construction / destruction                                           */
/* ------------------------------------------------------------------ */

SkConnectFlow::SkConnectFlow(SkConfig *config, QObject *parent)
    : QObject(parent)
    , m_config(config)
{
}

SkConnectFlow::~SkConnectFlow()
{
    /* If we are the active flow for signal handling, clear the pointer
     * before tearing down so that the handler never touches freed memory. */
    SkConnectFlow *expected = this;
    s_activeFlow.compare_exchange_strong(expected, nullptr);

    if (m_ctx) {
        sk_connect_free(m_ctx);
        m_ctx = nullptr;
    }

    if (m_worker) {
        m_worker->quit();
        m_worker->wait();
        delete m_worker;
        m_worker = nullptr;
    }
}

/* ------------------------------------------------------------------ */
/* start()                                                              */
/* ------------------------------------------------------------------ */

void SkConnectFlow::start(const SkConnectFlowParams &params)
{
    QMutexLocker lock(&m_mutex);

    if (m_ctx) {
        SK_LOG_WARN(SK_LOG_COMPONENT_UI,
                    "SkConnectFlow::start called while already connected");
        return;
    }

    /* Convert QString to UTF-8 and keep the byte arrays alive for the
     * duration of the connection (sk_connect_start copies internally,
     * but we keep them just in case intermediate callbacks reference
     * the SkConnectParams pointers). */
    m_hostnameUtf8 = params.hostname.toUtf8();
    m_usernameUtf8 = params.username.toUtf8();
    m_identityUtf8 = params.identityFile.toUtf8();
    m_proxyUtf8    = params.proxyJump.toUtf8();

    SkConnectParams cparams;
    memset(&cparams, 0, sizeof(cparams));
    cparams.hostname      = m_hostnameUtf8.constData();
    cparams.port          = params.port;
    cparams.username      = params.username.isEmpty()
                                ? nullptr
                                : m_usernameUtf8.constData();
    cparams.identity_file = params.identityFile.isEmpty()
                                ? nullptr
                                : m_identityUtf8.constData();
    cparams.proxy_jump    = params.proxyJump.isEmpty()
                                ? nullptr
                                : m_proxyUtf8.constData();
    cparams.app           = nullptr; /* not used in Qt build */
    cparams.parent_window = nullptr; /* not used in Qt build */

    /* Emit initial phase */
    Q_EMIT phaseChanged(SK_BRIDGE_PHASE_CONNECTING);

    SK_LOG_INFO(SK_LOG_COMPONENT_UI, "starting connect flow: host=%s port=%d",
                cparams.hostname, cparams.port > 0 ? cparams.port : 22);

    m_ctx = sk_connect_start(&cparams, m_config, &SkConnectFlow::onConnectDone,
                             static_cast<gpointer>(this));

    if (!m_ctx) {
        SK_LOG_ERROR(SK_LOG_COMPONENT_UI, "sk_connect_start returned NULL");
        Q_EMIT error(tr("Failed to initiate connection"));
        return;
    }

    /* Register as the active flow for signal handling. */
    s_activeFlow.store(this);
}

/* ------------------------------------------------------------------ */
/* disconnect()                                                         */
/* ------------------------------------------------------------------ */

void SkConnectFlow::disconnect()
{
    QMutexLocker lock(&m_mutex);

    if (!m_ctx) {
        return;
    }

    SK_LOG_INFO(SK_LOG_COMPONENT_UI, "initiating graceful disconnect");

    sk_connect_disconnect(m_ctx);
    sk_connect_free(m_ctx);
    m_ctx = nullptr;

    /* Clear signal handler pointer. */
    SkConnectFlow *expected = this;
    s_activeFlow.compare_exchange_strong(expected, nullptr);

    lock.unlock();
    Q_EMIT disconnected();
}

/* ------------------------------------------------------------------ */
/* Queries                                                              */
/* ------------------------------------------------------------------ */

bool SkConnectFlow::isConnected() const
{
    QMutexLocker lock(&m_mutex);
    return m_ctx && sk_connect_is_connected(m_ctx);
}

QString SkConnectFlow::hostname() const
{
    QMutexLocker lock(&m_mutex);
    if (!m_ctx)
        return {};
    const char *h = sk_connect_get_hostname(m_ctx);
    return h ? QString::fromUtf8(h) : QString();
}

QString SkConnectFlow::environment() const
{
    QMutexLocker lock(&m_mutex);
    if (!m_ctx)
        return {};
    const char *e = sk_connect_get_environment(m_ctx);
    return e ? QString::fromUtf8(e) : QString();
}

QString SkConnectFlow::clientId() const
{
    QMutexLocker lock(&m_mutex);
    if (!m_ctx)
        return {};
    const char *c = sk_connect_get_client_id(m_ctx);
    return c ? QString::fromUtf8(c) : QString();
}

/* ------------------------------------------------------------------ */
/* C callback thunk                                                     */
/* ------------------------------------------------------------------ */

void SkConnectFlow::onConnectDone(SkConnectContext * /* ctx */, bool success,
                                  const GError *gerror, gpointer userData)
{
    auto *self = static_cast<SkConnectFlow *>(userData);

    if (success) {
        /* Dispatch to the Qt main thread */
        QMetaObject::invokeMethod(
            self,
            [self]() {
                Q_EMIT self->phaseChanged(SK_BRIDGE_PHASE_DONE);
                Q_EMIT self->connected();
            },
            Qt::QueuedConnection);
    } else {
        QString msg;
        if (gerror && gerror->message) {
            msg = QString::fromUtf8(gerror->message);
        } else {
            msg = QCoreApplication::translate("SkConnectFlow",
                                              "Connection failed (unknown error)");
        }

        QMetaObject::invokeMethod(
            self,
            [self, msg]() {
                Q_EMIT self->phaseChanged(SK_BRIDGE_PHASE_ERROR);
                Q_EMIT self->error(msg);
            },
            Qt::QueuedConnection);
    }
}

/* ------------------------------------------------------------------ */
/* Signal / ctrl handlers                                               */
/* ------------------------------------------------------------------ */

#ifndef _WIN32

/*
 * Unix signal handler for SIGTERM / SIGINT.
 *
 * This runs in signal context, so we can only call async-signal-safe
 * functions.  sk_connect_emergency_shutdown() is designed for this:
 * it does best-effort state save and lock release synchronously.
 */
void sk_qt_signal_handler(int signum)
{
    /* Best-effort emergency shutdown. */
    SkConnectFlow *flow = SkConnectFlow::s_activeFlow.load(std::memory_order_relaxed);
    if (flow) {
        SkConnectContext *ctx = flow->context();
        if (ctx) {
            sk_connect_emergency_shutdown(ctx);
        }
    }

    /* Re-raise with default handler to get the expected exit status. */
    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sa.sa_handler = SIG_DFL;
    sigemptyset(&sa.sa_mask);
    sigaction(signum, &sa, nullptr);
    raise(signum);
}

void SkConnectFlow::installSignalHandlers()
{
    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sa.sa_handler = sk_qt_signal_handler;
    sigemptyset(&sa.sa_mask);
    sa.sa_flags = SA_RESETHAND; /* one-shot: restore default after first fire */

    sigaction(SIGTERM, &sa, nullptr);
    sigaction(SIGINT, &sa, nullptr);
}

#else /* _WIN32 */

static BOOL WINAPI sk_qt_console_handler(DWORD ctrlType)
{
    if (ctrlType == CTRL_C_EVENT || ctrlType == CTRL_BREAK_EVENT ||
        ctrlType == CTRL_CLOSE_EVENT) {
        SkConnectFlow *flow =
            SkConnectFlow::s_activeFlow.load(std::memory_order_relaxed);
        if (flow) {
            SkConnectContext *ctx = flow->context();
            if (ctx) {
                sk_connect_emergency_shutdown(ctx);
            }
        }
        return FALSE; /* let the default handler run after us */
    }
    return FALSE;
}

void SkConnectFlow::installSignalHandlers()
{
    SetConsoleCtrlHandler(sk_qt_console_handler, TRUE);
}

#endif /* _WIN32 */
