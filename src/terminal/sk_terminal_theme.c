// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_terminal_theme.c
 * @brief Terminal theme loading -- JSON (Gogh/base16 compatible).
 *
 * FR-TERMINAL-11: Terminal color themes configurable via JSON files in
 * ~/.config/shellkeep/themes/, compatible with Dracula, Catppuccin,
 * Solarized, etc.
 */

#include "shellkeep/sk_log.h"

#include "sk_terminal_internal.h"
#include <json-glib/json-glib.h>
#include <string.h>

/* ------------------------------------------------------------------ */
/* Default theme colors (dark)                                         */
/* ------------------------------------------------------------------ */

/* Standard 16-color dark palette matching common terminal defaults. */
static const char *DEFAULT_PALETTE[16] = {
  "#2e3436", /* black */
  "#cc0000", /* red */
  "#4e9a06", /* green */
  "#c4a000", /* yellow */
  "#3465a4", /* blue */
  "#75507b", /* magenta */
  "#06989a", /* cyan */
  "#d3d7cf", /* white */
  "#555753", /* bright black */
  "#ef2929", /* bright red */
  "#8ae234", /* bright green */
  "#fce94f", /* bright yellow */
  "#729fcf", /* bright blue */
  "#ad7fa8", /* bright magenta */
  "#34e2e2", /* bright cyan */
  "#eeeeec", /* bright white */
};

static const char *DEFAULT_FG = "#d3d7cf";
static const char *DEFAULT_BG = "#1e1e1e";

/* ------------------------------------------------------------------ */
/* Helpers                                                             */
/* ------------------------------------------------------------------ */

static bool
parse_color(const char *hex, GdkRGBA *out)
{
  if (hex == NULL || hex[0] == '\0')
  {
    return false;
  }
  return gdk_rgba_parse(out, hex);
}

static const char *
json_get_string(JsonObject *obj, const char *key)
{
  if (!json_object_has_member(obj, key))
  {
    return NULL;
  }
  JsonNode *node = json_object_get_member(obj, key);
  if (!JSON_NODE_HOLDS_VALUE(node))
  {
    return NULL;
  }
  return json_node_get_string(node);
}

/* ------------------------------------------------------------------ */
/* Public: default theme                                               */
/* ------------------------------------------------------------------ */

SkTerminalTheme *
sk_terminal_theme_default(void)
{
  SkTerminalTheme *theme = g_new0(SkTerminalTheme, 1);
  theme->name = g_strdup("Default Dark");

  for (int i = 0; i < 16; i++)
  {
    parse_color(DEFAULT_PALETTE[i], &theme->palette[i]);
  }

  parse_color(DEFAULT_FG, &theme->foreground);
  parse_color(DEFAULT_BG, &theme->background);

  theme->has_cursor_color = false;
  theme->has_cursor_fg = false;
  theme->has_highlight_bg = false;
  theme->has_highlight_fg = false;

  return theme;
}

/* ------------------------------------------------------------------ */
/* Public: load theme from JSON file                                   */
/* ------------------------------------------------------------------ */

SkTerminalTheme *
sk_terminal_theme_load(const char *path, GError **error)
{
  g_return_val_if_fail(path != NULL, NULL);

  JsonParser *parser = json_parser_new();

  if (!json_parser_load_from_file(parser, path, error))
  {
    g_object_unref(parser);
    return NULL;
  }

  JsonNode *root = json_parser_get_root(parser);
  if (root == NULL || !JSON_NODE_HOLDS_OBJECT(root))
  {
    g_set_error(error, SK_ERROR, SK_ERROR_GENERIC, "Theme file does not contain a JSON object");
    g_object_unref(parser);
    return NULL;
  }

  JsonObject *obj = json_node_get_object(root);
  SkTerminalTheme *theme = g_new0(SkTerminalTheme, 1);

  /* Name. */
  const char *name = json_get_string(obj, "name");
  theme->name = g_strdup(name ? name : "Untitled");

  /*
   * Try multiple key naming conventions to support Gogh, base16,
   * iTerm2, and other popular theme formats.
   */

  /* Palette colors: try "color0"-"color15", "color_01"-"color_16",
   * or "ansi" array. */
  bool palette_loaded = false;

  /* Format 1: Gogh style -- "color_01" to "color_16" (1-indexed). */
  for (int i = 0; i < 16; i++)
  {
    g_autofree char *key = g_strdup_printf("color_%02d", i + 1);
    const char *val = json_get_string(obj, key);
    if (val != NULL && parse_color(val, &theme->palette[i]))
    {
      palette_loaded = true;
    }
  }

  /* Format 2: base16 / standard -- "color0" to "color15" (0-indexed). */
  if (!palette_loaded)
  {
    for (int i = 0; i < 16; i++)
    {
      g_autofree char *key = g_strdup_printf("color%d", i);
      const char *val = json_get_string(obj, key);
      if (val != NULL && parse_color(val, &theme->palette[i]))
      {
        palette_loaded = true;
      }
    }
  }

  /* Format 3: "palette" array. */
  if (!palette_loaded && json_object_has_member(obj, "palette"))
  {
    JsonNode *pnode = json_object_get_member(obj, "palette");
    if (JSON_NODE_HOLDS_ARRAY(pnode))
    {
      JsonArray *arr = json_node_get_array(pnode);
      guint len = json_array_get_length(arr);
      for (guint i = 0; i < len && i < 16; i++)
      {
        const char *val = json_array_get_string_element(arr, i);
        if (val != NULL)
        {
          parse_color(val, &theme->palette[i]);
          palette_loaded = true;
        }
      }
    }
  }

  /* Fallback to defaults if palette not loaded. */
  if (!palette_loaded)
  {
    for (int i = 0; i < 16; i++)
    {
      parse_color(DEFAULT_PALETTE[i], &theme->palette[i]);
    }
  }

  /* Foreground: try "foreground", "fg", "foreground_color". */
  const char *fg = json_get_string(obj, "foreground");
  if (fg == NULL)
    fg = json_get_string(obj, "fg");
  if (fg == NULL)
    fg = json_get_string(obj, "foreground_color");
  if (fg == NULL)
    fg = DEFAULT_FG;
  parse_color(fg, &theme->foreground);

  /* Background: try "background", "bg", "background_color". */
  const char *bg = json_get_string(obj, "background");
  if (bg == NULL)
    bg = json_get_string(obj, "bg");
  if (bg == NULL)
    bg = json_get_string(obj, "background_color");
  if (bg == NULL)
    bg = DEFAULT_BG;
  parse_color(bg, &theme->background);

  /* Cursor color (optional). */
  const char *cursor = json_get_string(obj, "cursor");
  if (cursor == NULL)
    cursor = json_get_string(obj, "cursor_color");
  if (cursor == NULL)
    cursor = json_get_string(obj, "cursorColor");
  if (cursor != NULL)
  {
    theme->has_cursor_color = parse_color(cursor, &theme->cursor_color);
  }

  /* Cursor foreground (optional). */
  const char *cursor_fg = json_get_string(obj, "cursor_fg");
  if (cursor_fg == NULL)
    cursor_fg = json_get_string(obj, "cursorForeground");
  if (cursor_fg != NULL)
  {
    theme->has_cursor_fg = parse_color(cursor_fg, &theme->cursor_fg);
  }

  /* Selection/highlight colors (optional). */
  const char *sel_bg = json_get_string(obj, "selection_background");
  if (sel_bg == NULL)
    sel_bg = json_get_string(obj, "selectionBackground");
  if (sel_bg == NULL)
    sel_bg = json_get_string(obj, "highlight");
  if (sel_bg != NULL)
  {
    theme->has_highlight_bg = parse_color(sel_bg, &theme->highlight_bg);
  }

  const char *sel_fg = json_get_string(obj, "selection_foreground");
  if (sel_fg == NULL)
    sel_fg = json_get_string(obj, "selectionForeground");
  if (sel_fg != NULL)
  {
    theme->has_highlight_fg = parse_color(sel_fg, &theme->highlight_fg);
  }

  g_object_unref(parser);

  SK_LOG_INFO(SK_LOG_COMPONENT_TERMINAL, "Loaded theme '%s' from file", theme->name);

  return theme;
}

/* ------------------------------------------------------------------ */
/* Public: free theme                                                  */
/* ------------------------------------------------------------------ */

void
sk_terminal_theme_free(SkTerminalTheme *theme)
{
  if (theme == NULL)
  {
    return;
  }
  g_free(theme->name);
  g_free(theme);
}
