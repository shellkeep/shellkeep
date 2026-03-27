// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#include "SkDialogs.h"

#include <QApplication>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QMetaObject>
#include <QMutex>
#include <QMutexLocker>
#include <QPushButton>
#include <QThread>
#include <QTimer>
#include <QVBoxLayout>
#include <QWaitCondition>

#include "SkStyleSheet.h"

/* ------------------------------------------------------------------ */
/* Thread-safety helper                                                */
/* ------------------------------------------------------------------ */

/* Run a lambda on the Qt main thread, blocking the caller.
 * Uses QTimer::singleShot + QMutex/QWaitCondition instead of
 * BlockingQueuedConnection to avoid Qt 6.4 deadlock detection. */

template <typename Func>
auto SkDialogs::runOnUiThread(Func &&func) -> decltype(func())
{
    using ReturnType = decltype(func());

    if (QThread::currentThread() == QApplication::instance()->thread()) {
        return func();
    }

    ReturnType result{};
    QMutex mutex;
    QWaitCondition cond;
    bool done = false;
    mutex.lock();
    QTimer::singleShot(0, QApplication::instance(), [&]() {
        result = func();
        QMutexLocker lk(&mutex);
        done = true;
        cond.wakeOne();
    });
    while (!done)
        cond.wait(&mutex);
    mutex.unlock();
    return result;
}

template <>
auto SkDialogs::runOnUiThread<>(std::function<void()> &&func) -> void
{
    if (QThread::currentThread() == QApplication::instance()->thread()) {
        func();
        return;
    }

    QMutex mutex;
    QWaitCondition cond;
    bool done = false;
    mutex.lock();
    QTimer::singleShot(0, QApplication::instance(), [&]() {
        func();
        QMutexLocker lk(&mutex);
        done = true;
        cond.wakeOne();
    });
    while (!done)
        cond.wait(&mutex);
    mutex.unlock();
}

/* ------------------------------------------------------------------ */
/* Helper: create a styled QDialog                                     */
/* ------------------------------------------------------------------ */

static QDialog *createDialog(QWidget *parent, const QString &title)
{
    auto *dlg = new QDialog(parent);
    dlg->setWindowTitle(title);
    dlg->setStyleSheet(SkStyleSheet::get());
    dlg->setMinimumWidth(400);
    return dlg;
}

/* ------------------------------------------------------------------ */
/* hostKeyUnknown                                                      */
/* ------------------------------------------------------------------ */

SkBridgeHostKeyResult SkDialogs::hostKeyUnknown(QWidget *parent,
                                                 const QString &hostname,
                                                 const QString &fingerprint,
                                                 const QString &keyType)
{
    return runOnUiThread([&]() -> SkBridgeHostKeyResult {
        QDialog *dlg = createDialog(parent, QObject::tr("Unknown Host Key"));
        auto *layout = new QVBoxLayout(dlg);

        auto *icon = new QLabel(dlg);
        icon->setPixmap(dlg->style()->standardPixmap(QStyle::SP_MessageBoxQuestion));

        auto *msg = new QLabel(dlg);
        msg->setWordWrap(true);
        msg->setText(
            QObject::tr("The authenticity of host <b>%1</b> cannot be established.<br><br>"
               "Key type: %2<br>"
               "Fingerprint: <code>%3</code><br><br>"
               "Are you sure you want to continue connecting?")
                .arg(hostname.toHtmlEscaped(), keyType.toHtmlEscaped(),
                     fingerprint.toHtmlEscaped()));

        layout->addWidget(icon);
        layout->addWidget(msg);

        auto *buttons = new QDialogButtonBox(dlg);
        auto *acceptSave = buttons->addButton(QObject::tr("Accept && Save"), QDialogButtonBox::AcceptRole);
        auto *connectOnce = buttons->addButton(QObject::tr("Connect Once"), QDialogButtonBox::ActionRole);
        auto *reject = buttons->addButton(QObject::tr("Reject"), QDialogButtonBox::RejectRole);
        layout->addWidget(buttons);

        SkBridgeHostKeyResult result = SK_BRIDGE_HOST_KEY_REJECT;
        QObject::connect(acceptSave, &QPushButton::clicked, dlg, [&result, dlg]() {
            result = SK_BRIDGE_HOST_KEY_ACCEPT_SAVE;
            dlg->accept();
        });
        QObject::connect(connectOnce, &QPushButton::clicked, dlg, [&result, dlg]() {
            result = SK_BRIDGE_HOST_KEY_CONNECT_ONCE;
            dlg->accept();
        });
        QObject::connect(reject, &QPushButton::clicked, dlg, [&result, dlg]() {
            result = SK_BRIDGE_HOST_KEY_REJECT;
            dlg->reject();
        });

        dlg->exec();
        dlg->deleteLater();
        return result;
    });
}

/* ------------------------------------------------------------------ */
/* hostKeyChanged                                                      */
/* ------------------------------------------------------------------ */

void SkDialogs::hostKeyChanged(QWidget *parent,
                               const QString &hostname,
                               const QString &oldFingerprint,
                               const QString &newFingerprint,
                               const QString &keyType)
{
    auto fn = std::function<void()>([&]() {
        QMessageBox box(parent);
        box.setStyleSheet(SkStyleSheet::get());
        box.setIcon(QMessageBox::Critical);
        box.setWindowTitle(QObject::tr("HOST KEY CHANGED"));
        box.setText(
            QObject::tr("<b>WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!</b><br><br>"
               "Host: %1<br>"
               "Key type: %2<br><br>"
               "Old fingerprint:<br><code>%3</code><br><br>"
               "New fingerprint:<br><code>%4</code><br><br>"
               "Someone could be eavesdropping on you right now (man-in-the-middle attack). "
               "It is also possible that a host key has just been changed. "
               "Connection has been refused. Please verify the host key and update "
               "your known_hosts file manually.")
                .arg(hostname.toHtmlEscaped(), keyType.toHtmlEscaped(),
                     oldFingerprint.toHtmlEscaped(), newFingerprint.toHtmlEscaped()));
        box.setStandardButtons(QMessageBox::Ok);
        box.exec();
    });
    runOnUiThread(std::move(fn));
}

/* ------------------------------------------------------------------ */
/* authPassword                                                        */
/* ------------------------------------------------------------------ */

QString SkDialogs::authPassword(QWidget *parent, const QString &prompt)
{
    return runOnUiThread([&]() -> QString {
        QDialog *dlg = createDialog(parent, QObject::tr("Authentication"));
        auto *layout = new QVBoxLayout(dlg);

        auto *label = new QLabel(prompt.isEmpty() ? QObject::tr("Password:") : prompt, dlg);
        layout->addWidget(label);

        auto *input = new QLineEdit(dlg);
        input->setEchoMode(QLineEdit::Password);
        input->setPlaceholderText(QObject::tr("Enter password"));
        layout->addWidget(input);

        auto *buttons = new QDialogButtonBox(
            QDialogButtonBox::Ok | QDialogButtonBox::Cancel, dlg);
        layout->addWidget(buttons);

        QObject::connect(buttons, &QDialogButtonBox::accepted, dlg, &QDialog::accept);
        QObject::connect(buttons, &QDialogButtonBox::rejected, dlg, &QDialog::reject);
        QObject::connect(input, &QLineEdit::returnPressed, dlg, &QDialog::accept);

        QString result;
        if (dlg->exec() == QDialog::Accepted) {
            result = input->text();
        }
        dlg->deleteLater();
        return result;
    });
}

/* ------------------------------------------------------------------ */
/* authMfa                                                             */
/* ------------------------------------------------------------------ */

QStringList SkDialogs::authMfa(QWidget *parent,
                               const QString &name,
                               const QString &instruction,
                               const QStringList &prompts,
                               const QList<bool> &showInput)
{
    return runOnUiThread([&]() -> QStringList {
        QDialog *dlg = createDialog(parent, name.isEmpty() ? QObject::tr("Authentication") : name);
        auto *layout = new QVBoxLayout(dlg);

        if (!instruction.isEmpty()) {
            auto *instrLabel = new QLabel(instruction, dlg);
            instrLabel->setWordWrap(true);
            layout->addWidget(instrLabel);
        }

        QList<QLineEdit *> inputs;
        auto *form = new QFormLayout();
        for (int i = 0; i < prompts.size(); ++i) {
            auto *input = new QLineEdit(dlg);
            if (i < showInput.size() && !showInput[i]) {
                input->setEchoMode(QLineEdit::Password);
            }
            form->addRow(prompts[i], input);
            inputs.append(input);
        }
        layout->addLayout(form);

        auto *buttons = new QDialogButtonBox(
            QDialogButtonBox::Ok | QDialogButtonBox::Cancel, dlg);
        layout->addWidget(buttons);

        QObject::connect(buttons, &QDialogButtonBox::accepted, dlg, &QDialog::accept);
        QObject::connect(buttons, &QDialogButtonBox::rejected, dlg, &QDialog::reject);

        QStringList result;
        if (dlg->exec() == QDialog::Accepted) {
            for (auto *input : inputs) {
                result.append(input->text());
            }
        }
        dlg->deleteLater();
        return result;
    });
}

/* ------------------------------------------------------------------ */
/* authPassphrase                                                      */
/* ------------------------------------------------------------------ */

QString SkDialogs::authPassphrase(QWidget *parent, const QString &keyPath)
{
    return runOnUiThread([&]() -> QString {
        QDialog *dlg = createDialog(parent, QObject::tr("Key Passphrase"));
        auto *layout = new QVBoxLayout(dlg);

        auto *label = new QLabel(
            QObject::tr("Enter passphrase for key <b>%1</b>:").arg(keyPath.toHtmlEscaped()), dlg);
        label->setWordWrap(true);
        layout->addWidget(label);

        auto *input = new QLineEdit(dlg);
        input->setEchoMode(QLineEdit::Password);
        input->setPlaceholderText(QObject::tr("Passphrase"));
        layout->addWidget(input);

        auto *buttons = new QDialogButtonBox(
            QDialogButtonBox::Ok | QDialogButtonBox::Cancel, dlg);
        layout->addWidget(buttons);

        QObject::connect(buttons, &QDialogButtonBox::accepted, dlg, &QDialog::accept);
        QObject::connect(buttons, &QDialogButtonBox::rejected, dlg, &QDialog::reject);
        QObject::connect(input, &QLineEdit::returnPressed, dlg, &QDialog::accept);

        QString result;
        if (dlg->exec() == QDialog::Accepted) {
            result = input->text();
        }
        dlg->deleteLater();
        return result;
    });
}

/* ------------------------------------------------------------------ */
/* conflictDialog                                                      */
/* ------------------------------------------------------------------ */

bool SkDialogs::conflictDialog(QWidget *parent,
                               const QString &hostname,
                               const QString &connectedAt)
{
    return runOnUiThread([&]() -> bool {
        QDialog *dlg = createDialog(parent, QObject::tr("Session Conflict"));
        auto *layout = new QVBoxLayout(dlg);

        auto *msg = new QLabel(dlg);
        msg->setWordWrap(true);
        msg->setText(
            QObject::tr("Another device is currently connected to <b>%1</b>.<br><br>"
               "Connected since: %2<br><br>"
               "Disconnect the other device and connect here?")
                .arg(hostname.toHtmlEscaped(), connectedAt.toHtmlEscaped()));
        layout->addWidget(msg);

        auto *buttons = new QDialogButtonBox(dlg);
        auto *takeOver = buttons->addButton(
            QObject::tr("Disconnect && Connect Here"), QDialogButtonBox::AcceptRole);
        buttons->addButton(QDialogButtonBox::Cancel);
        layout->addWidget(buttons);

        QObject::connect(buttons, &QDialogButtonBox::accepted, dlg, &QDialog::accept);
        QObject::connect(buttons, &QDialogButtonBox::rejected, dlg, &QDialog::reject);

        Q_UNUSED(takeOver);

        bool result = (dlg->exec() == QDialog::Accepted);
        dlg->deleteLater();
        return result;
    });
}

/* ------------------------------------------------------------------ */
/* environmentSelect                                                   */
/* ------------------------------------------------------------------ */

QString SkDialogs::environmentSelect(QWidget *parent,
                                     const QStringList &envNames,
                                     const QString &lastEnv)
{
    return runOnUiThread([&]() -> QString {
        QDialog *dlg = createDialog(parent, QObject::tr("Select Environment"));
        auto *layout = new QVBoxLayout(dlg);

        auto *label = new QLabel(QObject::tr("Choose an environment:"), dlg);
        layout->addWidget(label);

        auto *combo = new QComboBox(dlg);
        combo->addItems(envNames);
        int lastIdx = envNames.indexOf(lastEnv);
        if (lastIdx >= 0) {
            combo->setCurrentIndex(lastIdx);
        }
        layout->addWidget(combo);

        auto *buttons = new QDialogButtonBox(
            QDialogButtonBox::Ok | QDialogButtonBox::Cancel, dlg);
        layout->addWidget(buttons);

        QObject::connect(buttons, &QDialogButtonBox::accepted, dlg, &QDialog::accept);
        QObject::connect(buttons, &QDialogButtonBox::rejected, dlg, &QDialog::reject);

        QString result;
        if (dlg->exec() == QDialog::Accepted) {
            result = combo->currentText();
        }
        dlg->deleteLater();
        return result;
    });
}

/* ------------------------------------------------------------------ */
/* closeWindow                                                         */
/* ------------------------------------------------------------------ */

SkBridgeCloseResult SkDialogs::closeWindow(QWidget *parent, int nActive)
{
    return runOnUiThread([&]() -> SkBridgeCloseResult {
        QDialog *dlg = createDialog(parent, QObject::tr("Close Window"));
        auto *layout = new QVBoxLayout(dlg);

        auto *msg = new QLabel(dlg);
        msg->setWordWrap(true);
        msg->setText(
            QObject::tr("You have <b>%1</b> active session(s).<br><br>"
               "What would you like to do?")
                .arg(nActive));
        layout->addWidget(msg);

        auto *buttons = new QDialogButtonBox(dlg);
        auto *hideBtn = buttons->addButton(
            QObject::tr("Hide to Tray"), QDialogButtonBox::ActionRole);
        auto *termBtn = buttons->addButton(
            QObject::tr("Terminate Sessions"), QDialogButtonBox::DestructiveRole);
        auto *cancelBtn = buttons->addButton(QDialogButtonBox::Cancel);
        layout->addWidget(buttons);

        SkBridgeCloseResult result = SK_BRIDGE_CLOSE_CANCEL;
        QObject::connect(hideBtn, &QPushButton::clicked, dlg, [&result, dlg]() {
            result = SK_BRIDGE_CLOSE_HIDE;
            dlg->accept();
        });
        QObject::connect(termBtn, &QPushButton::clicked, dlg, [&result, dlg]() {
            result = SK_BRIDGE_CLOSE_TERMINATE;
            dlg->accept();
        });
        QObject::connect(cancelBtn, &QPushButton::clicked, dlg, [&result, dlg]() {
            result = SK_BRIDGE_CLOSE_CANCEL;
            dlg->reject();
        });

        dlg->exec();
        dlg->deleteLater();
        return result;
    });
}

/* ------------------------------------------------------------------ */
/* errorDialog                                                         */
/* ------------------------------------------------------------------ */

void SkDialogs::errorDialog(QWidget *parent,
                            const QString &title,
                            const QString &message)
{
    auto fn = std::function<void()>([&]() {
        QMessageBox box(parent);
        box.setStyleSheet(SkStyleSheet::get());
        box.setIcon(QMessageBox::Critical);
        box.setWindowTitle(title);
        box.setText(message);
        box.setStandardButtons(QMessageBox::Ok);
        box.exec();
    });
    runOnUiThread(std::move(fn));
}

/* ------------------------------------------------------------------ */
/* infoDialog                                                          */
/* ------------------------------------------------------------------ */

void SkDialogs::infoDialog(QWidget *parent,
                           const QString &title,
                           const QString &message)
{
    auto fn = std::function<void()>([&]() {
        QMessageBox box(parent);
        box.setStyleSheet(SkStyleSheet::get());
        box.setIcon(QMessageBox::Information);
        box.setWindowTitle(title);
        box.setText(message);
        box.setStandardButtons(QMessageBox::Ok);
        box.exec();
    });
    runOnUiThread(std::move(fn));
}
