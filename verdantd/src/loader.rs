use std::fs;
use std::path::Path;

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;

use crate::parser::parse_service_file;
use crate::service_file::ServiceFile;
use crate::ordering::order_services;

const SERVICE_DIR: &str = "/etc/verdant/services";

pub fn load_services(
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
) -> Result<Vec<ServiceFile>, BloomError> {
    let mut services = Vec::new();
    let mut parsed_count = 0;
    let dir_path = Path::new(SERVICE_DIR);

    if !dir_path.exists() {
        fs::create_dir_all(dir_path).map_err(|e| {
            let msg = format!("Failed to create service directory {}: {}", SERVICE_DIR, e);
            console_logger.message(LogLevel::Fail, &msg, std::time::Duration::ZERO);
            file_logger.log(LogLevel::Fail, &msg);
            BloomError::Io(e)
        })?;
    }

    for entry in fs::read_dir(dir_path).map_err(BloomError::Io)? {
        let entry = entry.map_err(BloomError::Io)?;
        let path = entry.path();

        if path.is_file() && path.extension().map(|s| s == "vs").unwrap_or(false) {
            // Initial parse with no instance substitution to detect instances
            match parse_service_file(&path, None, console_logger, file_logger) {
                Ok(service) => {
                    if let Some(instances) = &service.instances {
                        for instance in instances {
                            let id = instance.trim();

                            match parse_service_file(&path, Some(id), console_logger, file_logger) {
                                Ok(inst) => {
                                    services.push(inst);
                                    parsed_count += 1;
                                }
                                Err(e) => {
                                    let msg = format!(
                                        "Failed to parse service file {:?} with instance '{}': {:?}",
                                        path, id, e
                                    );
                                    console_logger.message(LogLevel::Fail, &msg, std::time::Duration::ZERO);
                                    file_logger.log(LogLevel::Fail, &msg);
                                }
                            }
                        }
                    } else {
                        // No instances: use the original service definition as-is
                        services.push(service);
                        parsed_count += 1;
                    }
                }
                Err(e) => {
                    let msg = format!("Failed to parse service file {:?}: {:?}", path, e);
                    console_logger.message(LogLevel::Fail, &msg, std::time::Duration::ZERO);
                    file_logger.log(LogLevel::Fail, &msg);
                }
            }
        }
    }

    let ordered_services = order_services(services)?;

    let summary_msg = format!("Parsed {} service file(s)", parsed_count);
    console_logger.message(LogLevel::Ok, &summary_msg, std::time::Duration::ZERO);
    file_logger.log(LogLevel::Ok, &summary_msg);

    Ok(ordered_services)
}

