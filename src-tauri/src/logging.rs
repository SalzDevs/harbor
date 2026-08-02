use std::fs;
use std::path::PathBuf;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Returns the OS-conventional log directory for Harbor:
/// - macOS: `~/Library/Logs/Harbor`
/// - Linux / BSD: `$XDG_STATE_HOME/harbor` (or `~/.local/state/harbor`)
pub fn log_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            return home.join("Library/Logs/Harbor");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if let Ok(xdg_state) = std::env::var("XDG_STATE_HOME") {
            if !xdg_state.trim().is_empty() {
                return PathBuf::from(xdg_state).join("harbor");
            }
        }
        if let Some(home) = dirs::home_dir() {
            return home.join(".local/state/harbor");
        }
    }

    dirs::data_dir()
        .map(|p| p.join("harbor/logs"))
        .unwrap_or_else(|| PathBuf::from("./logs"))
}

/// Initialize tracing with file logging (rotated daily, 7 days max) and stdout.
/// Respects `HARBOR_LOG` env var (e.g. `HARBOR_LOG=debug`), falling back to `RUST_LOG` or `info`.
pub fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let dir = log_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("failed to create log dir {}: {e}", dir.display());
    }

    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .max_log_files(7)
        .filename_prefix("harbor.log")
        .build(&dir);

    let (non_blocking, guard) = match file_appender {
        Ok(appender) => tracing_appender::non_blocking(appender),
        Err(e) => {
            eprintln!("failed to create rolling log appender: {e}");
            return None;
        }
    };

    // Parse HARBOR_LOG (fallback RUST_LOG, then info). Keep HTML-parser crates
    // quiet: they emit DEBUG noise for every tag/comment, which floods the file
    // log during body parsing (tens of MB per session at debug level).
    let env_filter = EnvFilter::try_from_env("HARBOR_LOG")
        .or_else(|_| EnvFilter::try_from_env("RUST_LOG"))
        .unwrap_or_else(|_| EnvFilter::new("info"))
        .add_directive("html5ever=off".parse().unwrap())
        .add_directive("scraper=off".parse().unwrap());

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false);

    let stdout_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stdout);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(stdout_layer)
        .init();

    tracing::info!("Harbor logging initialized at {}", dir.display());

    Some(guard)
}
