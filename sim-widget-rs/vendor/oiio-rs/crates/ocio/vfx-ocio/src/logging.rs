//! OCIO-compatible logging API.
//!
//! Provides logging functions for OCIO parity. In Rust, logging is typically handled
//! by the `log` crate, but this module provides OCIO-compatible API for compatibility.
//!
//! # Example
//!
//! ```
//! use vfx_ocio::{LoggingLevel, set_logging_level, log_message};
//!
//! set_logging_level(LoggingLevel::Warning);
//! log_message(LoggingLevel::Warning, "Config validation warning");
//! ```

use std::sync::{Mutex, OnceLock};

/// Logging levels matching OCIO's LoggingLevel enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(u8)]
pub enum LoggingLevel {
    /// No logging.
    None = 0,
    /// Only errors.
    Warning = 1,
    /// Errors and warnings.
    #[default]
    Info = 2,
    /// All messages.
    Debug = 3,
    /// Unknown/undefined level.
    Unknown = 255,
}

impl From<u8> for LoggingLevel {
    fn from(value: u8) -> Self {
        match value {
            0 => LoggingLevel::None,
            1 => LoggingLevel::Warning,
            2 => LoggingLevel::Info,
            3 => LoggingLevel::Debug,
            _ => LoggingLevel::Unknown,
        }
    }
}

/// Type alias for custom logging function.
///
/// The function receives the logging level and message string.
pub type LoggingFunction = Box<dyn Fn(LoggingLevel, &str) + Send + Sync>;

/// Global logging state.
struct LoggingState {
    level: LoggingLevel,
    custom_fn: Option<LoggingFunction>,
}

impl Default for LoggingState {
    fn default() -> Self {
        Self {
            level: LoggingLevel::Info,
            custom_fn: None,
        }
    }
}

static LOGGING_STATE: OnceLock<Mutex<LoggingState>> = OnceLock::new();

fn state() -> &'static Mutex<LoggingState> {
    LOGGING_STATE.get_or_init(|| Mutex::new(LoggingState::default()))
}

/// Gets the current global logging level.
///
/// OCIO parity: `GetLoggingLevel()`.
pub fn get_logging_level() -> LoggingLevel {
    state().lock().unwrap().level
}

/// Sets the global logging level.
///
/// OCIO parity: `SetLoggingLevel()`.
pub fn set_logging_level(level: LoggingLevel) {
    state().lock().unwrap().level = level;
}

/// Sets a custom logging function.
///
/// OCIO parity: `SetLoggingFunction()`.
///
/// # Arguments
///
/// * `func` - Custom logging function that receives level and message.
///
/// # Example
///
/// ```
/// use vfx_ocio::{LoggingLevel, set_logging_function};
///
/// set_logging_function(Box::new(|level, msg| {
///     eprintln!("[OCIO {:?}] {}", level, msg);
/// }));
/// ```
pub fn set_logging_function(func: LoggingFunction) {
    state().lock().unwrap().custom_fn = Some(func);
}

/// Resets to the default logging function (`tracing` events).
///
/// OCIO parity: `ResetToDefaultLoggingFunction()`.
pub fn reset_logging_function() {
    state().lock().unwrap().custom_fn = None;
}

/// Logs a message using the current logging function.
///
/// OCIO parity: `LogMessage()`.
///
/// Messages are only output if the message level is at or below the current
/// global logging level.
///
/// # Arguments
///
/// * `level` - The logging level for this message.
/// * `message` - The message to log.
pub fn log_message(level: LoggingLevel, message: &str) {
    let state = state().lock().unwrap();

    // Check if message should be logged based on current level
    if level > state.level {
        return;
    }

    if let Some(ref func) = state.custom_fn {
        func(level, message);
    } else {
        // Default: a `tracing` event (target `vfx_ocio`), where OCIO writes to stderr. A library
        // does not own the process's stderr; the application's subscriber decides what is shown
        // (vfx-cli prints warnings).
        match level {
            LoggingLevel::None => {}
            LoggingLevel::Warning => tracing::warn!("{message}"),
            LoggingLevel::Info => tracing::info!("{message}"),
            LoggingLevel::Debug | LoggingLevel::Unknown => tracing::debug!("{message}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// The logger is process-global and the test harness runs these three in parallel: each
    /// one resets the level/function the others are asserting on, so they failed at random
    /// (seen: both counting tests at once). They now take this lock, and the counting callbacks
    /// only count the messages the test itself sent, so another test logging concurrently
    /// cannot bump the count either.
    static GLOBAL_LOGGER: Mutex<()> = Mutex::new(());

    #[test]
    fn test_logging_level() {
        let _guard = GLOBAL_LOGGER.lock().unwrap_or_else(|e| e.into_inner());
        reset_logging_function();
        set_logging_level(LoggingLevel::Info);

        set_logging_level(LoggingLevel::Warning);
        assert_eq!(get_logging_level(), LoggingLevel::Warning);

        set_logging_level(LoggingLevel::Debug);
        assert_eq!(get_logging_level(), LoggingLevel::Debug);

        // Reset to default
        set_logging_level(LoggingLevel::Info);
    }

    #[test]
    fn test_custom_logging_function() {
        let _guard = GLOBAL_LOGGER.lock().unwrap_or_else(|e| e.into_inner());
        reset_logging_function();
        set_logging_level(LoggingLevel::Info);

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        set_logging_function(Box::new(move |_level, msg| {
            if msg == "test message" || msg == "debug message" {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }
        }));

        set_logging_level(LoggingLevel::Debug);
        log_message(LoggingLevel::Info, "test message");
        log_message(LoggingLevel::Debug, "debug message");

        assert_eq!(counter.load(Ordering::SeqCst), 2);

        // Reset
        reset_logging_function();
        set_logging_level(LoggingLevel::Info);
    }

    #[test]
    fn test_level_filtering() {
        let _guard = GLOBAL_LOGGER.lock().unwrap_or_else(|e| e.into_inner());
        reset_logging_function();
        set_logging_level(LoggingLevel::Info);

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        set_logging_function(Box::new(move |_level, msg| {
            if msg == "info message" || msg == "warning message" {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }
        }));

        set_logging_level(LoggingLevel::Warning);

        // This should NOT be logged (Info > Warning)
        log_message(LoggingLevel::Info, "info message");
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // This SHOULD be logged (Warning <= Warning)
        log_message(LoggingLevel::Warning, "warning message");
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Reset
        reset_logging_function();
        set_logging_level(LoggingLevel::Info);
    }
}
