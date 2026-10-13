//! Thread-safe log ring buffer + a `tracing` layer + a colourised egui panel.
//!
//! The buffer is an `Arc<Mutex<VecDeque<LogEntry>>>` with a hard line cap, so any
//! worker thread can append while the egui thread reads a snapshot under the same
//! lock. [`LogLayer`] is a `tracing_subscriber::Layer` that captures events into
//! the buffer; [`show`] renders a tail-following, severity-coloured log panel.
//!
//! (Gathered from colmap-rs, which mirrors COLMAP's C++ `LogWidget`: a bounded
//! block count + a Clear button + INFO/WARN/ERROR surfacing.)

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// Re-exported so consumers can tag [`LogEntry`] levels without a direct
/// dependency on `tracing`.
pub use tracing::Level;

/// Maximum number of retained log lines (mirrors C++ `max_num_blocks = 100000`).
pub const DEFAULT_MAX_LINES: usize = 100_000;

/// One captured log line; the level is carried for colourisation in the UI.
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub level: tracing::Level,
    pub text: String,
}

/// Shared ring buffer of log entries. Cheap to clone (`Arc`).
#[derive(Clone, Debug, Default)]
pub struct LogBuffer {
    inner: Arc<Mutex<LogBufferInner>>,
}

#[derive(Debug)]
struct LogBufferInner {
    entries: VecDeque<LogEntry>,
    max_lines: usize,
}

impl Default for LogBufferInner {
    fn default() -> Self {
        Self {
            entries: VecDeque::with_capacity(1024),
            max_lines: DEFAULT_MAX_LINES,
        }
    }
}

impl LogBuffer {
    /// New buffer with the default cap.
    pub fn new() -> Self {
        Self::default()
    }

    /// New buffer with a custom line cap.
    pub fn with_max_lines(max_lines: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LogBufferInner {
                entries: VecDeque::with_capacity(1024),
                max_lines: max_lines.max(1),
            })),
        }
    }

    /// Append one entry; the oldest is dropped once the cap is hit.
    pub fn append(&self, entry: LogEntry) {
        if let Ok(mut g) = self.inner.lock() {
            if g.entries.len() == g.max_lines {
                g.entries.pop_front();
            }
            g.entries.push_back(entry);
        }
    }

    /// Drop all stored entries.
    pub fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.entries.clear();
        }
    }

    /// Snapshot the entries for rendering. The lock is held only for the copy.
    pub fn snapshot(&self) -> Vec<LogEntry> {
        match self.inner.lock() {
            Ok(g) => g.entries.iter().cloned().collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// A `tracing_subscriber::Layer` capturing `tracing::Event`s into a [`LogBuffer`].
pub struct LogLayer {
    buffer: LogBuffer,
}

impl LogLayer {
    pub fn new(buffer: LogBuffer) -> Self {
        Self { buffer }
    }
}

/// Visitor extracting the `message` field from a tracing event into a String.
struct MessageVisitor<'a> {
    out: &'a mut String,
}

impl tracing::field::Visit for MessageVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.out, "{value:?}");
        } else {
            let _ = write!(self.out, " {}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.out.push_str(value);
        } else {
            let _ = write!(self.out, " {}={value}", field.name());
        }
    }
}

impl<S> Layer<S> for LogLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let level = *event.metadata().level();
        // Only INFO/WARN/ERROR are surfaced (DEBUG/TRACE are dropped).
        if level > tracing::Level::INFO {
            return;
        }
        let mut text = String::new();
        match level {
            tracing::Level::WARN => text.push_str("W: "),
            tracing::Level::ERROR => text.push_str("E: "),
            _ => {}
        }
        let mut visitor = MessageVisitor { out: &mut text };
        event.record(&mut visitor);
        self.buffer.append(LogEntry { level, text });
    }
}

/// Render the log panel: a Clear button + a fixed-pitch, severity-coloured,
/// tail-following (`stick_to_bottom`) scroll area over the buffer's snapshot.
pub fn show(ui: &mut egui::Ui, buffer: &LogBuffer) {
    ui.horizontal(|ui| {
        if ui.button("Clear").clicked() {
            buffer.clear();
        }
    });
    ui.separator();

    let entries = buffer.snapshot();
    // Severity colours come from the shared semantic palette so they track the host theme.
    let sem = egui_widgets_config::semantic(ui);
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for entry in &entries {
                let colour = match entry.level {
                    tracing::Level::ERROR => sem.err,
                    tracing::Level::WARN => sem.warn,
                    _ => ui.visuals().text_color(),
                };
                ui.label(egui::RichText::new(&entry.text).monospace().color(colour));
            }
        });
}
