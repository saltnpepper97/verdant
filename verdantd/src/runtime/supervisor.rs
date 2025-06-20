use std::io;
use crate::managed_service::ManagedService;

pub fn supervise_services(services: &mut [ManagedService]) -> io::Result<()> {
    for svc in services.iter_mut() {
        svc.supervise()?;
    }
    Ok(())
}
