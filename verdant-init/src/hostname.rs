use std::ffi::CString;
use std::fs;
use libc;
use common::utils::*;

pub fn init_hostname() -> Result<(), Box<dyn std::error::Error>> {
    let hostname = fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| {
            print_error("/etc/hostname not found, defaulting to 'verdant'");
            "verdant".to_string()
        });

    let c_hostname = CString::new(hostname.clone())?;
    let result = unsafe { libc::sethostname(c_hostname.as_ptr(), hostname.len()) };

    if result == 0 {
        println!("{} Hostname set to: {}", status_ok(), hostname);
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}
