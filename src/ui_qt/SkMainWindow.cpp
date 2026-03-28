// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#include "SkMainWindow.h"

#include <QCloseEvent>
#include <QContextMenuEvent>
#include <QHBoxLayout>
#include <QKeySequence>
#include <QLineEdit>
#include <QMenu>
#include <QMouseEvent>
#include <QPainter>
#include <QSettings>
#include <QShortcut>
#include <QStyle>
#include <QStyleOptionTab>
#include <QVBoxLayout>

#include "SkStyleSheet.h"

/* ================================================================== */
/* SkTabBar                                                            */
/* ================================================================== */

SkTabBar::SkTabBar(QWidget *parent)
    : QTabBar(parent)
{
    setMovable(true);
    setTabsClosable(true);
    setElideMode(Qt::ElideRight);
    setExpanding(false);
    setDocumentMode(true);
    setAcceptDrops(true);
    setChangeCurrentOnDrag(true);
}

void SkTabBar::setIndicator(int index, SkBridgeConnIndicator indicator)
{
    m_tabStates[index].indicator = indicator;
    update();
}

void SkTabBar::setDead(int index, bool dead)
{
    m_tabStates[index].dead = dead;
    if (dead) {
        setTabIcon(index, QIcon::fromTheme("dialog-warning"));
    } else {
        setTabIcon(index, QIcon());
    }
    update();
}

void SkTabBar::mouseDoubleClickEvent(QMouseEvent *event)
{
    int index = tabAt(event->pos());
    if (index >= 0) {
        beginRename(index);
    } else {
        QTabBar::mouseDoubleClickEvent(event);
    }
}

void SkTabBar::contextMenuEvent(QContextMenuEvent *event)
{
    int index = tabAt(event->pos());
    if (index < 0)
        return;

    QMenu menu(this);

    QAction *renameAction = menu.addAction(tr("Rename"));
    QAction *duplicateAction = menu.addAction(tr("Duplicate"));
    menu.addSeparator();
    QAction *closeAction = menu.addAction(tr("Close"));
    QAction *closeOthersAction = menu.addAction(tr("Close Others"));

    if (count() <= 1)
        closeOthersAction->setEnabled(false);

    QAction *chosen = menu.exec(event->globalPos());

    if (chosen == renameAction) {
        beginRename(index);
    } else if (chosen == duplicateAction) {
        Q_EMIT duplicateRequested(index);
    } else if (chosen == closeAction) {
        Q_EMIT tabCloseRequested(index);
    } else if (chosen == closeOthersAction) {
        Q_EMIT closeOthersRequested(index);
    }
}

void SkTabBar::paintEvent(QPaintEvent *event)
{
    QTabBar::paintEvent(event);

    QPainter painter(this);
    painter.setRenderHint(QPainter::Antialiasing);

    for (int i = 0; i < count(); ++i) {
        QRect rect = tabRect(i);
        auto it = m_tabStates.find(i);

        /* Connection indicator dot */
        QColor dotColor;
        if (it != m_tabStates.end()) {
            switch (it->indicator) {
            case SK_BRIDGE_INDICATOR_GREEN:
                dotColor = QColor(SkStyleSheet::kGreen);
                break;
            case SK_BRIDGE_INDICATOR_YELLOW:
                dotColor = QColor(SkStyleSheet::kYellow);
                break;
            case SK_BRIDGE_INDICATOR_RED:
                dotColor = QColor(SkStyleSheet::kRed);
                break;
            }

            /* Draw indicator dot at left side of tab */
            int dotSize = 6;
            int dotX = rect.left() + 8;
            int dotY = rect.center().y() - dotSize / 2;
            painter.setPen(Qt::NoPen);
            painter.setBrush(dotColor);
            painter.drawEllipse(dotX, dotY, dotSize, dotSize);

            /* Dead tab: tint tab text red */
            if (it->dead) {
                setTabTextColor(i, QColor(SkStyleSheet::kRed));
            } else {
                setTabTextColor(i, QColor(SkStyleSheet::kText));
            }
        }
    }
}

void SkTabBar::beginRename(int index)
{
    if (m_renameEditor) {
        finishRename();
    }

    m_renameIndex = index;
    QRect rect = tabRect(index);

    m_renameEditor = new QLineEdit(this);
    m_renameEditor->setText(tabText(index));
    m_renameEditor->selectAll();
    m_renameEditor->setGeometry(rect.adjusted(20, 2, -20, -2));
    m_renameEditor->setFocus();
    m_renameEditor->show();

    connect(m_renameEditor, &QLineEdit::returnPressed,
            this, &SkTabBar::finishRename);
    connect(m_renameEditor, &QLineEdit::editingFinished,
            this, &SkTabBar::finishRename);
}

void SkTabBar::finishRename()
{
    if (!m_renameEditor || m_renameIndex < 0)
        return;

    QString newName = m_renameEditor->text().trimmed();
    int index = m_renameIndex;

    m_renameEditor->deleteLater();
    m_renameEditor = nullptr;
    m_renameIndex = -1;

    if (!newName.isEmpty()) {
        setTabText(index, newName);
        Q_EMIT tabRenamed(index, newName);
    }
}

void SkTabBar::cancelRename()
{
    if (m_renameEditor) {
        m_renameEditor->deleteLater();
        m_renameEditor = nullptr;
        m_renameIndex = -1;
    }
}

/* ================================================================== */
/* SkMainWindow                                                        */
/* ================================================================== */

SkMainWindow::SkMainWindow(QWidget *parent)
    : QMainWindow(parent)
{
    init();
    setMinimumSize(400, 300);
    resize(800, 600);
    restoreWindowGeometry();
}

SkMainWindow::SkMainWindow(const QString &title, int x, int y,
                           int width, int height, QWidget *parent)
    : QMainWindow(parent)
{
    init();
    setMinimumSize(400, 300);

    if (width > 0 && height > 0) {
        resize(width, height);
    } else {
        resize(800, 600);
    }
    if (x >= 0 && y >= 0) {
        move(x, y);
    }
    if (!title.isEmpty()) {
        setWindowTitle(title);
    }
}

SkMainWindow::~SkMainWindow()
{
    saveGeometry();
}

void SkMainWindow::init()
{
    setWindowTitle(QStringLiteral("shellkeep"));
    setupTabWidget();
    setupShortcuts();
}

void SkMainWindow::setupTabWidget()
{
    m_tabBar = new SkTabBar(this);
    m_tabs = new SkTabWidget(this);
    m_tabs->setCustomTabBar(m_tabBar);
    m_tabs->setTabsClosable(true);
    m_tabs->setMovable(true);
    m_tabs->setDocumentMode(true);

    setCentralWidget(m_tabs);

    connect(m_tabs, &QTabWidget::tabCloseRequested,
            this, &SkMainWindow::onTabCloseRequested);
    connect(m_tabs, &QTabWidget::currentChanged,
            this, &SkMainWindow::onCurrentChanged);
    connect(m_tabBar, &SkTabBar::tabRenamed,
            this, &SkMainWindow::tabRenamed);
}

void SkMainWindow::setupShortcuts()
{
    /* Ctrl+Shift+T: new tab */
    auto *newTab = new QShortcut(QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_T), this);
    connect(newTab, &QShortcut::activated, this, &SkMainWindow::newTabRequested);

    /* Ctrl+Shift+W: close tab */
    auto *closeTab = new QShortcut(QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_W), this);
    connect(closeTab, &QShortcut::activated, this, [this]() {
        int idx = m_tabs->currentIndex();
        if (idx >= 0)
            onTabCloseRequested(idx);
    });

    /* Ctrl+Shift+N: new window */
    auto *newWin = new QShortcut(QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_N), this);
    connect(newWin, &QShortcut::activated, this, &SkMainWindow::newWindowRequested);

    /* Ctrl+Shift+F: find */
    auto *find = new QShortcut(QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_F), this);
    connect(find, &QShortcut::activated, this, &SkMainWindow::findRequested);

    /* Ctrl+PgUp: previous tab */
    auto *prevTab = new QShortcut(QKeySequence(Qt::CTRL | Qt::Key_PageUp), this);
    connect(prevTab, &QShortcut::activated, this, [this]() {
        int idx = m_tabs->currentIndex();
        if (idx > 0)
            m_tabs->setCurrentIndex(idx - 1);
    });

    /* Ctrl+PgDn: next tab */
    auto *nextTab = new QShortcut(QKeySequence(Qt::CTRL | Qt::Key_PageDown), this);
    connect(nextTab, &QShortcut::activated, this, [this]() {
        int idx = m_tabs->currentIndex();
        if (idx < m_tabs->count() - 1)
            m_tabs->setCurrentIndex(idx + 1);
    });

    /* Ctrl+Shift+C: copy */
    auto *copy = new QShortcut(QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_C), this);
    connect(copy, &QShortcut::activated, this, [this]() {
        QWidget *w = m_tabs->currentWidget();
        if (w) {
            /* Forward to terminal widget's copy method via meta-call */
            QMetaObject::invokeMethod(w, "copyClipboard", Qt::DirectConnection);
        }
    });

    /* Ctrl+Shift+V: paste */
    auto *paste = new QShortcut(QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_V), this);
    connect(paste, &QShortcut::activated, this, [this]() {
        QWidget *w = m_tabs->currentWidget();
        if (w) {
            QMetaObject::invokeMethod(w, "pasteClipboard", Qt::DirectConnection);
        }
    });

    /* Ctrl+Shift+A: copy all */
    auto *copyAll = new QShortcut(QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_A), this);
    connect(copyAll, &QShortcut::activated, this, [this]() {
        QWidget *w = m_tabs->currentWidget();
        if (w) {
            QMetaObject::invokeMethod(w, "copyAll", Qt::DirectConnection);
        }
    });

    /* Ctrl+=: zoom in */
    auto *zoomIn = new QShortcut(QKeySequence(Qt::CTRL | Qt::Key_Equal), this);
    connect(zoomIn, &QShortcut::activated, this, [this]() {
        QWidget *w = m_tabs->currentWidget();
        if (w) {
            QMetaObject::invokeMethod(w, "zoomIn", Qt::DirectConnection);
        }
    });

    /* Ctrl+-: zoom out */
    auto *zoomOut = new QShortcut(QKeySequence(Qt::CTRL | Qt::Key_Minus), this);
    connect(zoomOut, &QShortcut::activated, this, [this]() {
        QWidget *w = m_tabs->currentWidget();
        if (w) {
            QMetaObject::invokeMethod(w, "zoomOut", Qt::DirectConnection);
        }
    });

    /* F2: rename current tab */
    auto *rename = new QShortcut(QKeySequence(Qt::Key_F2), this);
    connect(rename, &QShortcut::activated, this, [this]() {
        int idx = m_tabs->currentIndex();
        if (idx >= 0)
            m_tabBar->beginRename(idx);
    });
}

int SkMainWindow::addTab(QWidget *terminalWidget, const QString &title)
{
    int index = m_tabs->addTab(terminalWidget, title);
    m_tabs->setCurrentIndex(index);
    return index;
}

void SkMainWindow::removeTab(int index)
{
    if (index >= 0 && index < m_tabs->count()) {
        QWidget *w = m_tabs->widget(index);
        m_tabs->removeTab(index);
        /* Do not delete the widget -- caller owns it */
        if (w) {
            w->setParent(nullptr);
        }
    }
}

int SkMainWindow::tabCount() const
{
    return m_tabs ? m_tabs->count() : 0;
}

int SkMainWindow::activeTab() const
{
    return m_tabs->currentIndex();
}

void SkMainWindow::setActiveTab(int index)
{
    if (index >= 0 && index < m_tabs->count()) {
        m_tabs->setCurrentIndex(index);
    }
}

QWidget *SkMainWindow::tabWidget(int index) const
{
    return m_tabs->widget(index);
}

void SkMainWindow::setTabTitle(int index, const QString &title)
{
    if (index >= 0 && index < m_tabs->count()) {
        m_tabs->setTabText(index, title);
    }
}

QString SkMainWindow::tabTitle(int index) const
{
    if (index >= 0 && index < m_tabs->count()) {
        return m_tabs->tabText(index);
    }
    return {};
}

void SkMainWindow::setTabIndicator(int index, SkBridgeConnIndicator indicator)
{
    m_tabBar->setIndicator(index, indicator);
}

void SkMainWindow::setTabDead(int index, bool dead)
{
    m_tabBar->setDead(index, dead);
}

void SkMainWindow::beginTabRename(int index)
{
    m_tabBar->beginRename(index);
}

void SkMainWindow::saveGeometry()
{
    QSettings settings(QStringLiteral("shellkeep"), QStringLiteral("shellkeep"));
    settings.beginGroup(QStringLiteral("MainWindow"));
    settings.setValue(QStringLiteral("geometry"), QMainWindow::saveGeometry());
    settings.setValue(QStringLiteral("state"), QMainWindow::saveState());
    settings.endGroup();
}

void SkMainWindow::restoreWindowGeometry()
{
    QSettings settings(QStringLiteral("shellkeep"), QStringLiteral("shellkeep"));
    settings.beginGroup(QStringLiteral("MainWindow"));
    QByteArray geo = settings.value(QStringLiteral("geometry")).toByteArray();
    QByteArray state = settings.value(QStringLiteral("state")).toByteArray();
    settings.endGroup();

    if (!geo.isEmpty()) {
        QMainWindow::restoreGeometry(geo);
    }
    if (!state.isEmpty()) {
        QMainWindow::restoreState(state);
    }
}

void SkMainWindow::closeEvent(QCloseEvent *event)
{
    Q_EMIT windowCloseRequested();
    /* The connect layer decides whether to accept or ignore the close.
     * Default: accept. The bridge close_dialog callback will call
     * event->ignore() if the user cancels. */
    event->accept();
    saveGeometry();
}

void SkMainWindow::onTabCloseRequested(int index)
{
    Q_EMIT tabClosed(index);
}

void SkMainWindow::onCurrentChanged(int index)
{
    if (index >= 0) {
        QString title = m_tabs->tabText(index);
        setWindowTitle(QStringLiteral("shellkeep \u2014 %1").arg(title));
    } else {
        setWindowTitle(QStringLiteral("shellkeep"));
    }
}
