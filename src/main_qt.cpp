// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file main_qt.cpp
 * @brief Qt6 application entry point for shellkeep.
 *
 * Replaces src/main.c for the Qt6 build.  Provides:
 *   - QCommandLineParser for ssh-like arguments (FR-CLI-01, FR-CLI-03)
 *   - Single-instance via QLockFile + QLocalServer (FR-CLI-04, FR-CLI-05)
 *   - Logging, crash handler, permissions initialisation
 *   - GLib event loop integration
 *   - Platform-specific hardening (prctl, app bundle, console)
 *
 * Requirements: FR-CLI-01..05, NFR-SEC-03, NFR-SEC-10, NFR-I18N-06,
 *               NFR-ARCH-09
 */

#include "ui_qt/SkConnFeedback.h"
#include "ui_qt/SkConnectFlow.h"
#include "ui_qt/SkMainWindow.h"
#include "ui_qt/SkStyleSheet.h"
#include "ui_qt/SkTrayIcon.h"
#include "ui_qt/SkUiBridgeQt.h"
#include "ui_qt/SkWelcomeWidget.h"

#include <QApplication>
#include <QCommandLineParser>
#include <QDir>
#include <QLibraryInfo>
#include <QLocalServer>
#include <QLocalSocket>
#include <QPointer>
#include <QLockFile>
#include <QStandardPaths>
#include <QTimer>
#include <QTranslator>

#include <clocale>
#include <cstdlib>
#include <cstring>
#include <memory>

extern "C" {
#include "shellkeep/sk_config.h"
#include "shellkeep/sk_log.h"
#include "shellkeep/sk_state.h"
#include "shellkeep/sk_types.h"
#include "shellkeep/sk_ui_bridge.h"
} /* extern "C" */

/* Platform headers */
#ifdef Q_OS_LINUX
#ifdef HAVE_PRCTL
#include <sys/prctl.h>
#endif
#endif

#ifdef Q_OS_WIN
#include <windows.h>
#endif

/* ------------------------------------------------------------------ */
/* GLib event loop integration                                          */
/* ------------------------------------------------------------------ */

#include <QThread>

/**
 * Worker thread that pumps the GLib default main context.
 *
 * The backend's GTask callbacks, g_idle_add(), and GLib signal sources
 * all require a running GMainContext.  On Linux, Qt's QEventDispatcherGlib
 * theoretically integrates GLib, but in practice GTask completions are
 * not reliably dispatched without explicit iteration.  So we run this
 * thread on ALL platforms for consistent behavior.
 */
class GLibEventThread : public QThread
{
    Q_OBJECT

public:
    explicit GLibEventThread(QObject *parent = nullptr)
        : QThread(parent)
    {
    }

    void requestStop() { m_running.store(false); }

protected:
    void run() override
    {
        GMainContext *ctx = g_main_context_default();
        m_running.store(true);

        while (m_running.load()) {
            /* Non-blocking iteration — may_block=FALSE avoids holding
             * the GLib context lock, preventing deadlock when GTask
             * workers call BlockingQueuedConnection back to Qt. */
            g_main_context_iteration(ctx, FALSE);
            QThread::msleep(2);
        }
    }

private:
    std::atomic<bool> m_running{false};
};

/* ------------------------------------------------------------------ */
/* Forward declarations                                                 */
/* ------------------------------------------------------------------ */

static void parseHostString(const QString &hostStr, QString &outUser,
                            QString &outHost);
static bool trySendToRunningInstance(const QString &serverName,
                                     const QStringList &args);
static QString lockFilePath();
static QString localServerName();

/* ------------------------------------------------------------------ */
/* Entry point                                                          */
/* ------------------------------------------------------------------ */

int main(int argc, char *argv[])
{
    /* -------------------------------------------------------------- */
    /* Platform hardening (before anything else)                        */
    /* -------------------------------------------------------------- */
#ifdef Q_OS_LINUX
#ifdef HAVE_PRCTL
    /* NFR-SEC-10: disable core dumps to prevent leaking sensitive
     * memory (passwords, keys). */
    prctl(PR_SET_DUMPABLE, 0);
#endif
#endif

#ifdef Q_OS_WIN
    /* On Windows, attach to the parent console if launched from cmd so
     * that --version / --help output is visible. */
    if (AttachConsole(ATTACH_PARENT_PROCESS)) {
        (void)freopen("CONOUT$", "w", stdout);
        (void)freopen("CONOUT$", "w", stderr);
    }
#endif

    /* -------------------------------------------------------------- */
    /* QApplication                                                     */
    /* -------------------------------------------------------------- */

    QApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("shellkeep"));
    app.setApplicationVersion(QStringLiteral(SK_VERSION_STRING));
    app.setOrganizationDomain(QStringLiteral("shellkeep.org"));
    app.setDesktopFileName(QStringLiteral(SK_APPLICATION_ID));

#ifdef Q_OS_MACOS
    /* macOS app bundle: ensure the working directory is sensible. */
    QDir::setCurrent(QDir::homePath());
#endif

    /* -------------------------------------------------------------- */
    /* i18n — NFR-I18N-06                                               */
    /* -------------------------------------------------------------- */

    std::setlocale(LC_ALL, "");

    QTranslator translator;
    if (translator.load(QLocale(), QStringLiteral("shellkeep"),
                        QStringLiteral("_"),
                        QStringLiteral(":/translations"))) {
        app.installTranslator(&translator);
    }

    /* gettext for C backend strings */
#ifdef Q_OS_LINUX
    bindtextdomain("shellkeep", LOCALEDIR);
    bind_textdomain_codeset("shellkeep", "UTF-8");
    textdomain("shellkeep");
#endif

    /* -------------------------------------------------------------- */
    /* Command-line parsing — FR-CLI-01, FR-CLI-03                      */
    /* -------------------------------------------------------------- */

    QCommandLineParser parser;
    parser.setApplicationDescription(
        QCoreApplication::translate("main", "SSH terminal manager"));
    parser.addHelpOption();
    parser.addVersionOption();

    /* [user@]host positional argument */
    parser.addPositionalArgument(
        QStringLiteral("host"),
        QCoreApplication::translate("main",
                                    "Remote host in [user@]host format"),
        QStringLiteral("[user@]host"));

    QCommandLineOption portOpt(
        {QStringLiteral("p"), QStringLiteral("port")},
        QCoreApplication::translate("main", "SSH port"),
        QStringLiteral("PORT"), QStringLiteral("0"));
    parser.addOption(portOpt);

    QCommandLineOption identityOpt(
        {QStringLiteral("i"), QStringLiteral("identity")},
        QCoreApplication::translate("main", "Identity file (private key)"),
        QStringLiteral("FILE"));
    parser.addOption(identityOpt);

    QCommandLineOption loginOpt(
        {QStringLiteral("l"), QStringLiteral("login")},
        QCoreApplication::translate("main", "Login user name"),
        QStringLiteral("USER"));
    parser.addOption(loginOpt);

    QCommandLineOption debugOpt(
        QStringLiteral("debug"),
        QCoreApplication::translate(
            "main", "Enable debug logging [for COMPONENT]"),
        QStringLiteral("COMPONENT"));
    parser.addOption(debugOpt);

    QCommandLineOption traceOpt(
        QStringLiteral("trace"),
        QCoreApplication::translate("main", "Enable trace logging"));
    parser.addOption(traceOpt);

    QCommandLineOption configOpt(
        QStringLiteral("config"),
        QCoreApplication::translate("main", "Configuration file path"),
        QStringLiteral("PATH"));
    parser.addOption(configOpt);

    QCommandLineOption minimizedOpt(
        QStringLiteral("minimized"),
        QCoreApplication::translate("main",
                                    "Start minimized to system tray"));
    parser.addOption(minimizedOpt);

    QCommandLineOption crashReportOpt(
        QStringLiteral("crash-report"),
        QCoreApplication::translate(
            "main", "Show crash report from previous run"));
    parser.addOption(crashReportOpt);

    QCommandLineOption acceptHostsOpt(
        QStringLiteral("accept-unknown-hosts"),
        QCoreApplication::translate(
            "main", "Auto-accept unknown host keys (dev/testing only)"));
    parser.addOption(acceptHostsOpt);

    QCommandLineOption autoPasswordOpt(
        QStringLiteral("password"),
        QCoreApplication::translate(
            "main", "Auto-fill password (dev/testing only)"),
        QStringLiteral("PASS"));
    parser.addOption(autoPasswordOpt);

    parser.process(app);

    /* Extract values */
    const bool debugMode      = parser.isSet(debugOpt);
    const bool traceMode      = parser.isSet(traceOpt);
    const QString debugComp   = parser.value(debugOpt);
    const QString configPath  = parser.value(configOpt);
    const bool minimized      = parser.isSet(minimizedOpt);
    const bool crashReport    = parser.isSet(crashReportOpt);
    const int port            = parser.value(portOpt).toInt();
    const QString identityFile = parser.value(identityOpt);
    QString loginUser         = parser.value(loginOpt);

    /* Parse positional [user@]host */
    QString hostArg;
    QString hostUser;
    const QStringList positional = parser.positionalArguments();
    if (!positional.isEmpty()) {
        parseHostString(positional.first(), hostUser, hostArg);
    }

    /* -l flag takes precedence over user@host */
    if (loginUser.isEmpty() && !hostUser.isEmpty()) {
        loginUser = hostUser;
    }

    /* -------------------------------------------------------------- */
    /* --crash-report: print info and exit                              */
    /* -------------------------------------------------------------- */

    if (crashReport) {
        if (sk_crash_has_previous_dumps()) {
            char *dir = sk_crash_get_dir();
            fprintf(stdout, "Crash dumps found in: %s\n", dir);
            g_free(dir);
        } else {
            fprintf(stdout, "No crash dumps from previous runs.\n");
        }
        return 0;
    }

    /* -------------------------------------------------------------- */
    /* Logging initialisation                                           */
    /* -------------------------------------------------------------- */

    {
        QByteArray compUtf8 = debugComp.toUtf8();
        sk_log_init(debugMode, traceMode,
                    debugComp.isEmpty() ? nullptr : compUtf8.constData());
    }

    SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "shellkeep %s starting (Qt6)",
                SK_VERSION_STRING);

    /* -------------------------------------------------------------- */
    /* Crash handler                                                    */
    /* -------------------------------------------------------------- */

    sk_crash_handler_install();

    /* -------------------------------------------------------------- */
    /* Permissions — NFR-SEC-03                                         */
    /* -------------------------------------------------------------- */

    (void)sk_permissions_verify_and_fix();

    /* -------------------------------------------------------------- */
    /* Configuration — FR-CONFIG-01                                     */
    /* -------------------------------------------------------------- */

    GError *cfgError = nullptr;
    QByteArray configPathUtf8 = configPath.toUtf8();
    SkConfig *config = sk_config_load(
        configPath.isEmpty() ? nullptr : configPathUtf8.constData(),
        &cfgError);

    if (!config) {
        SK_LOG_ERROR(SK_LOG_COMPONENT_GENERAL,
                     "failed to load config: %s — using defaults",
                     cfgError ? cfgError->message : "unknown");
        g_clear_error(&cfgError);
        config = sk_config_new_defaults();
    }

    /* -------------------------------------------------------------- */
    /* Single instance — FR-CLI-04, FR-CLI-05                           */
    /*                                                                  */
    /* Strategy: QLockFile prevents races; QLocalServer/QLocalSocket    */
    /* forwards arguments to the running instance.                      */
    /* -------------------------------------------------------------- */

    const QString serverName = localServerName();
    const QString lockPath   = lockFilePath();

    auto lockFile = std::make_unique<QLockFile>(lockPath);
    lockFile->setStaleLockTime(0); /* we manage staleness ourselves */

    if (!lockFile->tryLock(100)) {
        /* Another instance is running.  Forward our arguments. */
        SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL,
                    "another instance detected, forwarding arguments");

        if (trySendToRunningInstance(serverName, QCoreApplication::arguments())) {
            sk_config_free(config);
            return 0;
        }

        /* Could not connect — stale lock.  Remove and retry. */
        lockFile->removeStaleLockFile();
        if (!lockFile->tryLock(100)) {
            SK_LOG_ERROR(SK_LOG_COMPONENT_GENERAL,
                         "could not acquire lock file");
            sk_config_free(config);
            return 1;
        }
    }

    /* Set up local server to receive args from future invocations. */
    QLocalServer::removeServer(serverName); /* clean up stale socket */
    QLocalServer localServer;
    localServer.listen(serverName);

    QObject::connect(&localServer, &QLocalServer::newConnection, [&]() {
        QLocalSocket *sock = localServer.nextPendingConnection();
        if (!sock)
            return;
        sock->waitForReadyRead(1000);
        QByteArray data = sock->readAll();
        sock->deleteLater();

        /* data is newline-separated argv.  For now we log it and could
         * raise the window / open a new tab.  Full handling will be
         * wired up when SkMainWindow is implemented. */
        SK_LOG_INFO(SK_LOG_COMPONENT_UI,
                    "received args from second instance: %s",
                    data.constData());

        /* FR-CLI-05: forward args to open new connection in existing window */
    });

    /* -------------------------------------------------------------- */
    /* UI Bridge — set up the Qt implementation                         */
    /* -------------------------------------------------------------- */

    auto *bridgeQt = new SkUiBridgeQt(&app);
    if (parser.isSet(acceptHostsOpt))
        bridgeQt->setAutoAcceptHosts(true);
    if (parser.isSet(autoPasswordOpt))
        bridgeQt->setAutoPassword(parser.value(autoPasswordOpt));
    sk_ui_bridge_set(bridgeQt->bridge(),
                     static_cast<SkUiHandle>(bridgeQt));

    /* -------------------------------------------------------------- */
    /* GLib event loop integration                                      */
    /* -------------------------------------------------------------- */

    GLibEventThread glibThread;
    glibThread.start();

    /* -------------------------------------------------------------- */
    /* Stylesheet                                                       */
    /* -------------------------------------------------------------- */

    app.setStyleSheet(SkStyleSheet::get());

    /* -------------------------------------------------------------- */
    /* Signal handlers for graceful shutdown                             */
    /* -------------------------------------------------------------- */

    SkConnectFlow::installSignalHandlers();

    /* -------------------------------------------------------------- */
    /* Create main window / start connection                            */
    /* -------------------------------------------------------------- */

    /* Use the bridge's primary window as THE main window.
     * The bridge creates it lazily, so we request it now and show it. */
    auto *mainWindow = bridgeQt->primaryWindow();
    mainWindow->setWindowTitle(QStringLiteral("shellkeep"));
    if (minimized) {
        mainWindow->hide();
    } else {
        mainWindow->show();
    }

    /* -------------------------------------------------------------- */
    /* System tray icon                                                */
    /* -------------------------------------------------------------- */

    if (SkTrayIcon::isAvailable()) {
        auto *tray = bridgeQt->trayIcon();
        tray->show();
        SK_LOG_INFO(SK_LOG_COMPONENT_UI, "system tray icon shown");
    } else {
        SK_LOG_INFO(SK_LOG_COMPONENT_UI, "system tray not available, skipping");
    }

    std::unique_ptr<SkConnectFlow> connectFlow;

    if (!hostArg.isEmpty()) {
        SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL,
                    "target host=%s port=%d user=%s",
                    qPrintable(hostArg),
                    port > 0 ? port : 22,
                    loginUser.isEmpty() ? "(default)" : qPrintable(loginUser));

        SkConnectFlowParams params;
        params.hostname     = hostArg;
        params.port         = port;
        params.username     = loginUser;
        params.identityFile = identityFile;

        connectFlow = std::make_unique<SkConnectFlow>(config);

        QObject::connect(connectFlow.get(), &SkConnectFlow::connected, [&]() {
            SK_LOG_INFO(SK_LOG_COMPONENT_UI, "connection established");
            /* TODO: signal SkMainWindow to switch from feedback to
             * terminal view. */
        });

        QObject::connect(connectFlow.get(), &SkConnectFlow::error,
                         [](const QString &msg) {
            SK_LOG_ERROR(SK_LOG_COMPONENT_UI, "connection error: %s",
                         qPrintable(msg));
            /* TODO: show error in SkMainWindow. */
        });

        QObject::connect(connectFlow.get(), &SkConnectFlow::disconnected, [&]() {
            SK_LOG_INFO(SK_LOG_COMPONENT_UI, "disconnected");
        });

        connectFlow->start(params);
    } else {
        SK_LOG_INFO(SK_LOG_COMPONENT_UI, "no host specified, showing welcome");
        auto *welcome = new SkWelcomeWidget();
        mainWindow->setCentralWidget(welcome);

        /* Helper to clean up feedback overlay */
        QPointer<SkConnFeedback> feedbackPtr;

        auto cleanupFeedback = [&feedbackPtr]() {
            if (feedbackPtr) {
                feedbackPtr->hide();
                feedbackPtr->deleteLater();
            }
        };

        QObject::connect(welcome, &SkWelcomeWidget::connectRequested,
                         [&, welcome, mainWindow, cleanupFeedback](const QString &host, const QString &user, int p) {
            /* Guard: ignore if already connecting */
            if (connectFlow) {
                SK_LOG_WARN(SK_LOG_COMPONENT_UI,
                            "connect requested while already connecting, ignoring");
                return;
            }

            SK_LOG_INFO(SK_LOG_COMPONENT_UI, "welcome: connect to %s:%d",
                        qPrintable(host), p);

            welcome->setConnecting(true);

            /* Show feedback overlay */
            cleanupFeedback();
            auto *fb = new SkConnFeedback(mainWindow);
            feedbackPtr = fb;
            fb->setPhase(SK_BRIDGE_PHASE_CONNECTING);

            SkConnectFlowParams wp;
            wp.hostname = host;
            wp.username = user;
            wp.port = p;
            wp.identityFile = identityFile;
            connectFlow = std::make_unique<SkConnectFlow>(config);

            QObject::connect(connectFlow.get(), &SkConnectFlow::phaseChanged,
                             fb, [fb](int phase) {
                fb->setPhase(static_cast<SkBridgeConnPhase>(phase));
            });

            QObject::connect(connectFlow.get(), &SkConnectFlow::connected,
                             [&, welcome, cleanupFeedback]() {
                SK_LOG_INFO(SK_LOG_COMPONENT_UI, "connection established");
                cleanupFeedback();
                welcome->setConnecting(false);
            });

            QObject::connect(connectFlow.get(), &SkConnectFlow::error,
                             fb, [&, welcome, fb, cleanupFeedback](const QString &msg) {
                SK_LOG_ERROR(SK_LOG_COMPONENT_UI, "connection error: %s",
                             qPrintable(msg));
                fb->setError(msg);
                /* Auto-hide feedback after 5 seconds on error */
                QTimer::singleShot(5000, fb, [&, welcome, cleanupFeedback]() {
                    cleanupFeedback();
                    connectFlow.reset();
                    welcome->setConnecting(false);
                });
            });

            connectFlow->start(wp);
        });

        QObject::connect(welcome, &SkWelcomeWidget::cancelRequested,
                         [&, welcome, cleanupFeedback]() {
            SK_LOG_INFO(SK_LOG_COMPONENT_UI, "connection cancelled by user");
            cleanupFeedback();
            connectFlow.reset();
            welcome->setConnecting(false);
        });
    }

    /* -------------------------------------------------------------- */
    /* Event loop                                                       */
    /* -------------------------------------------------------------- */

    int exitCode = app.exec();

    /* -------------------------------------------------------------- */
    /* Cleanup                                                          */
    /* -------------------------------------------------------------- */

    connectFlow.reset();

    glibThread.requestStop();
    glibThread.wait(2000);

    localServer.close();
    lockFile->unlock();

    SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "application shutting down");
    sk_log_shutdown();
    sk_config_free(config);

    return exitCode;
}

/* ------------------------------------------------------------------ */
/* Helper: parse "user@host" into separate fields                       */
/* ------------------------------------------------------------------ */

static void parseHostString(const QString &hostStr, QString &outUser,
                            QString &outHost)
{
    int at = hostStr.indexOf(QLatin1Char('@'));
    if (at >= 0) {
        outUser = hostStr.left(at);
        outHost = hostStr.mid(at + 1);
    } else {
        outHost = hostStr;
    }
}

/* ------------------------------------------------------------------ */
/* Helper: send argv to running instance via QLocalSocket               */
/* ------------------------------------------------------------------ */

static bool trySendToRunningInstance(const QString &serverName,
                                     const QStringList &args)
{
    QLocalSocket socket;
    socket.connectToServer(serverName);
    if (!socket.waitForConnected(500))
        return false;

    QByteArray payload = args.join(QLatin1Char('\n')).toUtf8();
    socket.write(payload);
    socket.waitForBytesWritten(500);
    socket.disconnectFromServer();
    return true;
}

/* ------------------------------------------------------------------ */
/* Helper: lock file path                                               */
/* ------------------------------------------------------------------ */

static QString lockFilePath()
{
    QString runtimeDir = QStandardPaths::writableLocation(
        QStandardPaths::RuntimeLocation);
    if (runtimeDir.isEmpty()) {
        runtimeDir = QStandardPaths::writableLocation(
            QStandardPaths::TempLocation);
    }
    return runtimeDir + QStringLiteral("/shellkeep-") +
           QString::fromLocal8Bit(qgetenv("USER").constData()) +
           QStringLiteral(".lock");
}

/* ------------------------------------------------------------------ */
/* Helper: local server name                                            */
/* ------------------------------------------------------------------ */

static QString localServerName()
{
    return QStringLiteral("shellkeep-") +
           QString::fromLocal8Bit(qgetenv("USER").constData());
}

/* ------------------------------------------------------------------ */
/* MOC include for GLibEventThread (defined in this TU)                 */
/* ------------------------------------------------------------------ */

#include "main_qt.moc"
