use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::ffi::CString;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nix::unistd::{fork, ForkResult, execvp};
use nix::sys::wait::{waitpid, WaitStatus};

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

/// Collects module names from a given file path.
/// Returns Vec<String> of module names.
fn collect_modules_from_file(path: &Path) -> Result<Vec<String>, BloomError> {
    let file = File::open(path).map_err(BloomError::Io)?;
    let reader = BufReader::new(file);
    let mut modules = Vec::new();

    for line_res in reader.lines() {
        let line = line_res.map_err(BloomError::Io)?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        modules.push(trimmed.to_string());
    }
    Ok(modules)
}

/// Loads kernel modules from multiple common paths.
/// Logs a summary of successes/failures or "no modules found".
pub fn load_kernel_modules(
    console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    let mut all_modules = Vec::new();

    // 1. Try /etc/modules (Debian style)
    if Path::new("/etc/modules").exists() {
        match collect_modules_from_file(Path::new("/etc/modules")) {
            Ok(mods) => all_modules.extend(mods),
            Err(e) => {
                log_error(console_logger, file_logger, &timer, LogLevel::Warn, &format!("Failed to read /etc/modules: {:?}", e));
            }
        }
    }

    // 2. Load from /etc/modules-load.d/*.conf and /usr/lib/modules-load.d/*.conf (Arch style)
    for dir_path in ["/etc/modules-load.d", "/usr/lib/modules-load.d"].iter() {
        let dir = Path::new(dir_path);
        if dir.is_dir() {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry_res in entries {
                    if let Ok(entry) = entry_res {
                        let path = entry.path();
                        if path.extension().and_then(|s| s.to_str()) == Some("conf") {
                            match collect_modules_from_file(&path) {
                                Ok(mods) => all_modules.extend(mods),
                                Err(e) => {
                                    log_error(console_logger, file_logger, &timer, LogLevel::Warn, &format!("Failed to read {:?}: {:?}", path, e));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Deduplicate module names
    all_modules.sort();
    all_modules.dedup();

    if all_modules.is_empty() {
        log_success(console_logger, file_logger, &timer, LogLevel::Info, "No kernel modules to load");
        return Ok(());
    }

    // Now load each module by forking modprobe
    let mut success_count = 0;
    let mut fail_count = 0;
    let mut children = Vec::new();

    for module_name in all_modules {
        match unsafe { fork() } {
            Ok(ForkResult::Child) => {
                let cmd = CString::new("modprobe").expect("CString::new failed");
                let arg = CString::new(module_name).expect("CString::new failed");
                let args = &[cmd.as_c_str(), arg.as_c_str()];
                let _ = execvp(&cmd, args);

                std::process::exit(1);
            }
            Ok(ForkResult::Parent { child }) => {
                children.push(child);
            }
            Err(_) => {
                fail_count += 1;
            }
        }
    }

    for child in children {
        match waitpid(child, None) {
            Ok(WaitStatus::Exited(_pid, 0)) => success_count += 1,
            Ok(WaitStatus::Exited(_pid, _)) => fail_count += 1,
            Ok(_) | Err(_) => fail_count += 1,
        }
    }

    let msg = format!("Kernel modules loaded: {} successful, {} failed", success_count, fail_count);
    let simple_console_msg = if success_count > 0 {
        "Kernel modules loaded"
    } else {
        "Failed to load kernel modules"
    };

    if success_count > 0 {
        log_success(console_logger, file_logger, &timer, LogLevel::Info, simple_console_msg);
        if let Ok(mut file_log) = file_logger.lock() {
            file_log.log(LogLevel::Info, &msg);
        }
        Ok(())
    } else {
        log_error(console_logger, file_logger, &timer, LogLevel::Fail, simple_console_msg);
        if let Ok(mut file_log) = file_logger.lock() {
            file_log.log(LogLevel::Fail, &msg);
        }
        Err(BloomError::Custom("Failed to load any kernel modules".into()))
    }
}

/// Applies kernel sysctl settings from common sysctl configuration files.
/// Only applies keys where the current value differs from the desired value.
pub fn apply_sysctl_settings(
    console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();
    let mut settings: HashMap<String, String> = HashMap::new();

    // Load settings from all sysctl sources
    let paths = [
        "/etc/sysctl.conf",
        "/etc/sysctl.d",
        "/usr/lib/sysctl.d",
    ];

    for path in paths.iter() {
        let p = Path::new(path);
        if p.is_file() {
            load_sysctl_file(p, &mut settings)?;
        } else if p.is_dir() {
            for entry in fs::read_dir(p).map_err(BloomError::Io)? {
                let entry = entry.map_err(BloomError::Io)?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("conf") {
                    load_sysctl_file(&path, &mut settings)?;
                }
            }
        }
    }

    let mut applied = 0;
    let mut skipped = 0;
    let mut failed = 0;

    for (key, desired_value) in &settings {
        let sysctl_path = format!("/proc/sys/{}", key.replace('.', "/"));
        let path = Path::new(&sysctl_path);

        if path.exists() {
            match fs::read_to_string(path) {
                Ok(current) => {
                    let current = current.trim();
                    if current == desired_value {
                        skipped += 1;
                        continue;
                    }
                    if let Err(_) = fs::write(path, desired_value) {
                        failed += 1;
                    } else {
                        applied += 1;
                    }
                }
                Err(_) => {
                    failed += 1;
                }
            }
        } else {
            failed += 1;
        }
    }

    let summary = format!("Sysctl settings: {} applied, {} skipped, {} failed", applied, skipped, failed);
    if let Ok(mut file_log) = file_logger.lock() {
        file_log.log(LogLevel::Info, &summary);
    }

    // Final console status based on outcome
    let (level, status_msg) = if applied > 0 {
        (LogLevel::Ok, "Kernel parameters applied")
    } else if skipped > failed {
        (LogLevel::Info, "All sysctl parameters already set")
    } else {
        (LogLevel::Warn, "Some sysctl parameters failed")
    };

    if let Ok(mut con_log) = console_logger.lock() {
        con_log.message(level, status_msg, timer.elapsed());
    }

    Ok(())
}


/// Helper to parse key=value lines from sysctl files
fn load_sysctl_file(path: &Path, map: &mut std::collections::HashMap<String, String>) -> Result<(), BloomError> {
    let file = File::open(path).map_err(BloomError::Io)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line.map_err(BloomError::Io)?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    Ok(())
}

fn log_success(
    console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
    timer: &ProcessTimer,
    level: LogLevel,
    msg: &str,
) {
    let elapsed = timer.elapsed();
    if let Ok(mut con_log) = console_logger.lock() {
        con_log.message(level, msg, elapsed);
    }
    if let Ok(mut file_log) = file_logger.lock() {
        file_log.log(level, msg);
    }
}

fn log_error(
    console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
    timer: &ProcessTimer,
    level: LogLevel,
    msg: &str,
) {
    let elapsed = timer.elapsed();
    if let Ok(mut con_log) = console_logger.lock() {
        con_log.message(level, msg, elapsed);
    }
    if let Ok(mut file_log) = file_logger.lock() {
        file_log.log(level, msg);
    }
}

