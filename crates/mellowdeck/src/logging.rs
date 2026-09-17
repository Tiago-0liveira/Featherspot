use std::{fs, path::Path, process};

use mellowdeck_core::{AppError, ErrorKind, Result};
use tracing_appender::non_blocking::WorkerGuard;

pub fn initialize(directory: &Path) -> Result<WorkerGuard> {
    fs::create_dir_all(directory).map_err(io_error)?;
    let file = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(format!("mellowdeck-{}", process::id()))
        .filename_suffix("log")
        .max_log_files(14)
        .build(directory)
        .map_err(|error| AppError::new(ErrorKind::Storage, error.to_string()))?;
    let (writer, guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_target(true)
        .with_max_level(tracing::Level::INFO)
        .with_writer(writer)
        .try_init()
        .map_err(|error| AppError::new(ErrorKind::Unexpected, error.to_string()))?;
    Ok(guard)
}

fn io_error(error: std::io::Error) -> AppError {
    let message = error.to_string();
    drop(error);
    AppError::new(ErrorKind::Storage, message)
}
