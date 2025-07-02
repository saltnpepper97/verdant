use std::env;

use bloom::errors::BloomError;
use bloom::status::LogLevel;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::time::ProcessTimer;

/// Set some basic environment variables for the process.
/// Logs a single message to both console and file after setting all variables.
/// Returns Ok(()) if all succeed.
pub fn set_basic_env_vars(
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    unsafe {
        env::set_var("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
        env::set_var("HOME", "/root");
        env::set_var("TERM", "xterm-256color");
        env::set_var("USER", "root");
        env::set_var("LOGNAME", "root");
    }

    let msg = "Basic environment variables set";
    log_message(msg, console_logger, file_logger, &timer);

    Ok(())
}

/// Helper function to log to console and file with Info level.
fn log_message(
    message: &str,
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
    timer: &ProcessTimer,
) {
    let elapsed = timer.elapsed();

    console_logger.message(LogLevel::Info, message, elapsed);
    file_logger.log(LogLevel::Info, message);
}
