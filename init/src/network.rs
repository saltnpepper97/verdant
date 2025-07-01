use std::os::unix::io::AsRawFd;
use std::mem::{zeroed, size_of};
use std::time::Duration;
use std::thread::sleep;

use nix::sys::socket::{socket, AddressFamily, SockType, SockFlag};
use nix::libc::{sockaddr_in, AF_INET, sockaddr, in_addr, c_char,self};

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

/// Setup the loopback interface `lo`
/// Equivalent to:
///   ip link set dev lo up
///   ip addr add 127.0.0.1/8 dev lo
pub fn setup_loopback(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    let sock = socket(AddressFamily::Inet, SockType::Datagram, SockFlag::empty(), None)
        .map_err(|e| BloomError::Custom(format!("Failed to open socket: {}", e)))?;

    let raw_sock = sock.as_raw_fd();

    bring_interface_up(raw_sock, "lo")?;
    assign_loopback_address(raw_sock, "lo")?;

    // Pause briefly to let interface settle
    sleep(Duration::from_millis(100));

    let msg = "Network interface configured";
    console_logger.message(LogLevel::Ok, msg, timer.elapsed());
    file_logger.log(LogLevel::Ok, msg);

    Ok(())
}

/// Bring up interface using ioctl SIOCSIFFLAGS
fn bring_interface_up(sock: libc::c_int, ifname: &str) -> Result<(), BloomError> {

    // Define ifreq struct for ioctl
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct Ifreq {
        ifr_name: [c_char; libc::IFNAMSIZ],
        ifr_flags: libc::c_short, // keep c_short here, musl compatible
        _pad: [u8; 24], // padding for rest of union, to size 40 bytes total
    }

    // Initialize ifreq
    let mut ifr: Ifreq = unsafe { zeroed() };
    for (dst, src) in ifr.ifr_name.iter_mut().zip(ifname.bytes()) {
        *dst = src as c_char;
    }

    // Get current flags
    unsafe {
        if libc::ioctl(sock, libc::SIOCGIFFLAGS.try_into().unwrap(), &mut ifr) < 0 {
            return Err(BloomError::Custom(format!("ioctl SIOCGIFFLAGS failed for {}", ifname)));
        }
    }

    // Set IFF_UP flag
    const IFF_UP: libc::c_short = 0x1;
    ifr.ifr_flags |= IFF_UP;

    // Set flags back
    unsafe {
        if libc::ioctl(sock, libc::SIOCSIFFLAGS.try_into().unwrap(), &ifr) < 0 {
            return Err(BloomError::Custom(format!("ioctl SIOCSIFFLAGS failed for {}", ifname)));
        }
    }

    Ok(())
}

/// Assign 127.0.0.1/8 IP address to lo interface
fn assign_loopback_address(sock: libc::c_int, ifname: &str) -> Result<(), BloomError> {

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct IfreqAddr {
        ifr_name: [c_char; libc::IFNAMSIZ],
        ifr_addr: sockaddr,
    }

    // Prepare sockaddr_in for 127.0.0.1
    let mut addr_in: sockaddr_in = unsafe { zeroed() };
    addr_in.sin_family = AF_INET as u16;

    // IMPORTANT: s_addr is in network byte order (big endian)
    // musl expects it as u32 in network byte order; use u32::from_be_bytes + u32::to_be()
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
    }

    unsafe {
        if libc::ioctl(sock, libc::SIOCSIFADDR.try_into().unwrap(), &ifr) < 0 {
            return Err(BloomError::Custom(format!("ioctl SIOCSIFADDR failed for {}", ifname)));
        }
    }

    Ok(())
}

