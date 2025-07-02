use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use threadpool::ThreadPool;
use wait_timeout::ChildExt;
use walkdir::WalkDir;

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

pub fn load_hardware_drivers(
    console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    // Run depmod just in case
    let _ = Command::new("/sbin/depmod")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let mut aliases = BTreeSet::new();

    for entry in WalkDir::new("/sys/devices")
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_name() == "modalias")
    {
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        if path.parent().map(|p| p.join("driver").exists()).unwrap_or(false) {
            continue;
        }

        if let Ok(file) = File::open(path) {
            for line in BufReader::new(file).lines().flatten() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && trimmed.len() < 256 {
                    aliases.insert(trimmed.to_string());
                }
            }
        }
    }

    if aliases.is_empty() {
        let msg = "No modalias entries found to load hardware drivers.";
        if let Ok(mut file_log) = file_logger.lock() {
            file_log.log(LogLevel::Info, msg);
        }
        if let Ok(mut con_log) = console_logger.lock() {
            con_log.message(LogLevel::Warn, msg, timer.elapsed());
        }
        return Ok(());
    }

    // Parallel modprobe execution
    let pool = ThreadPool::new(12);
    let timeout = Duration::from_secs(2);
    let loaded = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let file_logger = Arc::clone(file_logger);

    for alias in aliases {
        let loaded = Arc::clone(&loaded);
        let failed = Arc::clone(&failed);
        let file_logger = Arc::clone(&file_logger);
        let alias = alias.clone();

        pool.execute(move || {
            let mut cmd = Command::new("/sbin/modprobe");
            cmd.arg("-b").arg(&alias);
            cmd.stdout(Stdio::null()).stderr(Stdio::null());

            match cmd.spawn() {
                Ok(mut child) => match child.wait_timeout(timeout).unwrap_or(None) {
                    Some(status) if status.success() => {
                        loaded.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        failed.fetch_add(1, Ordering::Relaxed);
                        if let Ok(mut log) = file_logger.lock() {
                            let _ = log.log(
                                LogLevel::Info,
                                &format!("modprobe timed out or failed for alias: {}", alias),
                            );
                        }
                    }
                },
                Err(e) => {
                    failed.fetch_add(1, Ordering::Relaxed);
                    if let Ok(mut log) = file_logger.lock() {
                        let _ = log.log(
                            LogLevel::Info,
                            &format!("Failed to spawn modprobe for {}: {}", alias, e),
                        );
                    }
                }
            }
        });
    }

    pool.join(); // wait for all threads

    let loaded_count = loaded.load(Ordering::Relaxed);
    let failed_count = failed.load(Ordering::Relaxed);
    let msg = format!("Loaded {} hardware modules ({} failed)", loaded_count, failed_count);

    if let Ok(mut file_log) = file_logger.lock() {
        file_log.log(LogLevel::Info, &msg);
    }

    if let Ok(mut con_log) = console_logger.lock() {
        con_log.message(
            if loaded_count > 0 { LogLevel::Ok } else { LogLevel::Warn },
            &msg,
            timer.elapsed(),
        );
    }

    Ok(())
}

