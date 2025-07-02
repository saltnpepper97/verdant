use std::io::Write;
use std::sync::{Arc, Mutex};

use bloom::log::{ConsoleLogger, ConsoleLoggerImpl, FileLogger, FileLoggerImpl};
use bloom::status::LogLevel;
use bloom::time::SystemTimer;

use crate::device_manager::{monitor_udev_events, start_device_manager};
use crate::env::set_basic_env_vars;
use crate::filesystem::{mount_virtual_filesystems, mount_securityfs};
use crate::hardware_drivers::load_hardware_drivers;
use crate::kernel::{apply_sysctl_settings, load_kernel_modules};
use crate::mount::{check_filesystem_health, mount_fstab_filesystems, remount_root};
use crate::network::setup_networks;
use crate::seed::seed_entropy;
use crate::utils::{detect_timezone, set_hostname, sync_clock_from_hardware};

pub fn boot() -> (
    Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    Arc<Mutex<dyn FileLogger + Send + Sync>>,
    SystemTimer,
) {
    let console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>> =
        Arc::new(Mutex::new(ConsoleLoggerImpl::new(LogLevel::Info)));
    let file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>> =
        Arc::new(Mutex::new(FileLoggerImpl::new(LogLevel::Info, "/var/log/verdant.log")));

    let start_time = SystemTimer::new();

    print!("\x1b[2J\x1b[H");
    std::io::stdout().flush().unwrap();

    // Print banner by locking once, still valid:
    {
        let mut con_log = console_logger.lock().unwrap();
        con_log.banner(&format!("Verdant Init v{} - Rooted in Resilience", env!("CARGO_PKG_VERSION")));
    }

    // Setup phase: call funcs passing Arc<Mutex<_>> refs directly
    let _ = set_hostname(&console_logger, &file_logger);
    let _ = detect_timezone(&console_logger, &file_logger);
    let _ = mount_virtual_filesystems(&console_logger, &file_logger);
    let _ = start_device_manager(&console_logger, &file_logger);
    let _ = load_kernel_modules(&console_logger, &file_logger);
    let _ = apply_sysctl_settings(&console_logger, &file_logger);

    // Spawn udev monitor thread — clone and move Arc
    {
        let file_logger_clone = Arc::clone(&file_logger);
        std::thread::spawn(move || {
            if let Err(e) = monitor_udev_events(&file_logger_clone) {
                if let Ok(mut log) = file_logger_clone.lock() {
                    log.log(LogLevel::Fail, &format!("udev event monitor failed: {}", e));
                }
            }
        });
    }

    // Continue boot, calling functions with Arc<Mutex<_>> refs
    let _ = load_hardware_drivers(&console_logger, &file_logger);

    // For operations needing multiple logs locked, lock explicitly once:
    {
        let mut con_log = console_logger.lock().unwrap();
        let mut file_log = file_logger.lock().unwrap();

        let _ = check_filesystem_health(&mut *con_log, &mut *file_log);
        let _ = remount_root(&mut *con_log, &mut *file_log);
        let _ = mount_fstab_filesystems(&mut *con_log, &mut *file_log);
        let _ = mount_securityfs(&mut *con_log, &mut *file_log);

        let _ = file_log.initialize(&mut *con_log);

        let _ = seed_entropy(&mut *con_log, &mut *file_log);
        let _ = sync_clock_from_hardware(&mut *con_log, &mut *file_log);
        let _ = set_basic_env_vars(&mut *con_log, &mut *file_log);
        let _ = setup_networks(&mut *con_log, &mut *file_log);
    }

    (console_logger, file_logger, start_time)
}

