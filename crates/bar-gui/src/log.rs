//! In-app log buffer: levels, entries, and the ring buffer.

use std::collections::VecDeque;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum LogLevel {
    #[default]
    Info,
    Debug,
    Warning,
    Error,
}

impl LogLevel {
    pub(crate) fn label(self) -> &'static str {
        match self {
            LogLevel::Debug => "DBG",
            LogLevel::Info => "INF",
            LogLevel::Warning => "WRN",
            LogLevel::Error => "ERR",
        }
    }
}

#[derive(Clone)]
pub(crate) struct LogEntry {
    pub(crate) level: LogLevel,
    pub(crate) message: String,
    /// Seconds since the LogBuffer was created -- precomputed at push time.
    pub(crate) elapsed_secs: f32,
}

pub(crate) struct LogBuffer {
    entries: VecDeque<LogEntry>,
    capacity: usize,
    start: Instant,
    needs_scroll: bool,
}

impl LogBuffer {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity.min(512)),
            capacity,
            start: Instant::now(),
            needs_scroll: false,
        }
    }

    pub(crate) fn push(&mut self, level: LogLevel, message: String) {
        let elapsed_secs = self.start.elapsed().as_secs_f32();
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(LogEntry {
            level,
            message,
            elapsed_secs,
        });
        self.needs_scroll = true;
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = &LogEntry> {
        self.entries.iter()
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    /// Returns true (once) when new entries were pushed since the last call.
    pub(crate) fn take_needs_scroll(&mut self) -> bool {
        let v = self.needs_scroll;
        self.needs_scroll = false;
        v
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new(500)
    }
}
