// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! shellkeep — SSH terminal manager.
//!
//! Persistent sessions that survive everything.
//! Open source. Cross-platform. Zero server setup.

mod app;
mod cli;
mod instance;
mod theme;

pub(crate) use app::ShellKeep;

use iced::{Point, Size, window};
use shellkeep::i18n;

// Re-export for view layer
pub(crate) use app::update::RENAME_INPUT_ID;

fn main() -> iced::Result {
    let args: Vec<String> = std::env::args().collect();

    // Handle --version and --help before initializing anything
    for arg in &args[1..] {
        match arg.as_str() {
            "--crash-report" => {
                let dir = shellkeep::crash::crash_dir();
                if dir.exists() {
                    match std::fs::read_dir(&dir) {
                        Ok(entries) => {
                            let mut files: Vec<_> = entries
                                .filter_map(|e| e.ok())
                                .filter(|e| e.path().extension().is_some_and(|ext| ext == "txt"))
                                .collect();
                            files.sort_by_key(|e| e.path());
                            if files.is_empty() {
                                println!("No crash dumps found.");
                            } else {
                                println!("Crash dumps in {}:", dir.display());
                                for f in &files {
                                    println!("  {}", f.path().display());
                                }
                                // Show the latest one
                                if let Some(latest) = files.last() {
                                    println!(
                                        "\nLatest:\n{}",
                                        std::fs::read_to_string(latest.path()).unwrap_or_default()
                                    );
                                }
                            }
                        }
                        Err(_) => println!("No crash dumps found."),
                    }
                } else {
                    println!("No crash dumps found.");
                }
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("shellkeep {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!(
                    "shellkeep {} — SSH sessions that survive everything\n\n\
                     Usage: shellkeep [user@]host [-p port] [-i identity] [-l user]\n\
                     \n\
                     Options:\n  \
                       -p PORT          SSH port (default: 22)\n  \
                       -i FILE          Identity file (private key)\n  \
                       -l USER          Login user name\n  \
                       --debug          Enable debug logging\n  \
                       --crash-report   Show crash dumps from previous runs\n  \
                       --version        Show version\n  \
                       --help           Show this help\n\
                     \n\
                     Without arguments, opens the welcome screen.\n\
                     https://github.com/shellkeep/shellkeep",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            _ => {}
        }
    }

    let log_level = if args.iter().any(|a| a == "--trace") {
        "trace"
    } else if args.iter().any(|a| a == "--debug") {
        "debug"
    } else {
        "info"
    };

    // Set up logging — stderr + optional file
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

    // Try to also log to file
    let log_dir = dirs::state_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("shellkeep")
        .join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("shellkeep.log");

    // NFR-OBS-04: rotate log if it exceeds 10 MB
    shellkeep::crash::rotate_logs(&log_path);
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        let file_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::sync::Mutex::new(file))
            .with_ansi(false);
        let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);

        tracing_subscriber::registry()
            .with(filter)
            .with(file_layer)
            .with(stderr_layer)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    // Parse SSH args (skip --debug which is shellkeep-specific)
    let initial_ssh_args = cli::parse_cli_ssh_args(&args[1..]);

    tracing::info!("shellkeep v{} starting", env!("CARGO_PKG_VERSION"));

    // NFR-I18N-07: detect and initialize locale
    let locale = i18n::detect_locale();
    i18n::init(&locale);
    tracing::info!("locale: {locale}");

    // NFR-SEC-10: disable core dumps
    shellkeep::crash::disable_core_dumps();

    // NFR-OBS-09: install crash handler
    shellkeep::crash::install_panic_hook();

    // NFR-SEC-03: verify and fix file permissions on startup
    shellkeep::state::permissions::verify_and_fix();

    // FR-CLI-04: single instance detection
    let _pid_guard = match instance::check_single_instance() {
        Some(guard) => guard,
        None => {
            eprintln!("shellkeep is already running (another instance detected)");
            std::process::exit(0);
        }
    };

    // FR-STATE-14: window geometry will be restored from server state after connect
    let saved_window: Option<shellkeep::state::state_file::WindowGeometry> = None;

    // Build initial window settings from saved geometry
    let mut window_settings = window::Settings::default();
    if let Some(ref geo) = saved_window {
        window_settings.size = Size::new(geo.width as f32, geo.height as f32);
        if let (Some(x), Some(y)) = (geo.x, geo.y) {
            window_settings.position = window::Position::Specific(Point::new(x as f32, y as f32));
        }
    } else {
        window_settings.size = Size::new(900.0, 600.0);
    }

    // Use daemon builder for multi-window support. The first window is
    // opened inside ShellKeep::new() via window::open().
    let initial_window_settings = window_settings;
    iced::daemon(
        move || ShellKeep::new(initial_ssh_args.clone(), initial_window_settings.clone()),
        ShellKeep::update,
        ShellKeep::view,
    )
    .title(ShellKeep::title)
    .subscription(ShellKeep::subscription)
    .theme(ShellKeep::theme)
    .antialiasing(true)
    .run()
}
