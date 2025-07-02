use std::os::unix::io::AsRawFd;
use std::mem::{zeroed, size_of};
use std::time::Duration;
use std::thread::sleep;
use std::convert::TryInto;
use std::fs;

use nix::sys::socket::{socket, AddressFamily, SockType, SockFlag};
use nix::libc::{sockaddr_in, AF_INET, sockaddr, in_addr, c_char};

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

/// Setup loopback + bring all non-virtual interfaces up
pub fn setup_networks(
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    let sock = socket(AddressFamily::Inet, SockType::Datagram, SockFlag::empty(), None)
        .map_err(|e| BloomError::Custom(format!("Failed to open socket: {}", e)))?;
    let raw_sock = sock.as_raw_fd();

    setup_loopback_internal(raw_sock, console_logger, file_logger)?;

    if let Ok(entries) = fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            if let Ok(ifname_os) = entry.file_name().into_string() {
                let ifname = ifname_os.as_str();

                if ifname == "lo"
                    || ifname.starts_with("veth")
                    || ifname.starts_with("br")
                    || ifname.starts_with("docker")
                    || ifname.starts_with("tap")
                    || ifname.starts_with("tun")
                {
                    continue;
                }

                if is_interface_up(raw_sock, ifname)? {
                    let msg = format!("Interface {} already up", ifname);
                    console_logger.message(LogLevel::Info, &msg, timer.elapsed());
                    file_logger.log(LogLevel::Info, &msg);
                } else {
                    bring_interface_up(raw_sock, ifname)?;
                    let msg = format!("Brought up interface {}", ifname);
                    console_logger.message(LogLevel::Ok, &msg, timer.elapsed());
                    file_logger.log(LogLevel::Ok, &msg);
                }
            }
        }
    }

    Ok(())
}

fn setup_loopback_internal(
    raw_sock: libc::c_int,
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
) -> Result<(), BloomError> {
    if is_interface_up(raw_sock, "lo")? {
        let msg = "Loopback interface already up";
        console_logger.message(LogLevel::Info, msg, Duration::ZERO);
        file_logger.log(LogLevel::Info, msg);
        return Ok(());
    }

    bring_interface_up(raw_sock, "lo")?;
    assign_loopback_address(raw_sock, "lo")?;
    sleep(Duration::from_millis(100));

    let msg = "Loopback interface configured";
    console_logger.message(LogLevel::Ok, msg, Duration::ZERO);
    file_logger.log(LogLevel::Ok, msg);

    Ok(())
}

fn bring_interface_up(sock: libc::c_int, ifname: &str) -> Result<(), BloomError> {
    let mut ifr: libc::ifreq = unsafe { zeroed() };

    for (dst, src) in ifr.ifr_name.iter_mut().zip(ifname.bytes()) {
        *dst = src as c_char;
    }

    unsafe {
        if libc::ioctl(sock, libc::SIOCGIFFLAGS.try_into().unwrap(), &mut ifr) < 0 {
            return Err(BloomError::Custom(format!(
                "ioctl SIOCGIFFLAGS failed for {}",
                ifname
            )));
        }
    }

    let current_flags = unsafe { ifr.ifr_ifru.ifru_flags };
    let new_flags = current_flags | libc::IFF_UP as libc::c_short;

    unsafe {
        ifr.ifr_ifru.ifru_flags = new_flags;
        if libc::ioctl(sock, libc::SIOCSIFFLAGS.try_into().unwrap(), &ifr) < 0 {
            return Err(BloomError::Custom(format!(
                "ioctl SIOCSIFFLAGS failed for {}",
                ifname
            )));
        }
    }

    Ok(())
}

fn is_interface_up(sock: libc::c_int, ifname: &str) -> Result<bool, BloomError> {
    let mut ifr: libc::ifreq = unsafe { zeroed() };

    for (dst, src) in ifr.ifr_name.iter_mut().zip(ifname.bytes()) {
        *dst = src as c_char;
    }

    unsafe {
        if libc::ioctl(sock, libc::SIOCGIFFLAGS.try_into().unwrap(), &mut ifr) < 0 {
            return Err(BloomError::Custom(format!(
                "ioctl SIOCGIFFLAGS failed for {}",
                ifname
            )));
        }

        let flags = ifr.ifr_ifru.ifru_flags;
        Ok(flags & (libc::IFF_UP as libc::c_short) != 0)
    }
}

fn assign_loopback_address(sock: libc::c_int, ifname: &str) -> Result<(), BloomError> {
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct IfreqAddr {
        ifr_name: [c_char; libc::IFNAMSIZ],
        ifr_addr: sockaddr,
    }

    let mut addr_in: sockaddr_in = unsafe { zeroed() };
    addr_in.sin_family = AF_INET as u16;
    addr_in.sin_addr = in_addr {
        s_addr: u32::from_be_bytes([127, 0, 0, 1]),
    };

    let mut ifr: IfreqAddr = unsafe { zeroed() };
    for (dst, src) in ifr.ifr_name.iter_mut().zip(ifname.bytes()) {
        *dst = src as c_char;
    }

    unsafe {
        std::ptr::copy_nonoverlapping(
            &addr_in as *const sockaddr_in as *const u8,
            &mut ifr.ifr_addr as *mut sockaddr as *mut u8,
            size_of::<sockaddr_in>(),
        );

        if libc::ioctl(sock, libc::SIOCSIFADDR.try_into().unwrap(), &ifr) < 0 {
            return Err(BloomError::Custom(format!(
                "ioctl SIOCSIFADDR failed for {}",
                ifname
            )));
        }
    }

    Ok(())
}

