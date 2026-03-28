// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#ifndef SK_MAIN_WINDOW_H
#define SK_MAIN_WINDOW_H

#include <QAction>
#include <QLabel>
#include <QMainWindow>
#include <QShortcut>
#include <QTabBar>
#include <QTabWidget>
#include <QWidget>

extern "C" {
#include "shellkeep/sk_ui_bridge.h"
}

/**
 * Custom tab bar supporting rename-on-double-click, context menu,
 * and per-tab connection indicator dots.
 */
class SkTabBar : public QTabBar
{
    Q_OBJECT

public:
    explicit SkTabBar(QWidget *parent = nullptr);

    /** Set the connection indicator color for a tab. */
    void setIndicator(int index, SkBridgeConnIndicator indicator);

    /** Mark a tab as dead (red title + warning icon). */
    void setDead(int index, bool dead);

Q_SIGNALS:
    void tabRenamed(int index, const QString &newName);
    void renameRequested(int index);
    void duplicateRequested(int index);
    void closeOthersRequested(int index);

protected:
    void mouseDoubleClickEvent(QMouseEvent *event) override;
    void contextMenuEvent(QContextMenuEvent *event) override;
    void paintEvent(QPaintEvent *event) override;

private Q_SLOTS:
    void finishRename();
    void cancelRename();

public:
    void beginRename(int index);

    int m_renameIndex = -1;
    class QLineEdit *m_renameEditor = nullptr;

    struct TabState {
        SkBridgeConnIndicator indicator = SK_BRIDGE_INDICATOR_GREEN;
        bool dead = false;
    };
    QHash<int, TabState> m_tabStates;
};

/**
 * Custom QTabWidget that exposes the protected setTabBar() method.
 */
class SkTabWidget : public QTabWidget
{
    Q_OBJECT

public:
    explicit SkTabWidget(QWidget *parent = nullptr) : QTabWidget(parent) {}

    /** Expose the protected setTabBar() so we can install a custom SkTabBar. */
    void setCustomTabBar(QTabBar *bar) { setTabBar(bar); }
};

/**
 * Main application window with tabbed terminal interface.
 *
 * FR-TABS-*, FR-UI-*
 */
class SkMainWindow : public QMainWindow
{
    Q_OBJECT

public:
    explicit SkMainWindow(QWidget *parent = nullptr);
    SkMainWindow(const QString &title, int x, int y, int width, int height,
                 QWidget *parent = nullptr);
    ~SkMainWindow() override;

    /** Add a terminal widget as a new tab. Returns the tab index. */
    int addTab(QWidget *terminalWidget, const QString &title);

    /** Remove a tab by index. Does not delete the widget. */
    void removeTab(int index);

    /** Get the number of tabs. */
    int tabCount() const;

    /** Get the currently active tab index. */
    int activeTab() const;

    /** Set the active tab by index. */
    void setActiveTab(int index);

    /** Get the widget at a tab index. */
    QWidget *tabWidget(int index) const;

    /** Set the tab title. */
    void setTabTitle(int index, const QString &title);

    /** Get the tab title. */
    QString tabTitle(int index) const;

    /** Set the connection indicator for a tab. */
    void setTabIndicator(int index, SkBridgeConnIndicator indicator);

    /** Mark a tab as dead. */
    void setTabDead(int index, bool dead);

    /** Begin editing a tab title. */
    void beginTabRename(int index);

    /** Get the tab widget (for adding overlays). */
    SkTabWidget *tabWidget() const { return m_tabs; }

    /** Save window geometry to settings. */
    void saveGeometry();

    /** Restore window geometry from settings. */
    void restoreWindowGeometry();

Q_SIGNALS:
    void tabClosed(int index);
    void tabRenamed(int index, const QString &newName);
    void windowCloseRequested();
    void newTabRequested();
    void newWindowRequested();
    void findRequested();

protected:
    void closeEvent(QCloseEvent *event) override;

private:
    void init();
    void ensureTabWidget();
    void setupShortcuts();
    void setupTabWidget();
    void onTabCloseRequested(int index);
    void onCurrentChanged(int index);

    SkTabWidget *m_tabs = nullptr;
    SkTabBar *m_tabBar = nullptr;
};

#endif /* SK_MAIN_WINDOW_H */
