use libc;

use std::{fs, io};

/// Shutdown the system gracefully:
/// 1. Sync disks
/// 2. Reboot syscall with POWER_OFF
/// 3. Fallback: write “o” to /proc/sysrq-trigger
pub fn shutdown() -> io::Result<()> {
    // 1. sync disks
    unsafe { libc::sync() };

    // 2. try the reboot syscall
    if reboot_syscall(libc::LINUX_REBOOT_CMD_POWER_OFF).is_ok() {
        return Ok(());
    }

    // 3. fallback via sysrq-trigger
    fs::write("/proc/sysrq-trigger", "o\n")
}

/// Reboot the system gracefully:
/// 1. Sync disks
/// 2. Reboot syscall with RESTART
/// 3. Fallback: write “b” to /proc/sysrq-trigger
pub fn reboot() -> io::Result<()> {
    // 1. sync disks
    unsafe { libc::sync() };

    // 2. try the reboot syscall
    if reboot_syscall(libc::LINUX_REBOOT_CMD_RESTART).is_ok() {
        return Ok(());
    }

    // 3. fallback via sysrq-trigger
    fs::write("/proc/sysrq-trigger", "b\n")
}

/// Perform the Linux reboot syscall with the given command.
///
/// Uses the standard magic constants. Returns Ok(()) on success.
fn reboot_syscall(cmd: i32) -> io::Result<()> {
    const LINUX_REBOOT_MAGIC1: libc::c_int = 0xfee1_dead_u32 as libc::c_int;
    const LINUX_REBOOT_MAGIC2: libc::c_int = 672274793;
    const SYS_REBOOT: libc::c_long = libc::SYS_reboot as libc::c_long; // portable syscall number

    let res = unsafe {
        libc::syscall(
            SYS_REBOOT,
            LINUX_REBOOT_MAGIC1,
            LINUX_REBOOT_MAGIC2,
            cmd,
            std::ptr::null::<()>(),
        )
    };

    if res == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
