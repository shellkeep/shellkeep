// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file SkTerminalTheme.cpp
 * @brief Terminal theme management for the Qt terminal widget.
 *
 * FR-TERMINAL-11: Converts between the C-side SkTheme (uint32_t 0xRRGGBB)
 * and Qt QColor values. Default colors are Catppuccin Mocha inspired for
 * a modern dark look.
 */

#include "SkTerminalTheme.h"

#include "SkTerminalWidget.h"

/* ------------------------------------------------------------------ */
/* Default Catppuccin Mocha-inspired palette                           */
/* ------------------------------------------------------------------ */

/* 16 ANSI colors: Catppuccin Mocha palette. */
static constexpr uint32_t DEFAULT_ANSI[16] = {
    0x45475a, /* 0  black   (surface1)   */
    0xf38ba8, /* 1  red     (red)        */
    0xa6e3a1, /* 2  green   (green)      */
    0xf9e2af, /* 3  yellow  (yellow)     */
    0x89b4fa, /* 4  blue    (blue)       */
    0xf5c2e7, /* 5  magenta (pink)       */
    0x94e2d5, /* 6  cyan    (teal)       */
    0xbac2de, /* 7  white   (subtext1)   */
    0x585b70, /* 8  bright black  (surface2) */
    0xf38ba8, /* 9  bright red    (red)      */
    0xa6e3a1, /* 10 bright green  (green)    */
    0xf9e2af, /* 11 bright yellow (yellow)   */
    0x89b4fa, /* 12 bright blue   (blue)     */
    0xf5c2e7, /* 13 bright magenta(pink)     */
    0x94e2d5, /* 14 bright cyan   (teal)     */
    0xa6adc8, /* 15 bright white  (subtext0) */
};

static constexpr uint32_t DEFAULT_FG = 0xcdd6f4; /* text     */
static constexpr uint32_t DEFAULT_BG = 0x1e1e2e; /* base     */
static constexpr uint32_t DEFAULT_CURSOR = 0xf5e0dc; /* rosewater */

/* ------------------------------------------------------------------ */
/* Construction / Destruction                                          */
/* ------------------------------------------------------------------ */

SkTerminalTheme::SkTerminalTheme()
    : m_name(QStringLiteral("Default"))
    , m_foreground(colorFromRgb(DEFAULT_FG))
    , m_background(colorFromRgb(DEFAULT_BG))
    , m_cursorColor(colorFromRgb(DEFAULT_CURSOR))
    , m_hasCursorColor(false)
{
    for (int i = 0; i < 16; ++i) {
        m_palette[i] = colorFromRgb(DEFAULT_ANSI[i]);
    }
}

SkTerminalTheme::~SkTerminalTheme() = default;

/* ------------------------------------------------------------------ */
/* Factory: default theme                                              */
/* ------------------------------------------------------------------ */

SkTerminalTheme SkTerminalTheme::loadDefault()
{
    SkTerminalTheme theme;
    theme.m_name = QStringLiteral("Catppuccin Mocha");
    theme.m_hasCursorColor = true;
    return theme;
}

/* ------------------------------------------------------------------ */
/* Factory: from C-side SkTheme                                        */
/* ------------------------------------------------------------------ */

SkTerminalTheme SkTerminalTheme::fromSkTheme(const SkTheme *theme)
{
    SkTerminalTheme qt;

    if (theme == nullptr) {
        return qt;
    }

    if (theme->name != nullptr) {
        qt.m_name = QString::fromUtf8(theme->name);
    }

    for (int i = 0; i < 16; ++i) {
        qt.m_palette[i] = colorFromRgb(theme->ansi_colors[i]);
    }

    qt.m_foreground = colorFromRgb(theme->foreground);
    qt.m_background = colorFromRgb(theme->background);

    if (theme->has_cursor_color) {
        qt.m_cursorColor = colorFromRgb(theme->cursor_color);
        qt.m_hasCursorColor = true;
    } else {
        qt.m_hasCursorColor = false;
    }

    return qt;
}

/* ------------------------------------------------------------------ */
/* Apply to terminal widget                                            */
/* ------------------------------------------------------------------ */

void SkTerminalTheme::applyToTerminal(SkTerminalWidget *terminal) const
{
    if (terminal == nullptr) {
        return;
    }

    /* Build a stylesheet for the fallback QPlainTextEdit terminal.
     * When using QTermWidget, the colors are applied via its API. */
    QString bgHex = m_background.name(QColor::HexRgb);
    QString fgHex = m_foreground.name(QColor::HexRgb);

    /* Selection color: slightly brighter than background. */
    QColor selBg = m_background.lighter(160);
    QString selBgHex = selBg.name(QColor::HexRgb);

    QString stylesheet = QStringLiteral(
        "QPlainTextEdit {"
        "  background-color: %1;"
        "  color: %2;"
        "  selection-background-color: %3;"
        "  selection-color: %4;"
        "  border: none;"
        "}")
        .arg(bgHex, fgHex, selBgHex, fgHex);

    /* Apply via a method that the terminal widget understands.
     * We access the fallback terminal's stylesheet indirectly --
     * the terminal widget's internal QPlainTextEdit picks this up. */

    /* For now, set the widget's palette programmatically as well. */
    QPalette pal = terminal->palette();
    pal.setColor(QPalette::Base, m_background);
    pal.setColor(QPalette::Text, m_foreground);
    pal.setColor(QPalette::Window, m_background);
    pal.setColor(QPalette::WindowText, m_foreground);

    if (m_hasCursorColor) {
        /* Qt doesn't have a direct cursor color palette role,
         * but we can influence it through stylesheets. */
        stylesheet += QStringLiteral(
            " QPlainTextEdit { qproperty-cursorColor: %1; }")
            .arg(m_cursorColor.name(QColor::HexRgb));
    }

    terminal->setPalette(pal);
    terminal->setStyleSheet(stylesheet);
}

/* ------------------------------------------------------------------ */
/* Color accessors                                                     */
/* ------------------------------------------------------------------ */

QColor SkTerminalTheme::ansiColor(int index) const
{
    if (index < 0 || index >= 16) {
        return QColor();
    }
    return m_palette[index];
}

void SkTerminalTheme::setAnsiColor(int index, const QColor &color)
{
    if (index >= 0 && index < 16) {
        m_palette[index] = color;
    }
}

QColor SkTerminalTheme::foreground() const
{
    return m_foreground;
}

void SkTerminalTheme::setForeground(const QColor &color)
{
    m_foreground = color;
}

QColor SkTerminalTheme::background() const
{
    return m_background;
}

void SkTerminalTheme::setBackground(const QColor &color)
{
    m_background = color;
}

QColor SkTerminalTheme::cursorColor() const
{
    return m_cursorColor;
}

void SkTerminalTheme::setCursorColor(const QColor &color)
{
    m_cursorColor = color;
    m_hasCursorColor = true;
}

bool SkTerminalTheme::hasCursorColor() const
{
    return m_hasCursorColor;
}

QString SkTerminalTheme::name() const
{
    return m_name;
}

void SkTerminalTheme::setName(const QString &name)
{
    m_name = name;
}

/* ------------------------------------------------------------------ */
/* Conversion helpers                                                  */
/* ------------------------------------------------------------------ */

QColor SkTerminalTheme::colorFromRgb(uint32_t rgb)
{
    int r = (rgb >> 16) & 0xFF;
    int g = (rgb >> 8) & 0xFF;
    int b = rgb & 0xFF;
    return QColor(r, g, b);
}

uint32_t SkTerminalTheme::colorToRgb(const QColor &color)
{
    return (static_cast<uint32_t>(color.red()) << 16) |
           (static_cast<uint32_t>(color.green()) << 8) |
            static_cast<uint32_t>(color.blue());
}
