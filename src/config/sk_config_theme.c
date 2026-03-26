// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file sk_config_theme.c
 * @brief Terminal theme loading from JSON files (Gogh/base16 compatible).
 *
 * Loads color themes from $XDG_CONFIG_HOME/shellkeep/themes/<name>.json.
 * Supports both Gogh format (color_01..color_16 + foreground/background/cursor)
 * and base16 format (base00..base0F).
 */

#include "shellkeep/sk_config.h"
#include "shellkeep/sk_log.h"

#include <json-glib/json-glib.h>
#include <string.h>

/* ------------------------------------------------------------------ */
/* Color parsing                                                       */
/* ------------------------------------------------------------------ */

/**
 * Parse a hex color string (#RRGGBB or RRGGBB) to 0xRRGGBB.
 * Returns true on success.
 */
static bool
parse_hex_color(const char *str, uint32_t *out)
{
  char *end = NULL;
  unsigned long val;

  if (str == NULL)
    return false;

  /* Skip leading '#' */
  if (str[0] == '#')
    str++;

  if (strlen(str) != 6)
    return false;

  val = strtoul(str, &end, 16);
  if (end == NULL || *end != '\0')
    return false;

  *out = (uint32_t)val;
  return true;
}

/**
 * Read a color from a JSON object by key.
 * Returns true if found and valid.
 */
static bool
json_read_color(JsonObject *obj, const char *key, uint32_t *out)
{
  const char *val;

  if (!json_object_has_member(obj, key))
    return false;

  val = json_object_get_string_member(obj, key);
  return parse_hex_color(val, out);
}

/* ------------------------------------------------------------------ */
/* Default theme — standard terminal colors                            */
/* ------------------------------------------------------------------ */

SkTheme *
sk_theme_new_default(void)
{
  SkTheme *t = g_new0(SkTheme, 1);
  t->name = g_strdup("default");

  /* clang-format off */
  /* Standard xterm-256color default ANSI palette */
  t->ansi_colors[0]  = 0x000000; /* black */
  t->ansi_colors[1]  = 0xCC0000; /* red */
  t->ansi_colors[2]  = 0x4E9A06; /* green */
  t->ansi_colors[3]  = 0xC4A000; /* yellow */
  t->ansi_colors[4]  = 0x3465A4; /* blue */
  t->ansi_colors[5]  = 0x75507B; /* magenta */
  t->ansi_colors[6]  = 0x06989A; /* cyan */
  t->ansi_colors[7]  = 0xD3D7CF; /* white */
  t->ansi_colors[8]  = 0x555753; /* bright black */
  t->ansi_colors[9]  = 0xEF2929; /* bright red */
  t->ansi_colors[10] = 0x8AE234; /* bright green */
  t->ansi_colors[11] = 0xFCE94F; /* bright yellow */
  t->ansi_colors[12] = 0x729FCF; /* bright blue */
  t->ansi_colors[13] = 0xAD7FA8; /* bright magenta */
  t->ansi_colors[14] = 0x34E2E2; /* bright cyan */
  t->ansi_colors[15] = 0xEEEEEC; /* bright white */
  /* clang-format on */

  t->foreground = 0xD3D7CF;
  t->background = 0x2E3436;
  t->cursor_color = 0xD3D7CF;
  t->has_cursor_color = true;

  return t;
}

/* ------------------------------------------------------------------ */
/* Free                                                                */
/* ------------------------------------------------------------------ */

void
sk_theme_free(SkTheme *theme)
{
  if (theme == NULL)
    return;
  g_free(theme->name);
  g_free(theme);
}

/* ------------------------------------------------------------------ */
/* Gogh format parsing                                                 */
/* ------------------------------------------------------------------ */

/**
 * Try to parse Gogh format: color_01..color_16, foreground, background, cursor.
 * Gogh uses "color_01" through "color_16" for the 16 ANSI colors.
 */
static bool
try_parse_gogh(JsonObject *obj, SkTheme *t)
{
  bool found_any = false;

  for (int i = 0; i < 16; i++)
  {
    char key[12];
    uint32_t color;
    g_snprintf(key, sizeof(key), "color_%02d", i + 1);

    if (json_read_color(obj, key, &color))
    {
      t->ansi_colors[i] = color;
      found_any = true;
    }
  }

  if (!found_any)
    return false;

  json_read_color(obj, "foreground", &t->foreground);
  json_read_color(obj, "background", &t->background);
  t->has_cursor_color = json_read_color(obj, "cursor", &t->cursor_color);

  return true;
}

/* ------------------------------------------------------------------ */
/* base16 format parsing                                               */
/* ------------------------------------------------------------------ */

/**
 * Try to parse base16 format: base00..base0F.
 * base16 mapping to ANSI:
 *   base00 = bg, base01..base07 = various UI colors
 *   base08..base0F = ANSI colors (red,orange,yellow,green,cyan,blue,magenta,brown)
 *
 * We use a common mapping:
 *   ANSI 0 (black)   = base00
 *   ANSI 1 (red)     = base08
 *   ANSI 2 (green)   = base0B
 *   ANSI 3 (yellow)  = base0A
 *   ANSI 4 (blue)    = base0D
 *   ANSI 5 (magenta) = base0E
 *   ANSI 6 (cyan)    = base0C
 *   ANSI 7 (white)   = base05
 *   ANSI 8-15 = bright variants (base03, base08..base0F with base05..base07)
 */
static bool
try_parse_base16(JsonObject *obj, SkTheme *t)
{
  uint32_t base[16] = { 0 };
  bool found_any = false;

  for (int i = 0; i < 16; i++)
  {
    char key[8];
    g_snprintf(key, sizeof(key), "base%02X", i);

    if (json_read_color(obj, key, &base[i]))
      found_any = true;
  }

  if (!found_any)
    return false;

  /* Map base16 to ANSI colors */
  /* clang-format off */
  t->ansi_colors[0]  = base[0x00]; /* black */
  t->ansi_colors[1]  = base[0x08]; /* red */
  t->ansi_colors[2]  = base[0x0B]; /* green */
  t->ansi_colors[3]  = base[0x0A]; /* yellow */
  t->ansi_colors[4]  = base[0x0D]; /* blue */
  t->ansi_colors[5]  = base[0x0E]; /* magenta */
  t->ansi_colors[6]  = base[0x0C]; /* cyan */
  t->ansi_colors[7]  = base[0x05]; /* white */
  t->ansi_colors[8]  = base[0x03]; /* bright black */
  t->ansi_colors[9]  = base[0x08]; /* bright red (same as red) */
  t->ansi_colors[10] = base[0x0B]; /* bright green */
  t->ansi_colors[11] = base[0x0A]; /* bright yellow */
  t->ansi_colors[12] = base[0x0D]; /* bright blue */
  t->ansi_colors[13] = base[0x0E]; /* bright magenta */
  t->ansi_colors[14] = base[0x0C]; /* bright cyan */
  t->ansi_colors[15] = base[0x07]; /* bright white */
  /* clang-format on */

  t->foreground = base[0x05];
  t->background = base[0x00];
  t->cursor_color = base[0x05];
  t->has_cursor_color = true;

  return true;
}

/* ------------------------------------------------------------------ */
/* Load theme — public API                                             */
/* ------------------------------------------------------------------ */

SkTheme *
sk_theme_load(const char *name, GError **error)
{
  char *dir;
  char *filename;
  char *path;
  JsonParser *parser;
  JsonNode *root_node;
  JsonObject *obj;
  SkTheme *t;

  if (name == NULL || name[0] == '\0')
  {
    g_set_error(error, SK_ERROR, SK_ERROR_GENERIC, "Theme name is empty");
    return NULL;
  }

  /* Build path: $XDG_CONFIG_HOME/shellkeep/themes/<name>.json */
  dir = sk_config_get_dir();
  filename = g_strdup_printf("%s.json", name);
  path = g_build_filename(dir, "themes", filename, NULL);
  g_free(filename);
  g_free(dir);

  if (!g_file_test(path, G_FILE_TEST_EXISTS))
  {
    g_set_error(error, SK_ERROR, SK_ERROR_IO, "Theme file not found: %s", path);
    g_free(path);
    return NULL;
  }

  /* Parse JSON */
  parser = json_parser_new();
  if (!json_parser_load_from_file(parser, path, error))
  {
    SK_LOG_ERROR(SK_LOG_COMPONENT_GENERAL, "config: cannot parse theme file %s", path);
    g_object_unref(parser);
    g_free(path);
    return NULL;
  }

  root_node = json_parser_get_root(parser);
  if (root_node == NULL || !JSON_NODE_HOLDS_OBJECT(root_node))
  {
    g_set_error(error, SK_ERROR, SK_ERROR_GENERIC, "Theme file %s does not contain a JSON object",
                path);
    g_object_unref(parser);
    g_free(path);
    return NULL;
  }

  obj = json_node_get_object(root_node);

  /* Start with defaults, then overlay */
  t = sk_theme_new_default();
  g_free(t->name);
  t->name = g_strdup(name);

  /* Try Gogh format first, then base16 */
  if (!try_parse_gogh(obj, t))
  {
    if (!try_parse_base16(obj, t))
    {
      SK_LOG_WARN(SK_LOG_COMPONENT_GENERAL,
                  "config: theme %s has unrecognized format, "
                  "using default colors",
                  name);
    }
  }

  /* Also try direct "foreground"/"background"/"cursor" at top level
   * (some theme formats have these regardless of color key naming) */
  json_read_color(obj, "foreground", &t->foreground);
  json_read_color(obj, "background", &t->background);
  if (json_read_color(obj, "cursor", &t->cursor_color))
    t->has_cursor_color = true;

  g_object_unref(parser);
  g_free(path);

  SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "config: loaded theme '%s'", name);

  return t;
}
