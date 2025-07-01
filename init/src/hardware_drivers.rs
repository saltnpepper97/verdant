use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

/// Load hardware drivers using the best available method for the system.
///
/// Tries, in order:
/// - `mdev -s`
/// - `udevadm trigger --action=add`
/// - manual uevent writes as a last resort
pub fn load_hardware_drivers(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    if !Path::new("/sys").exists() {
        return Err(BloomError::Custom("/sys not available".to_string()));
    }

    // Try mdev -s (BusyBox)
    if Path::new("/sbin/mdev").exists() || Path::new("/bin/mdev").exists() {
        let mdev_path = if Path::new("/sbin/mdev").exists() {
            "/sbin/mdev"
        } else {
            "/bin/mdev"
        };

        let status = Command::new(mdev_path)
            .arg("-s")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        if let Ok(s) = status {
            if s.success() {
                log_success(
                    console_logger,
                    file_logger,
                    &timer,
                    "Loaded hardware drivers via mdev -s",
                );
                return Ok(());
            }
        }
    }

    // Try udevadm (most major distros)
    if Path::new("/usr/bin/udevadm").exists() || Path::new("/bin/udevadm").exists() {
        let udevadm_path = if Path::new("/usr/bin/udevadm").exists() {
            "/usr/bin/udevadm"
        } else {
            "/bin/udevadm"
        };

        let trigger_status = Command::new(udevadm_path)
            .args(["trigger", "--action=add"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        if let Ok(s) = trigger_status {
            if s.success() {
                // Optional: wait until all events settle
                let _ = Command::new(udevadm_path)
                    .arg("settle")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();

                log_success(
                    console_logger,
                    file_logger,
                    &timer,
                    "Loaded hardware drivers via udevadm trigger",
                );
                return Ok(());
            }
        }
    }

    // Fallback: manually write "add" to every uevent (slow and potentially unsafe)
    let mut triggered = 0;
    let mut failed = 0;

    for entry in fs::read_dir("/sys/class").map_err(BloomError::Io)? {
        if let Ok(entry) = entry {
            let path = entry.path();
            if path.join("uevent").exists() {
                if let Err(e) = fs::write(path.join("uevent"), b"add\n") {
                    failed += 1;
                    let _ = file_logger.log(
                        LogLevel::Warn,
                        &format!("Failed uevent trigger: {:?} - {}", path, e),
                    );
                } else {
                    triggered += 1;
                }
            }
        }
    }

    for entry in walkdir::WalkDir::new("/sys/devices")
        .follow_links(true)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().ends_with("uevent"))
    {
        if let Err(e) = fs::write(entry.path(), b"add\n") {
            failed += 1;
            let _ = file_logger.log(
                LogLevel::Warn,
                &format!("Failed uevent trigger: {} - {}", entry.path().display(), e),
            );
        } else {
            triggered += 1;
        }
    }

    let msg = format!("Manually triggered {} uevents ({} failed)", triggered, failed);
    let level = if triggered > 0 { LogLevel::Ok } else { LogLevel::Warn };

    console_logger.message(level, &msg, timer.elapsed());
    file_logger.log(level, &msg);

    Ok(())
}

fn log_success(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
    timer: &ProcessTimer,
    msg: &str,
) {
    console_logger.message(LogLevel::Ok, msg, timer.elapsed());
    file_logger.log(LogLevel::Info, msg);
}

