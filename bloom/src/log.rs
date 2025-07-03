use std::fs::{metadata, OpenOptions};
use std::io::Write;
use std::time::{Duration, Instant};
use std::path::Path;

use regex::Regex;
use terminal_size::{Width, terminal_size};

use crate::status::LogLevel;
use crate::time::format_duration;
use crate::colour::color::{color_time, color_level, GREEN, RESET, BOLD};
use crate::errors::BloomError;

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Ok => "OK",
            LogLevel::Fail => "FAIL",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
        }
    }
}

/// Common logging interface — no need for Arc/Mutex here.
pub trait Logger {
    fn log(&mut self, level: LogLevel, message: &str, duration: Option<Duration>);
}

// === CONSOLE LOGGER ===

pub trait ConsoleLogger {
    fn message(&mut self, level: LogLevel, message: &str, duration: Duration);
    fn banner(&mut self, message: &str);
}

pub struct ConsoleLoggerImpl {
    pub min_level: LogLevel,
    pub start_time: Instant,
}

impl ConsoleLoggerImpl {
    pub fn new(min_level: LogLevel) -> Self {
        Self {
            min_level,
            start_time: Instant::now(),
        }
    }

    fn format_console(&self, level: LogLevel, message: &str, duration: Duration) -> String {
        let raw_time_str = format_duration(duration);
        let time_str = color_time(&raw_time_str);
        let level_raw = padded_level(level);
        let level_str = color_level(level, &level_raw);

        let term_width = terminal_size()
            .map(|(Width(w), _)| w as usize)
            .unwrap_or(80);

        let base_str = format!("{time_str} {message}");

        // Strip ANSI to get visible lengths only
        let base_len = strip_ansi_codes(&base_str).chars().count();
        let level_len = strip_ansi_codes(&level_str).chars().count();

        let padding = if term_width > base_len + level_len {
            term_width - base_len - level_len
        } else {
            1
        };

        let pad_spaces = " ".repeat(padding);

        format!("{base_str}{pad_spaces}{level_str}")
    }
}

impl ConsoleLogger for ConsoleLoggerImpl {
    fn message(&mut self, level: LogLevel, message: &str, duration: Duration) {
        if level >= self.min_level {
            let line = self.format_console(level, message, duration);
            println!("{}", line);
        }
    }

    fn banner(&mut self, message: &str) {
        println!("{BOLD}{GREEN}{message}{RESET}\n");
    }
}

// === FILE LOGGER ===

pub trait FileLogger {
    fn log(&mut self, level: LogLevel, message: &str);

    // No default implementation here: force explicit call on impl
    fn initialize(&mut self, console_logger: &mut dyn ConsoleLogger) -> Result<(), BloomError>;
}

pub struct FileLoggerImpl {
    pub min_level: LogLevel,
    pub file_path: String,
    has_initialized: bool,
    buffer: Vec<String>,
}

impl FileLoggerImpl {
    pub fn new(min_level: LogLevel, file_path: impl Into<String>) -> Self {
        Self {
            min_level,
            file_path: file_path.into(),
            has_initialized: false,
            buffer: Vec::new(),
        }
    }

    fn format_file(&self, level: LogLevel, message: &str) -> String {
        let now = chrono::Local::now();
        let timestamp = now.format("[%d-%m-%Y %H:%M:%S]").to_string();
        let level_str = padded_level(level);
        format!("{level_str} {timestamp} {message}")
    }

    fn maybe_write_session_header(&mut self) -> Result<(), BloomError> {
        if self.has_initialized {
            return Ok(());
        }

        let is_existing_file = metadata(&self.file_path)
            .map(|m| m.len() > 0)
            .unwrap_or(false);

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
        {
            if is_existing_file {
                writeln!(file, "\n────────── NEW SESSION ──────────").map_err(BloomError::Io)?;
            }
        }
        Ok(())
    }
}

impl FileLogger for FileLoggerImpl {
    fn log(&mut self, level: LogLevel, message: &str) {
        if level >= self.min_level {
            let line = self.format_file(level, message);

            if self.has_initialized {
                if let Ok(mut file) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.file_path)
                {
                    let _ = writeln!(file, "{}", line);
                }
            } else {
                self.buffer.push(line);
            }
        }
    }

    fn initialize(&mut self, console_logger: &mut dyn ConsoleLogger) -> Result<(), BloomError> {
        if self.has_initialized {
            return Ok(());
        }

        // Ensure parent directory exists
        if let Some(parent) = Path::new(&self.file_path).parent() {
            std::fs::create_dir_all(parent).map_err(BloomError::Io)?;
        }

        self.maybe_write_session_header()?;

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
        {
            for entry in &self.buffer {
                writeln!(file, "{}", entry).map_err(BloomError::Io)?;
            }
        }
        self.buffer.clear();
        self.has_initialized = true;

        console_logger.message(
            LogLevel::Info,
            &format!("File logger initialized: {}", self.file_path),
            Duration::from_secs(0),
        );

        Ok(())
    }
}

// === HELPERS ===

fn padded_level(level: LogLevel) -> String {
    format!("[ {:^4} ]", level.as_str())
}

fn strip_ansi_codes(s: &str) -> String {
    // Matches ANSI escape codes like \x1b[...m
    let ansi_re = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    ansi_re.replace_all(s, "").to_string()
}

