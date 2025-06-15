mod mount;
mod hostname;
mod process;

use common::utils::verdant_banner;

fn main() {
    // Uncomment this to ensure the program is running as PID 1 (the init process)
    /*
    if std::process::id() != 1 {
        eprintln!("{} Verdant init system must be run as PID 1!", status_fail());
        exit(1);
    }
    */
    
    verdant_banner();

    let _ = mount::mount_drives();    

    let _ = hostname::init_hostname();

    process::start_verdantd();
}
