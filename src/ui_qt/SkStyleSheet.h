// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#ifndef SK_STYLESHEET_H
#define SK_STYLESHEET_H

#include <QString>

/**
 * Global dark theme stylesheet for shellkeep.
 *
 * Modern Catppuccin Mocha inspired palette:
 *   Base:    #1e1e2e
 *   Surface: #313244
 *   Overlay: #45475a
 *   Text:    #cdd6f4
 *   Accent:  blue=#89b4fa, green=#a6e3a1, red=#f38ba8, yellow=#f9e2af
 *
 * Provides QSS for all widgets: QMainWindow, QTabBar, QTabWidget,
 * QLineEdit, QPushButton, QDialog, QMenu, QToolTip, QProgressBar, etc.
 */
class SkStyleSheet
{
public:
    /** Get the full application stylesheet. */
    static QString get();

    /* Palette constants for programmatic use */
    static constexpr const char *kBase      = "#1e1e2e";
    static constexpr const char *kMantle    = "#181825";
    static constexpr const char *kCrust     = "#11111b";
    static constexpr const char *kSurface0  = "#313244";
    static constexpr const char *kSurface1  = "#45475a";
    static constexpr const char *kSurface2  = "#585b70";
    static constexpr const char *kOverlay0  = "#6c7086";
    static constexpr const char *kOverlay1  = "#7f849c";
    static constexpr const char *kText      = "#cdd6f4";
    static constexpr const char *kSubtext0  = "#a6adc8";
    static constexpr const char *kSubtext1  = "#bac2de";
    static constexpr const char *kBlue      = "#89b4fa";
    static constexpr const char *kGreen     = "#a6e3a1";
    static constexpr const char *kRed       = "#f38ba8";
    static constexpr const char *kYellow    = "#f9e2af";
    static constexpr const char *kPeach     = "#fab387";
    static constexpr const char *kMauve     = "#cba6f7";
    static constexpr const char *kLavender  = "#b4befe";

private:
    SkStyleSheet() = delete;
};

#endif /* SK_STYLESHEET_H */
