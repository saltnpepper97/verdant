use std::io::{self, Write};
use std::process::{Child, Command, Stdio};
use std::{thread, time::Duration};

use bloom::log::ConsoleLogger;
use bloom::status::LogLevel;

/// Launches verdantd as a child process after displaying a polished transition.
pub fn launch_verdant_service_manager(console_logger: &mut (impl ConsoleLogger + ?Sized)) -> Option<Child> {
    // Print launching line with loading animation
    print!("\nInitialization complete, launching Verdant Service Manager");
    io::stdout().flush().unwrap();

    for _ in 0..3 {
        print!(".");
        io::stdout().flush().unwrap();
        thread::sleep(Duration::from_millis(333));
    }

    // Clear line (move cursor to beginning and erase line)
    print!("\r\x1b[2K"); // \r = carriage return, \x1b[2K = ANSI erase line
    io::stdout().flush().unwrap();

    // Spawn verdantd silently
    match Command::new("/usr/sbin/verdantd")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => Some(child),
        Err(e) => {
            // Failure: log the error visibly
            console_logger.message(
                LogLevel::Fail,
                &format!("Failed to launch Verdant Service Manager: {e}"),
                Duration::from_secs(0),
            );
            None
        }
    }
}

