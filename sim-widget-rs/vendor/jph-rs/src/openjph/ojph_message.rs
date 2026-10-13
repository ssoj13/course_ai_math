//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/openjph/ojph_message.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/others/ojph_message.cpp`

use std::fmt;
use std::io::{self, Write};
use std::sync::{Arc, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum OjphMsgLevel {
    OjphMsgAllMsg = 0,
    OjphMsgInfo = 1,
    OjphMsgWarn = 2,
    OjphMsgError = 3,
    OjphMsgNoMsg = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageStream {
    Stdout,
    Stderr,
}

pub trait MessageBase: Send + Sync {
    fn call(&self, warn_code: i32, file_name: &str, line_num: i32, message: &str);
}

pub trait MessageInfo: MessageBase {}
pub trait MessageWarning: MessageBase {}
pub trait MessageError: MessageBase {}

/// Typed error mirroring C++'s `OJPH_ERROR` exception: a numeric OpenJPH error code (as used
/// throughout the port, e.g. `0x00030072`) plus the formatted message. Where C++ throws, the 1:1
/// port returns this via `Result` instead — see `ojph_error!` and `error_value`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use = "an OjphError must be propagated (`return Err(..)`/`?`) — dropping it silently \
              discards what used to be a hard `ojph_error!` panic"]
pub struct OjphError {
    /// OpenJPH numeric error code (as passed to `ojph_error!` / the C++ `OJPH_ERROR` macro).
    pub code: i32,
    /// Formatted diagnostic text (already includes the same detail the C++ message would carry).
    pub message: String,
}

impl fmt::Display for OjphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ojph error 0x{:08X}: {}", self.code, self.message)
    }
}

impl std::error::Error for OjphError {}

/// Convenience alias used throughout the port for fallible operations that mirror a C++
/// `OJPH_ERROR`-throwing path.
pub type OjphResult<T> = Result<T, OjphError>;

#[derive(Default)]
pub struct DefaultMessageInfo;

#[derive(Default)]
pub struct DefaultMessageWarning;

#[derive(Default)]
pub struct DefaultMessageError;

impl MessageBase for DefaultMessageInfo {
    fn call(&self, info_code: i32, file_name: &str, line_num: i32, message: &str) {
        let state = read_state();
        if state.info_stream.is_none() || state.message_level > OjphMsgLevel::OjphMsgInfo {
            return;
        }

        emit_to_stream(
            state.info_stream,
            "ojph info",
            info_code,
            file_name,
            line_num,
            message,
        );
    }
}

impl MessageInfo for DefaultMessageInfo {}

impl MessageBase for DefaultMessageWarning {
    fn call(&self, warn_code: i32, file_name: &str, line_num: i32, message: &str) {
        let state = read_state();
        if state.warning_stream.is_none() || state.message_level > OjphMsgLevel::OjphMsgWarn {
            return;
        }

        emit_to_stream(
            state.warning_stream,
            "ojph warning",
            warn_code,
            file_name,
            line_num,
            message,
        );
    }
}

impl MessageWarning for DefaultMessageWarning {}

impl MessageBase for DefaultMessageError {
    fn call(&self, error_code: i32, file_name: &str, line_num: i32, message: &str) {
        let state = read_state();
        if state.error_stream.is_some() && state.message_level <= OjphMsgLevel::OjphMsgError {
            emit_to_stream(
                state.error_stream,
                "ojph error",
                error_code,
                file_name,
                line_num,
                message,
            );
        }
        // No panic here: unlike C++'s `OJPH_ERROR` (which throws right after invoking the message
        // handler), the Rust port reports failure via `Result` (see `error_value` / `ojph_error!`).
        // This handler is purely the logging side effect; the caller decides what to do with the
        // `Err` that `ojph_error!` produces.
    }
}

impl MessageError for DefaultMessageError {}

struct MessageState {
    info_stream: Option<MessageStream>,
    warning_stream: Option<MessageStream>,
    error_stream: Option<MessageStream>,
    info: Arc<dyn MessageInfo>,
    warning: Arc<dyn MessageWarning>,
    error: Arc<dyn MessageError>,
    message_level: OjphMsgLevel,
}

static MESSAGE_STATE: OnceLock<RwLock<MessageState>> = OnceLock::new();

fn state() -> &'static RwLock<MessageState> {
    MESSAGE_STATE.get_or_init(|| {
        RwLock::new(MessageState {
            info_stream: Some(MessageStream::Stdout),
            warning_stream: Some(MessageStream::Stdout),
            error_stream: Some(MessageStream::Stderr),
            info: Arc::new(DefaultMessageInfo),
            warning: Arc::new(DefaultMessageWarning),
            error: Arc::new(DefaultMessageError),
            message_level: OjphMsgLevel::OjphMsgAllMsg,
        })
    })
}

#[inline]
fn read_state() -> RwLockReadGuard<'static, MessageState> {
    match state().read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[inline]
fn write_state() -> RwLockWriteGuard<'static, MessageState> {
    match state().write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn emit_to_stream(
    stream: Option<MessageStream>,
    prefix: &str,
    code: i32,
    file_name: &str,
    line_num: i32,
    message: &str,
) {
    match stream {
        Some(MessageStream::Stdout) => {
            let mut out = io::stdout().lock();
            let _ = writeln!(
                out,
                "{prefix} 0x{code:08X} at {file_name}:{line_num}: {message}"
            );
        }
        Some(MessageStream::Stderr) => {
            let mut out = io::stderr().lock();
            let _ = writeln!(
                out,
                "{prefix} 0x{code:08X} at {file_name}:{line_num}: {message}"
            );
        }
        None => {}
    }
}

#[inline]
pub fn configure_info(info: Arc<dyn MessageInfo>) {
    write_state().info = info;
}

#[inline]
pub fn get_info() -> Arc<dyn MessageInfo> {
    read_state().info.clone()
}

#[inline]
pub fn set_info_stream(s: Option<MessageStream>) {
    write_state().info_stream = s;
}

#[inline]
pub fn configure_warning(warn: Arc<dyn MessageWarning>) {
    write_state().warning = warn;
}

#[inline]
pub fn get_warning() -> Arc<dyn MessageWarning> {
    read_state().warning.clone()
}

#[inline]
pub fn set_warning_stream(s: Option<MessageStream>) {
    write_state().warning_stream = s;
}

#[inline]
pub fn configure_error(error: Arc<dyn MessageError>) {
    write_state().error = error;
}

#[inline]
pub fn get_error() -> Arc<dyn MessageError> {
    read_state().error.clone()
}

#[inline]
pub fn set_error_stream(s: Option<MessageStream>) {
    write_state().error_stream = s;
}

#[inline]
pub fn set_message_level(level: OjphMsgLevel) {
    debug_assert!(
        (OjphMsgLevel::OjphMsgAllMsg as i32..=OjphMsgLevel::OjphMsgNoMsg as i32)
            .contains(&(level as i32))
    );
    write_state().message_level = level;
}

#[inline]
pub fn ojph_file_name(file_name: &str) -> &str {
    file_name.rsplit(['/', '\\']).next().unwrap_or(file_name)
}

#[inline]
pub fn info_message(code: i32, file_name: &str, line_num: i32, args: fmt::Arguments<'_>) {
    get_info().call(code, file_name, line_num, &args.to_string());
}

#[inline]
pub fn warn_message(code: i32, file_name: &str, line_num: i32, args: fmt::Arguments<'_>) {
    get_warning().call(code, file_name, line_num, &args.to_string());
}

/// Logs through the configured error handler (same side effect C++'s `OJPH_ERROR` gets before it
/// throws) and returns the typed [`OjphError`] instead of panicking/unwinding. This is what
/// `ojph_error!` expands to; every caller propagates the result with `?`.
#[inline]
pub fn error_value(code: i32, file_name: &str, line_num: i32, args: fmt::Arguments<'_>) -> OjphError {
    let message = args.to_string();
    get_error().call(code, file_name, line_num, &message);
    OjphError { code, message }
}

#[macro_export]
macro_rules! ojph_info {
    ($t:expr, $($arg:tt)*) => {{
        $crate::openjph::ojph_message::info_message(
            $t,
            $crate::openjph::ojph_message::ojph_file_name(file!()),
            line!() as i32,
            format_args!($($arg)*),
        );
    }};
}

#[macro_export]
macro_rules! ojph_warn {
    ($t:expr, $($arg:tt)*) => {{
        $crate::openjph::ojph_message::warn_message(
            $t,
            $crate::openjph::ojph_message::ojph_file_name(file!()),
            line!() as i32,
            format_args!($($arg)*),
        );
    }};
}

/// Builds an [`OjphError`](crate::openjph::ojph_message::OjphError) (logging it through the
/// configured error handler as a side effect, mirroring C++'s `OJPH_ERROR` message-then-throw).
/// Unlike `ojph_info!`/`ojph_warn!`, this is a **value-producing** expression, not a statement —
/// callers write `return Err(ojph_error!(...))` (or `Err(ojph_error!(...))?`) instead of the old
/// bare-statement-then-panic form.
#[macro_export]
macro_rules! ojph_error {
    ($t:expr, $($arg:tt)*) => {
        $crate::openjph::ojph_message::error_value(
            $t,
            $crate::openjph::ojph_message::ojph_file_name(file!()),
            line!() as i32,
            format_args!($($arg)*),
        )
    };
}

#[cfg(test)]
mod tests {
    use super::{
        MessageBase, MessageError, MessageInfo, MessageStream, MessageWarning, OjphMsgLevel,
        configure_error, configure_info, configure_warning, ojph_file_name, set_error_stream,
        set_info_stream, set_message_level, set_warning_stream,
    };
    use std::sync::{Arc, Mutex, MutexGuard};

    fn lock_entries(mutex: &Mutex<Vec<String>>) -> MutexGuard<'_, Vec<String>> {
        match mutex.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[derive(Default)]
    struct Recorder {
        entries: Mutex<Vec<String>>,
    }

    impl MessageBase for Recorder {
        fn call(&self, warn_code: i32, file_name: &str, line_num: i32, message: &str) {
            lock_entries(&self.entries)
                .push(format!("{warn_code:08X}:{file_name}:{line_num}:{message}"));
        }
    }

    impl MessageInfo for Recorder {}
    impl MessageWarning for Recorder {}

    #[derive(Default)]
    struct ErrorRecorder {
        entries: Mutex<Vec<String>>,
    }

    impl MessageBase for ErrorRecorder {
        fn call(&self, warn_code: i32, file_name: &str, line_num: i32, message: &str) {
            lock_entries(&self.entries)
                .push(format!("{warn_code:08X}:{file_name}:{line_num}:{message}"));
        }
    }

    impl MessageError for ErrorRecorder {}

    #[test]
    fn ojph_file_macro_name_matches_reference_rule() {
        assert_eq!(ojph_file_name("a/b/c.txt"), "c.txt");
        assert_eq!(ojph_file_name(r"a\b\c.txt"), "c.txt");
        assert_eq!(ojph_file_name("c.txt"), "c.txt");
    }

    #[test]
    fn custom_info_and_warning_handlers_receive_messages() {
        let info = Arc::new(Recorder::default());
        let warn = Arc::new(Recorder::default());

        configure_info(info.clone());
        configure_warning(warn.clone());
        set_info_stream(Some(MessageStream::Stdout));
        set_warning_stream(Some(MessageStream::Stdout));
        set_message_level(OjphMsgLevel::OjphMsgAllMsg);

        crate::ojph_info!(0x12, "hello {}", "info");
        crate::ojph_warn!(0x34, "hello {}", "warn");

        assert_eq!(lock_entries(&info.entries).len(), 1);
        assert_eq!(lock_entries(&warn.entries).len(), 1);
    }

    #[test]
    fn error_macro_logs_and_returns_typed_error() {
        let error = Arc::new(ErrorRecorder::default());
        configure_error(error.clone());
        set_error_stream(Some(MessageStream::Stderr));
        set_message_level(OjphMsgLevel::OjphMsgAllMsg);

        let err = crate::ojph_error!(0x56, "fatal {}", "message");

        assert_eq!(err.code, 0x56);
        assert_eq!(err.message, "fatal message");
        assert_eq!(lock_entries(&error.entries).len(), 1);
    }
}
