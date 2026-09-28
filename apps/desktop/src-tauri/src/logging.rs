//! tracing to stderr (debug builds) and to `<data_dir>/arcvox.log`.

use std::path::Path;

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub struct LogGuard(#[allow(dead_code)] tracing_appender::non_blocking::WorkerGuard);

pub fn init(data_dir: &Path) -> Option<LogGuard> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,arcvox_desktop=debug,arcvox_core=debug"));
    let appender = tracing_appender::rolling::Builder::new()
        .filename_prefix("arcvox")
        .filename_suffix("log")
        .max_log_files(2)
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .build(data_dir)
        .ok()?;
    let (file_writer, guard) = tracing_appender::non_blocking(appender);
    let file_layer = fmt::layer().with_writer(file_writer).with_ansi(false);
    let registry = tracing_subscriber::registry().with(filter).with(file_layer);
    if cfg!(debug_assertions) {
        registry
            .with(fmt::layer().with_writer(std::io::stderr))
            .init();
    } else {
        registry.init();
    }
    Some(LogGuard(guard))
}
