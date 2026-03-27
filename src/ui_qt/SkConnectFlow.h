// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file SkConnectFlow.h
 * @brief Qt wrapper around the C connect layer (sk_connect_*).
 *
 * Runs the end-to-end connection flow on a worker thread and exposes
 * progress / result as Qt signals so the UI never blocks.
 *
 * Requirements: FR-CONN-16..22
 */

#ifndef SK_CONNECT_FLOW_H
#define SK_CONNECT_FLOW_H

#include <QObject>
#include <QString>
#include <QThread>
#include <QMutex>

#include <atomic>

extern "C" {
#include "shellkeep/sk_config.h"
#include "shellkeep/sk_connect.h"
#include "shellkeep/sk_types.h"
#include "shellkeep/sk_ui_bridge.h"
} /* extern "C" */

/**
 * Parameters for starting a connection flow from the Qt side.
 * Mirrors SkConnectParams but without GTK types.
 */
struct SkConnectFlowParams
{
    QString hostname;
    int     port         = 0;
    QString username;
    QString identityFile;
    QString proxyJump;
};

/**
 * Qt wrapper around the C connection lifecycle.
 *
 * Usage:
 *   auto *flow = new SkConnectFlow(config, this);
 *   connect(flow, &SkConnectFlow::connected, ...);
 *   connect(flow, &SkConnectFlow::error, ...);
 *   flow->start(params);
 *
 * Thread safety: start() and disconnect() may be called from the main
 * thread.  All signals are emitted on the main thread via queued
 * connections.
 */
class SkConnectFlow : public QObject
{
    Q_OBJECT

public:
    /**
     * Construct a connect flow wrapper.
     *
     * @param config  Application configuration (must outlive this object).
     * @param parent  QObject parent for ownership.
     */
    explicit SkConnectFlow(SkConfig *config, QObject *parent = nullptr);
    ~SkConnectFlow() override;

    /* -- Control -------------------------------------------------------- */

    /**
     * Start the connection flow asynchronously.
     * Must be called from the main thread.
     * Emits phaseChanged / progressChanged as the flow advances, then
     * either connected() or error() on completion.
     */
    void start(const SkConnectFlowParams &params);

    /**
     * Initiate graceful disconnect.
     * Safe to call from any thread; the actual work runs asynchronously.
     * Emits disconnected() when done.
     */
    void disconnect();

    /* -- Queries -------------------------------------------------------- */

    /** True if a connection is currently established. */
    bool isConnected() const;

    /** Hostname of the current/last connection (empty if never started). */
    QString hostname() const;

    /** Active environment name (empty if none selected). */
    QString environment() const;

    /** Client-ID used for this connection. */
    QString clientId() const;

    /** Raw C context (for advanced interop). */
    SkConnectContext *context() const { return m_ctx; }

    /* -- Signal handler (Unix) / ctrl handler (Windows) ---------------- */

    /**
     * Install process-level signal/ctrl handlers that trigger emergency
     * shutdown of the active connection context.
     *
     * Call once at application startup after the first SkConnectFlow is
     * created.  On Unix this installs SIGTERM/SIGINT handlers; on
     * Windows it calls SetConsoleCtrlHandler.
     */
    static void installSignalHandlers();

Q_SIGNALS:
    /** The connection flow completed successfully. */
    void connected();

    /** The connection was closed (graceful disconnect). */
    void disconnected();

    /**
     * The connection flow failed.
     * @param message  Human-readable error description.
     */
    void error(const QString &message);

    /**
     * The connection flow entered a new phase.
     * @param phase  One of SkBridgeConnPhase values.
     */
    void phaseChanged(int phase);

    /**
     * Session restoration progress update.
     * @param current  Number of sessions restored so far.
     * @param total    Total sessions to restore.
     */
    void progressChanged(int current, int total);

private:
    /* C-callback thunks (static, forwarded via queued invocations) */
    static void onConnectDone(SkConnectContext *ctx, bool success,
                              const GError *gerror, gpointer userData);

    /* Prevent copies */
    SkConnectFlow(const SkConnectFlow &) = delete;
    SkConnectFlow &operator=(const SkConnectFlow &) = delete;

    SkConfig          *m_config = nullptr;
    SkConnectContext  *m_ctx    = nullptr;
    QThread           *m_worker = nullptr;
    mutable QMutex     m_mutex;

    /* Keep a copy of params for the lifetime of the flow so that the
     * C layer (which receives const char* pointers) always has valid
     * backing storage. */
    QByteArray m_hostnameUtf8;
    QByteArray m_usernameUtf8;
    QByteArray m_identityUtf8;
    QByteArray m_proxyUtf8;

    /* Global instance pointer for signal handlers. */
    static std::atomic<SkConnectFlow *> s_activeFlow;
    friend void sk_qt_signal_handler(int);
};

#endif /* SK_CONNECT_FLOW_H */
