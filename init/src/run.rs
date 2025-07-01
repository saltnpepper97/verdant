use bloom::log::{ConsoleLogger, ConsoleLoggerImpl, FileLoggerImpl};
use bloom::status::LogLevel;
use bloom::time::SystemTimer;

use crate::device_manager::{monitor_udev_events, start_device_manager};
use crate::env::set_basic_env_vars;
use crate::fs::{mount_virtual_filesystems, mount_securityfs};
use crate::hardware_drivers::check_hardware_drivers;
use crate::kernel::{apply_sysctl_settings, load_kernel_modules};
use crate::mount::{check_filesystem_health, mount_fstab_filesystems, remount_root};
use crate::network::setup_loopback;
use crate::seed::seed_entropy;
use crate::utils::{detect_timezone, set_hostname, sync_clock_from_hardware};

pub fn boot() -> (ConsoleLoggerImpl, FileLoggerImpl, SystemTimer) {
    let mut console_logger = ConsoleLoggerImpl::new(LogLevel::Info);
    let mut file_logger = FileLoggerImpl::new(LogLevel::Info, "/var/log/verdant.log");
    let start_time = SystemTimer::new();

    let version = env!("CARGO_PKG_VERSION");
    let banner = format!("Verdant Init v{version} - Rooted in Resilience");
    console_logger.banner(&banner);

    let _ = set_hostname(&mut console_logger, &mut file_logger);
    let _ = detect_timezone(&mut console_logger, &mut file_logger);

    let _ = mount_virtual_filesystems(&mut console_logger, &mut file_logger);

    let _ = load_kernel_modules(&mut console_logger, &mut file_logger);
    let _ = apply_sysctl_settings(&mut console_logger, &mut file_logger);

    let _ = start_device_manager(&mut console_logger, &mut file_logger);
    let _ = monitor_udev_events(&mut file_logger);

    let _ = check_hardware_drivers(&mut console_logger, &mut file_logger);

    let _ = check_filesystem_health(&mut console_logger, &mut file_logger);

    let _ = remount_root(&mut console_logger, &mut file_logger);

    let _ = mount_fstab_filesystems(&mut console_logger, &mut file_logger);

    let _ = mount_securityfs(&mut console_logger, &mut file_logger);

    let _ = file_logger.initialize(&mut console_logger);

    let _ = seed_entropy(&mut console_logger, &mut file_logger);

    let _ = sync_clock_from_hardware(&mut console_logger, &mut file_logger);

    let _ = set_basic_env_vars(&mut console_logger, &mut file_logger);

    let _ = setup_loopback(&mut console_logger, &mut file_logger);

    (console_logger, file_logger, start_time)
}

