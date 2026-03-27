// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file SkTerminalTheme.h
 * @brief Terminal theme management for the Qt terminal widget.
 *
 * FR-TERMINAL-11: Converts between the C-side SkTheme (uint32_t 0xRRGGBB)
 * and Qt QColor values. Provides 16 ANSI palette colors plus foreground,
 * background, and cursor colors with Catppuccin Mocha-inspired defaults.
 */

#ifndef SK_TERMINAL_THEME_H
#define SK_TERMINAL_THEME_H

#include <QColor>
#include <QString>
#include <array>

extern "C" {
#include "shellkeep/sk_config.h"
}

class SkTerminalWidget;

/**
 * Terminal color theme for the Qt terminal widget.
 *
 * Holds 16 ANSI colors plus foreground, background, and cursor colors.
 * Can be constructed from the C-side SkTheme struct or loaded with defaults.
 */
class SkTerminalTheme
{
public:
    SkTerminalTheme();
    ~SkTerminalTheme();

    /** Load the default dark theme (Catppuccin Mocha inspired). */
    static SkTerminalTheme loadDefault();

    /** Create a theme from a C-side SkTheme struct. */
    static SkTerminalTheme fromSkTheme(const SkTheme *theme);

    /** Apply this theme to a terminal widget. */
    void applyToTerminal(SkTerminalWidget *terminal) const;

    /* ---- Color accessors ---- */

    /** ANSI palette color (index 0-15). */
    [[nodiscard]] QColor ansiColor(int index) const;

    /** Set ANSI palette color (index 0-15). */
    void setAnsiColor(int index, const QColor &color);

    /** Foreground color. */
    [[nodiscard]] QColor foreground() const;
    void setForeground(const QColor &color);

    /** Background color. */
    [[nodiscard]] QColor background() const;
    void setBackground(const QColor &color);

    /** Cursor color. */
    [[nodiscard]] QColor cursorColor() const;
    void setCursorColor(const QColor &color);
    [[nodiscard]] bool hasCursorColor() const;

    /** Theme name. */
    [[nodiscard]] QString name() const;
    void setName(const QString &name);

    /* ---- Conversion helpers ---- */

    /** Convert a 0xRRGGBB uint32_t to QColor. */
    static QColor colorFromRgb(uint32_t rgb);

    /** Convert a QColor to 0xRRGGBB uint32_t. */
    static uint32_t colorToRgb(const QColor &color);

private:
    QString m_name;
    std::array<QColor, 16> m_palette;
    QColor m_foreground;
    QColor m_background;
    QColor m_cursorColor;
    bool m_hasCursorColor = false;
};

#endif /* SK_TERMINAL_THEME_H */
