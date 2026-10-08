//! tracing to stderr (debug builds) and to `<data_dir>/arcvox.log`.

use std::path::Path;

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub struct LogGuard(#[allow(dead_code)] tracing_appender::non_blocking::WorkerGuard);

/// Holds the guard for the life of the process, so that it can be dropped,
/// and the lines still buffered written, before the process is ended.
pub struct LogSlot(pub std::sync::Mutex<Option<LogGuard>>);

impl LogSlot {
    /// Writes the lines still buffered. The log file takes nothing after this.
    pub fn close(&self) {
        drop(self.0.lock().unwrap_or_else(|e| e.into_inner()).take());
    }
}

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
    log_panics();
    Some(LogGuard(guard))
}

/// A panic on any thread goes to the log file with its place and its
/// backtrace; a thread that ends this way leaves no other trace.
fn log_panics() {
    let earlier = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        tracing::error!(
            thread = thread.name().unwrap_or("unnamed"),
            backtrace = %std::backtrace::Backtrace::force_capture(),
            "panic: {info}"
        );
        earlier(info);
    }));
}
