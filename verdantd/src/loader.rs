use std::fs;

use crate::parser::parse_service_file;
use crate::service::Service;
use bloom::log::FileLogger;
use bloom::status;

const SERVICE_DIR: &str = "/etc/verdant/services";

pub fn load_services(logger: &mut dyn FileLogger) -> (Vec<Service>, usize, usize) {
    let mut services = Vec::new();
    let mut loaded_count = 0;
    let mut failed_count = 0;

    let entries = match fs::read_dir(SERVICE_DIR) {
        Ok(entries) => entries,
        Err(e) => {
            logger.log(
                status::LogLevel::Fail,
                &format!("Failed to read service directory: {}", e),
            );
            return (services, 0, 0);
        }
    };

    for entry in entries {
        if let Ok(entry) = entry {
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) == Some("vs") {
                match parse_service_file(path.to_str().unwrap_or_default()) {
                    Ok(mut parsed_services) => {
                        loaded_count += parsed_services.len();
                        services.append(&mut parsed_services);
                    }
                    Err(err) => {
                        failed_count += 1;
                        logger.log(
                            status::LogLevel::Fail,
                            &format!("Failed to load {}: {}", path.display(), err),
                        );
                    }
                }
            }
        }
    }

    logger.log(
        status::LogLevel::Info,
        &format!(
            "Service loading complete: {} loaded, {} failed.",
            loaded_count, failed_count
        ),
    );

    (services, loaded_count, failed_count)
}

