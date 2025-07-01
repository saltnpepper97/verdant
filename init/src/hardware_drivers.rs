use std::fs;
use std::process::Command;

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

use udev::{Enumerator, Device};

fn is_module_loaded(module_name: &str) -> bool {
    if let Ok(output) = Command::new("lsmod").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return stdout.lines().any(|line| line.starts_with(module_name));
        }
    }
    false
}

fn get_driver_for_device(device: &Device) -> Option<String> {
    let syspath = device.syspath();
    let driver_link = syspath.join("driver");

    if let Ok(driver_path) = fs::read_link(&driver_link) {
        return driver_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string());
    }
    None
}

pub fn check_hardware_drivers(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    let mut enumerator = Enumerator::new().map_err(BloomError::from)?;
    let mut loaded_count = 0;
    let mut missing_count = 0;

    for device in enumerator.scan_devices().map_err(BloomError::from)? {
        let devname = device
            .property_value("ID_MODEL")
            .or_else(|| device.property_value("DEVNAME"))
            .map(|s| s.to_string_lossy().into_owned());

        // Skip unknown devices
        let Some(_devname) = devname else { continue };

        if let Some(driver) = get_driver_for_device(&device) {
            if is_module_loaded(&driver) {
                loaded_count += 1;
            } else {
                missing_count += 1;
            }
        }
    }

    let summary = format!(
        "Hardware driver check: {} loaded, {} missing",
        loaded_count, missing_count
    );
    file_logger.log(LogLevel::Info, &summary);

    let elapsed = timer.elapsed();
    if loaded_count > 0 {
        console_logger.message(LogLevel::Ok, "Hardware drivers loaded", elapsed);
    } else {
        console_logger.message(LogLevel::Fail, "No hardware drivers loaded", elapsed);
    }

    Ok(())
}

