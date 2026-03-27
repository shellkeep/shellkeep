// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file SkTerminalSearch.h
 * @brief Search overlay bar for the Qt terminal widget.
 *
 * FR-TERMINAL-07: Ctrl+Shift+F opens overlay search bar. Local scrollback
 * search with next/prev navigation and close-on-Esc.
 */

#ifndef SK_TERMINAL_SEARCH_H
#define SK_TERMINAL_SEARCH_H

#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QWidget>

/**
 * Dark-themed search overlay bar for terminal scrollback search.
 *
 * Positioned at the top of the terminal widget as an overlay.
 * Contains a text input, next/prev/close buttons, and a match status label.
 */
class SkTerminalSearch : public QWidget
{
    Q_OBJECT

public:
    explicit SkTerminalSearch(QWidget *parent = nullptr);
    ~SkTerminalSearch() override;

    /** Get the current search text. */
    [[nodiscard]] QString searchText() const;

    /** Set the match status label text. */
    void setStatusText(const QString &text);

    /** Clear the search field and status. */
    void clear();

    /** Focus the search input field. */
    void focusInput();

Q_SIGNALS:
    /** Emitted when the search text changes. */
    void searchRequested(const QString &text);

    /** Emitted when the user requests the next match. */
    void nextMatch();

    /** Emitted when the user requests the previous match. */
    void prevMatch();

    /** Emitted when the user closes the search bar. */
    void closed();

protected:
    void keyPressEvent(QKeyEvent *event) override;

private:
    QLineEdit *m_searchInput = nullptr;
    QLabel *m_statusLabel = nullptr;
    QPushButton *m_prevButton = nullptr;
    QPushButton *m_nextButton = nullptr;
    QPushButton *m_closeButton = nullptr;

    void setupUi();
    void applyStyleSheet();
};

#endif /* SK_TERMINAL_SEARCH_H */
