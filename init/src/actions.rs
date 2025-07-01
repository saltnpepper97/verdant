use std::process::Command;
use std::io;
use std::path::Path;
use std::ptr;

/// Shutdown the system gracefully.
///
/// Attempts to run `/sbin/shutdown now`, then falls back to Linux reboot syscall with poweroff.
pub fn shutdown() -> io::Result<()> {
    if try_command("/sbin/shutdown", &["now"])? {
        return Ok(());
    }
    reboot_syscall(libc::LINUX_REBOOT_CMD_POWER_OFF)
}

/// Reboot the system gracefully.
///
/// Attempts to run `/sbin/reboot`, then falls back to Linux reboot syscall with restart.
pub fn reboot() -> io::Result<()> {
    if try_command("/sbin/reboot", &[])? {
        return Ok(());
    }
    reboot_syscall(libc::LINUX_REBOOT_CMD_RESTART)
}

/// Try running a command with arguments if the binary exists.
///
/// Returns Ok(true) if command ran successfully, Ok(false) if not present or failed.
fn try_command(cmd: &str, args: &[&str]) -> io::Result<bool> {
    if !Path::new(cmd).exists() {
        return Ok(false);
    }

    let status = Command::new(cmd)
        .args(args)
        .status();

    match status {
        Ok(s) if s.success() => Ok(true),
        Ok(_) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Perform the Linux reboot syscall with the given reboot command.
///
/// Must be run as root. Uses proper magic constants and syncs disks first.
fn reboot_syscall(cmd: i32) -> io::Result<()> {
    unsafe { libc::sync() };

    const LINUX_REBOOT_MAGIC1: libc::c_int = 0xfee1_dead_u32 as libc::c_int;
    const LINUX_REBOOT_MAGIC2: libc::c_int = 672274793;
    const SYS_reboot: libc::c_long = 169; // syscall number for reboot on x86_64 Linux

    let res = unsafe {
        libc::syscall(
            SYS_reboot,
            LINUX_REBOOT_MAGIC1,
            LINUX_REBOOT_MAGIC2,
            cmd,
            ptr::null::<()>()
        )
    };

    if res == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

