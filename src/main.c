// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * @file main.c
 * @brief shellkeep application entry point.
 *
 * GtkApplication skeleton with single-instance (FR-CLI-04, FR-CLI-05),
 * command-line argument parsing (FR-CLI-01, FR-CLI-03), logging
 * initialisation, and placeholder connect flow.
 */

#include "shellkeep/sk_config.h"
#include "shellkeep/sk_i18n.h"
#include "shellkeep/sk_log.h"
#include "shellkeep/sk_state.h"
#include "shellkeep/sk_types.h"

#include <gtk/gtk.h>

#include <locale.h>
#include <stdlib.h>
#include <string.h>
#ifdef HAVE_PRCTL
#include <sys/prctl.h>
#endif

/* ------------------------------------------------------------------ */
/* Command-line option storage                                         */
/* ------------------------------------------------------------------ */

typedef struct
{
  char *host;            /* user@host or just host */
  char *user;            /* -l user */
  char *identity_file;   /* -i identity_file */
  int port;              /* -p port (0 = default) */
  char *config_path;     /* --config <path> */
  char *debug_component; /* --debug[=COMPONENT] */
  gboolean debug;        /* --debug flag */
  gboolean trace;        /* --trace flag */
  gboolean minimized;    /* --minimized */
  gboolean crash_report; /* --crash-report */
  gboolean version;      /* --version */
} SkCliOptions;

static SkCliOptions cli_opts = { 0 };

/* ------------------------------------------------------------------ */
/* Forward declarations                                                */
/* ------------------------------------------------------------------ */

static void on_startup(GtkApplication *app, gpointer user_data);
static void on_activate(GtkApplication *app, gpointer user_data);
static int on_command_line(GtkApplication *app, GApplicationCommandLine *cmdline,
                           gpointer user_data);
static void on_shutdown(GtkApplication *app, gpointer user_data);
static void parse_host_string(const char *host_str);

/* ------------------------------------------------------------------ */
/* GOptionEntry definitions — FR-CLI-01, FR-CLI-03                     */
/* ------------------------------------------------------------------ */

/* clang-format off */
static GOptionEntry cli_entries[] = {
  { "port",         'p', 0, G_OPTION_ARG_INT,      &cli_opts.port,
    "SSH port",                                      "PORT" },
  { "identity",     'i', 0, G_OPTION_ARG_FILENAME,  &cli_opts.identity_file,
    "Identity file (private key)",                   "FILE" },
  { "login",        'l', 0, G_OPTION_ARG_STRING,    &cli_opts.user,
    "Login user name",                               "USER" },
  { "debug",         0,  G_OPTION_FLAG_OPTIONAL_ARG,
    G_OPTION_ARG_CALLBACK, NULL, /* set in main() */
    "Enable debug logging [for COMPONENT]",          "COMPONENT" },
  { "trace",         0,  G_OPTION_FLAG_OPTIONAL_ARG,
    G_OPTION_ARG_CALLBACK, NULL, /* set in main() */
    "Enable trace logging [for COMPONENT]",          "COMPONENT" },
  { "config",        0,  0, G_OPTION_ARG_FILENAME,  &cli_opts.config_path,
    "Configuration file path",                       "PATH" },
  { "minimized",     0,  0, G_OPTION_ARG_NONE,      &cli_opts.minimized,
    "Start minimized to system tray",                NULL },
  { "crash-report",  0,  0, G_OPTION_ARG_NONE,      &cli_opts.crash_report,
    "Show crash report from previous run",           NULL },
  { "version",       0,  0, G_OPTION_ARG_NONE,      &cli_opts.version,
    "Show version and exit",                         NULL },
  { NULL, 0, 0, 0, NULL, NULL, NULL }
};
/* clang-format on */

/* ------------------------------------------------------------------ */
/* Callback helpers for --debug/--trace optional-arg parsing           */
/* ------------------------------------------------------------------ */

static gboolean
on_debug_option(const gchar *option_name G_GNUC_UNUSED, const gchar *value,
                gpointer data G_GNUC_UNUSED, GError **error G_GNUC_UNUSED)
{
  cli_opts.debug = TRUE;
  g_free(cli_opts.debug_component);
  cli_opts.debug_component = g_strdup(value); /* NULL is fine */
  return TRUE;
}

static gboolean
on_trace_option(const gchar *option_name G_GNUC_UNUSED, const gchar *value,
                gpointer data G_GNUC_UNUSED, GError **error G_GNUC_UNUSED)
{
  cli_opts.trace = TRUE;
  g_free(cli_opts.debug_component);
  cli_opts.debug_component = g_strdup(value);
  return TRUE;
}

/* ------------------------------------------------------------------ */
/* Application callbacks                                               */
/* ------------------------------------------------------------------ */

/**
 * Called once when the primary instance starts up, after GTK and GLib
 * are fully initialised.  This is the safe place to touch XDG paths.
 *
 * NFR-SEC-03: verify and fix permissions on all shellkeep directories
 * and files at startup.  Must happen before any user-facing file I/O
 * but after GLib/GTK init so that XDG path helpers work reliably.
 */
static void
on_startup(GtkApplication *app G_GNUC_UNUSED, gpointer user_data G_GNUC_UNUSED)
{
  /* NFR-SEC-03: best-effort permission fix.  Failure is non-fatal so
   * that the application can still start in restricted environments
   * (CI, containers, read-only filesystems). */
  (void)sk_permissions_verify_and_fix();
}

/**
 * Called when the application is activated (no command line arguments,
 * or a second instance brings the primary to the foreground).
 * FR-CLI-04: single-instance via GtkApplication / D-Bus.
 */
static void
on_activate(GtkApplication *app, gpointer user_data G_GNUC_UNUSED)
{
  GtkWindow *win;
  GtkWidget *window;
  GtkWidget *label;

  SK_LOG_INFO(SK_LOG_COMPONENT_UI, "application activated");

  /* If we already have a window, just present it */
  win = gtk_application_get_active_window(app);
  if (win != NULL)
  {
    gtk_window_present(win);
    return;
  }

  /* Create a placeholder window.
   * TODO: implement full window creation with SkWindow when UI layer
   * is ready. */
  window = gtk_application_window_new(app);
  gtk_window_set_title(GTK_WINDOW(window), _("shellkeep"));
  gtk_window_set_default_size(GTK_WINDOW(window), 800, 600);

  if (cli_opts.minimized)
  {
    gtk_window_iconify(GTK_WINDOW(window));
  }

  /* Placeholder: show a label until the connection flow is wired up */
  if (cli_opts.host != NULL)
  {
    char *text = g_strdup_printf("Connecting to %s%s%s:%d ...", cli_opts.user ? cli_opts.user : "",
                                 cli_opts.user ? "@" : "", cli_opts.host,
                                 cli_opts.port > 0 ? cli_opts.port : 22);
    label = gtk_label_new(text);
    g_free(text);
  }
  else
  {
    label = gtk_label_new(_("shellkeep — no host specified.\n"
                            "Usage: shellkeep [options] [user@]host"));
  }

  gtk_container_add(GTK_CONTAINER(window), label);
  gtk_widget_show_all(window);
}

/**
 * Handle command-line arguments.
 * FR-CLI-01: Accept user@host, -p, -i, -l like ssh.
 * FR-CLI-04: Single-instance — second invocation sends args to primary.
 */
static int
on_command_line(GtkApplication *app, GApplicationCommandLine *cmdline,
                gpointer user_data G_GNUC_UNUSED)
{
  int argc = 0;
  char **argv = g_application_command_line_get_arguments(cmdline, &argc);
  GOptionContext *ctx;
  GError *error = NULL;

  /* Reset mutable options for this invocation */
  g_free(cli_opts.host);
  cli_opts.host = NULL;
  g_free(cli_opts.user);
  cli_opts.user = NULL;
  g_free(cli_opts.identity_file);
  cli_opts.identity_file = NULL;
  cli_opts.port = 0;
  cli_opts.debug = FALSE;
  cli_opts.trace = FALSE;
  cli_opts.minimized = FALSE;
  cli_opts.crash_report = FALSE;
  cli_opts.version = FALSE;

  ctx = g_option_context_new("[user@]host");
  g_option_context_add_main_entries(ctx, cli_entries, NULL);

  {
    GOptionGroup *gtk_group = gtk_get_option_group(FALSE);
    if (gtk_group != NULL)
      g_option_context_add_group(ctx, gtk_group);
  }

  if (!g_option_context_parse(ctx, &argc, &argv, &error))
  {
    g_application_command_line_printerr(cmdline, "Error: %s\n", error->message);
    g_error_free(error);
    g_option_context_free(ctx);
    g_strfreev(argv);
    return 1;
  }
  g_option_context_free(ctx);

  /* --version: print and exit */
  if (cli_opts.version)
  {
    g_application_command_line_print(cmdline, "shellkeep %s\n", SK_VERSION_STRING);
    g_strfreev(argv);
    return 0;
  }

  /* --crash-report: show previous crash info and exit */
  if (cli_opts.crash_report)
  {
    if (sk_crash_has_previous_dumps())
    {
      char *dir = sk_crash_get_dir();
      g_application_command_line_print(cmdline, "Crash dumps found in: %s\n", dir);
      g_free(dir);
    }
    else
    {
      g_application_command_line_print(cmdline, "No crash dumps from previous runs.\n");
    }
    g_strfreev(argv);
    return 0;
  }

  /* Positional argument: [user@]host — FR-CLI-01 */
  if (argc > 1)
  {
    parse_host_string(argv[1]);
  }

  g_strfreev(argv);

  /* Initialise logging with CLI flags — FR-CLI-03, FR-CLI-04 */
  sk_log_init(cli_opts.debug, cli_opts.trace, cli_opts.debug_component);

  if (cli_opts.host != NULL)
  {
    SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "target host=%s port=%d user=%s", cli_opts.host,
                cli_opts.port > 0 ? cli_opts.port : 22,
                cli_opts.user ? cli_opts.user : "(default)");
  }

  /* Activate the application (creates or presents window) */
  g_application_activate(G_APPLICATION(app));

  return 0;
}

/**
 * Parse "user@host" into separate user and host fields.
 * FR-CLI-01: Accept same parameters as SSH.
 */
static void
parse_host_string(const char *host_str)
{
  const char *at;

  if (host_str == NULL)
    return;

  at = strchr(host_str, '@');
  if (at != NULL)
  {
    /* user@host format */
    if (cli_opts.user == NULL)
    {
      cli_opts.user = g_strndup(host_str, (gsize)(at - host_str));
    }
    cli_opts.host = g_strdup(at + 1);
  }
  else
  {
    cli_opts.host = g_strdup(host_str);
  }
}

/**
 * Clean shutdown: flush logs, free resources.
 */
static void
on_shutdown(GtkApplication *app G_GNUC_UNUSED, gpointer user_data G_GNUC_UNUSED)
{
  SK_LOG_INFO(SK_LOG_COMPONENT_GENERAL, "application shutting down");
  sk_log_shutdown();

  g_free(cli_opts.host);
  g_free(cli_opts.user);
  g_free(cli_opts.identity_file);
  g_free(cli_opts.config_path);
  g_free(cli_opts.debug_component);
  memset(&cli_opts, 0, sizeof(cli_opts));
}

/* ------------------------------------------------------------------ */
/* Entry point                                                         */
/* ------------------------------------------------------------------ */

int
main(int argc, char *argv[])
{
  GOptionEntry *entries;
  int i;
  GtkApplication *app;
  int status;

  /* NFR-SEC-10: Disable core dumps early to prevent leaking sensitive
   * memory (passwords, keys). This is also set in sk_crash_handler_install()
   * but we do it as early as possible for defense-in-depth. */
#ifdef HAVE_PRCTL
  prctl(PR_SET_DUMPABLE, 0);
#endif

  /* NFR-I18N-06: Initialize gettext */
  setlocale(LC_ALL, "");
  bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR);
  bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8");
  textdomain(GETTEXT_PACKAGE);

  /* Wire up --debug and --trace callback handlers.
   * We need to do this at runtime because GOptionEntry's arg_data
   * for G_OPTION_ARG_CALLBACK must be a function pointer, and we
   * cannot assign that in the static initialiser portably in C11. */
  /* The entries array is const, so we cast to set the callback.
   * This is safe because GLib only reads these during parse. */
  entries = (GOptionEntry *)(void *)cli_entries;
  for (i = 0; entries[i].long_name != NULL; i++)
  {
    if (g_strcmp0(entries[i].long_name, "debug") == 0)
    {
      union
      {
        GOptionArgFunc fn;
        void *ptr;
      } u;
      u.fn = on_debug_option;
      entries[i].arg_data = u.ptr;
    }
    else if (g_strcmp0(entries[i].long_name, "trace") == 0)
    {
      union
      {
        GOptionArgFunc fn;
        void *ptr;
      } u;
      u.fn = on_trace_option;
      entries[i].arg_data = u.ptr;
    }
  }

  /* NFR-ARCH-09, FR-CLI-04, FR-CLI-05:
   * GtkApplication provides single-instance via D-Bus and
   * G_APPLICATION_HANDLES_COMMAND_LINE routes args to the primary.
   *
   * G_APPLICATION_NON_UNIQUE is added so the application can start
   * even when D-Bus session bus is unavailable (e.g., headless / CI
   * environments).  Single-instance enforcement still works when
   * D-Bus is present — GLib only falls back to non-unique when
   * registration fails. */
  app = gtk_application_new(SK_APPLICATION_ID,
                            G_APPLICATION_HANDLES_COMMAND_LINE | G_APPLICATION_NON_UNIQUE);

  g_signal_connect(app, "startup", G_CALLBACK(on_startup), NULL);
  g_signal_connect(app, "activate", G_CALLBACK(on_activate), NULL);
  g_signal_connect(app, "command-line", G_CALLBACK(on_command_line), NULL);
  g_signal_connect(app, "shutdown", G_CALLBACK(on_shutdown), NULL);

  status = g_application_run(G_APPLICATION(app), argc, argv);
  g_object_unref(app);

  return status;
}
