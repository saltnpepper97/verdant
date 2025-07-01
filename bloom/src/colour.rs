/// ANSI escape codes for terminal colors
pub mod color {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const GREEN: &str = "\x1b[32m";
    pub const RED: &str = "\x1b[31m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const CYAN: &str = "\x1b[36m";
    pub const DIM: &str = "\x1b[2m";

    use crate::status::LogLevel;

    pub fn color_for_level(level: LogLevel) -> &'static str {
        match level {
            LogLevel::Ok => GREEN,
            LogLevel::Fail => RED,
            LogLevel::Warn => YELLOW,
            LogLevel::Info => CYAN,
        }
    }

    pub fn color_time(time_str: &str) -> String {
        format!("{DIM}{time_str}{RESET}")
    }

    pub fn color_level(level: LogLevel, level_str: &str) -> String {
        format!("{}{}{}", color_for_level(level), level_str, RESET)
    }
}
