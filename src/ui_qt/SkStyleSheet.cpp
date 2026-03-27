// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#include "SkStyleSheet.h"

QString SkStyleSheet::get()
{
    return QStringLiteral(
        /* ---------------------------------------------------------- */
        /* Global defaults                                             */
        /* ---------------------------------------------------------- */
        "* {"
        "  font-family: 'Inter', 'Segoe UI', 'Helvetica Neue', sans-serif;"
        "}"

        /* ---------------------------------------------------------- */
        /* QMainWindow / QWidget base                                  */
        /* ---------------------------------------------------------- */
        "QMainWindow, QDialog, QWidget {"
        "  background-color: #1e1e2e;"
        "  color: #cdd6f4;"
        "}"

        /* ---------------------------------------------------------- */
        /* QTabWidget                                                  */
        /* ---------------------------------------------------------- */
        "QTabWidget {"
        "  background-color: #1e1e2e;"
        "}"
        "QTabWidget::pane {"
        "  border: none;"
        "  background-color: #1e1e2e;"
        "}"
        "QTabWidget::tab-bar {"
        "  alignment: left;"
        "}"

        /* ---------------------------------------------------------- */
        /* QTabBar                                                     */
        /* ---------------------------------------------------------- */
        "QTabBar {"
        "  background-color: #181825;"
        "  border: none;"
        "}"
        "QTabBar::tab {"
        "  background-color: #181825;"
        "  color: #a6adc8;"
        "  padding: 8px 24px 8px 20px;"
        "  margin: 2px 1px 0 1px;"
        "  border-top-left-radius: 6px;"
        "  border-top-right-radius: 6px;"
        "  min-width: 80px;"
        "}"
        "QTabBar::tab:selected {"
        "  background-color: #1e1e2e;"
        "  color: #cdd6f4;"
        "  border-bottom: 2px solid #89b4fa;"
        "}"
        "QTabBar::tab:hover:!selected {"
        "  background-color: #313244;"
        "  color: #bac2de;"
        "}"
        "QTabBar::close-button {"
        "  subcontrol-position: right;"
        "  padding: 2px;"
        "}"
        "QTabBar::close-button:hover {"
        "  background-color: #45475a;"
        "  border-radius: 3px;"
        "}"

        /* ---------------------------------------------------------- */
        /* QLineEdit                                                   */
        /* ---------------------------------------------------------- */
        "QLineEdit {"
        "  background-color: #313244;"
        "  color: #cdd6f4;"
        "  border: 1px solid #45475a;"
        "  border-radius: 6px;"
        "  padding: 6px 10px;"
        "  selection-background-color: #89b4fa;"
        "  selection-color: #11111b;"
        "}"
        "QLineEdit:focus {"
        "  border: 1px solid #89b4fa;"
        "}"
        "QLineEdit:disabled {"
        "  background-color: #181825;"
        "  color: #6c7086;"
        "}"

        /* ---------------------------------------------------------- */
        /* QPushButton                                                 */
        /* ---------------------------------------------------------- */
        "QPushButton {"
        "  background-color: #313244;"
        "  color: #cdd6f4;"
        "  border: 1px solid #45475a;"
        "  border-radius: 6px;"
        "  padding: 6px 16px;"
        "  min-height: 28px;"
        "}"
        "QPushButton:hover {"
        "  background-color: #45475a;"
        "  border-color: #585b70;"
        "}"
        "QPushButton:pressed {"
        "  background-color: #585b70;"
        "}"
        "QPushButton:default {"
        "  background-color: #89b4fa;"
        "  color: #11111b;"
        "  border: none;"
        "  font-weight: bold;"
        "}"
        "QPushButton:default:hover {"
        "  background-color: #b4befe;"
        "}"
        "QPushButton:default:pressed {"
        "  background-color: #cba6f7;"
        "}"
        "QPushButton:disabled {"
        "  background-color: #181825;"
        "  color: #6c7086;"
        "  border-color: #313244;"
        "}"

        /* ---------------------------------------------------------- */
        /* QComboBox                                                   */
        /* ---------------------------------------------------------- */
        "QComboBox {"
        "  background-color: #313244;"
        "  color: #cdd6f4;"
        "  border: 1px solid #45475a;"
        "  border-radius: 6px;"
        "  padding: 6px 10px;"
        "}"
        "QComboBox:hover {"
        "  border-color: #89b4fa;"
        "}"
        "QComboBox::drop-down {"
        "  border: none;"
        "  padding-right: 8px;"
        "}"
        "QComboBox QAbstractItemView {"
        "  background-color: #313244;"
        "  color: #cdd6f4;"
        "  border: 1px solid #45475a;"
        "  selection-background-color: #89b4fa;"
        "  selection-color: #11111b;"
        "}"

        /* ---------------------------------------------------------- */
        /* QMenu                                                       */
        /* ---------------------------------------------------------- */
        "QMenu {"
        "  background-color: #313244;"
        "  color: #cdd6f4;"
        "  border: 1px solid #45475a;"
        "  border-radius: 6px;"
        "  padding: 4px 0;"
        "}"
        "QMenu::item {"
        "  padding: 6px 24px 6px 16px;"
        "}"
        "QMenu::item:selected {"
        "  background-color: #45475a;"
        "  color: #cdd6f4;"
        "}"
        "QMenu::item:disabled {"
        "  color: #6c7086;"
        "}"
        "QMenu::separator {"
        "  height: 1px;"
        "  background-color: #45475a;"
        "  margin: 4px 8px;"
        "}"

        /* ---------------------------------------------------------- */
        /* QDialog                                                     */
        /* ---------------------------------------------------------- */
        "QDialog {"
        "  background-color: #1e1e2e;"
        "  color: #cdd6f4;"
        "}"

        /* ---------------------------------------------------------- */
        /* QLabel                                                      */
        /* ---------------------------------------------------------- */
        "QLabel {"
        "  color: #cdd6f4;"
        "}"

        /* ---------------------------------------------------------- */
        /* QProgressBar                                                */
        /* ---------------------------------------------------------- */
        "QProgressBar {"
        "  background-color: #313244;"
        "  border: 1px solid #45475a;"
        "  border-radius: 4px;"
        "  text-align: center;"
        "  color: #cdd6f4;"
        "  height: 8px;"
        "}"
        "QProgressBar::chunk {"
        "  background-color: #89b4fa;"
        "  border-radius: 3px;"
        "}"

        /* ---------------------------------------------------------- */
        /* QToolTip                                                    */
        /* ---------------------------------------------------------- */
        "QToolTip {"
        "  background-color: #313244;"
        "  color: #cdd6f4;"
        "  border: 1px solid #45475a;"
        "  border-radius: 4px;"
        "  padding: 4px 8px;"
        "}"

        /* ---------------------------------------------------------- */
        /* QScrollBar (vertical)                                       */
        /* ---------------------------------------------------------- */
        "QScrollBar:vertical {"
        "  background-color: #1e1e2e;"
        "  width: 10px;"
        "  margin: 0;"
        "}"
        "QScrollBar::handle:vertical {"
        "  background-color: #45475a;"
        "  border-radius: 5px;"
        "  min-height: 20px;"
        "}"
        "QScrollBar::handle:vertical:hover {"
        "  background-color: #585b70;"
        "}"
        "QScrollBar::add-line:vertical,"
        "QScrollBar::sub-line:vertical {"
        "  height: 0;"
        "}"
        "QScrollBar::add-page:vertical,"
        "QScrollBar::sub-page:vertical {"
        "  background: none;"
        "}"

        /* ---------------------------------------------------------- */
        /* QScrollBar (horizontal)                                     */
        /* ---------------------------------------------------------- */
        "QScrollBar:horizontal {"
        "  background-color: #1e1e2e;"
        "  height: 10px;"
        "  margin: 0;"
        "}"
        "QScrollBar::handle:horizontal {"
        "  background-color: #45475a;"
        "  border-radius: 5px;"
        "  min-width: 20px;"
        "}"
        "QScrollBar::handle:horizontal:hover {"
        "  background-color: #585b70;"
        "}"
        "QScrollBar::add-line:horizontal,"
        "QScrollBar::sub-line:horizontal {"
        "  width: 0;"
        "}"
        "QScrollBar::add-page:horizontal,"
        "QScrollBar::sub-page:horizontal {"
        "  background: none;"
        "}"

        /* ---------------------------------------------------------- */
        /* QListWidget                                                 */
        /* ---------------------------------------------------------- */
        "QListWidget {"
        "  background-color: #313244;"
        "  color: #cdd6f4;"
        "  border: 1px solid #45475a;"
        "  border-radius: 6px;"
        "  outline: none;"
        "}"
        "QListWidget::item {"
        "  padding: 4px 8px;"
        "}"
        "QListWidget::item:hover {"
        "  background-color: #45475a;"
        "}"
        "QListWidget::item:selected {"
        "  background-color: #89b4fa;"
        "  color: #11111b;"
        "}"

        /* ---------------------------------------------------------- */
        /* QMessageBox                                                 */
        /* ---------------------------------------------------------- */
        "QMessageBox {"
        "  background-color: #1e1e2e;"
        "  color: #cdd6f4;"
        "}"
        "QMessageBox QLabel {"
        "  color: #cdd6f4;"
        "}"

        /* ---------------------------------------------------------- */
        /* QDialogButtonBox                                            */
        /* ---------------------------------------------------------- */
        "QDialogButtonBox QPushButton {"
        "  min-width: 80px;"
        "}"

        /* ---------------------------------------------------------- */
        /* Focus indicator                                             */
        /* ---------------------------------------------------------- */
        "*:focus {"
        "  outline: none;"
        "}"
    );
}
