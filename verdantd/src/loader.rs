use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};

use crate::parser::parse_service_file;
use crate::service_file::ServiceFile;

const SERVICE_DIR: &str = "/etc/verdant/services";

pub fn load_services(
    _vars: &HashMap<String, String>,
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
) -> Result<Vec<ServiceFile>, BloomError> {
    let mut services = Vec::new();
    let dir_path = Path::new(SERVICE_DIR);

    if !dir_path.exists() {
        fs::create_dir_all(dir_path).map_err(|e| {
            let msg = format!("Failed to create service directory {}: {}", SERVICE_DIR, e);
            console_logger.message(bloom::status::LogLevel::Fail, &msg, std::time::Duration::ZERO);
            file_logger.log(bloom::status::LogLevel::Fail, &msg);
            BloomError::Io(e)
        })?;
    }

    for entry in fs::read_dir(dir_path).map_err(BloomError::Io)? {
        let entry = entry.map_err(BloomError::Io)?;
        let path = entry.path();

        if path.is_file() && path.extension().map(|s| s == "vs").unwrap_or(false) {
            let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

            if filename.contains('@') {
                // Handle template files like tty@.vs
                for id in find_tty_devices()? {
                    let mut vars = HashMap::new();
                    vars.insert("id".to_string(), id.clone());

                    match parse_service_file(&path, &vars, console_logger, file_logger) {
                        Ok(mut service) => {
                            // Rewrite the name to include the instance
                            service.name = filename.replace("@", &format!("@{}", id)).replace(".vs", "");
                            services.push(service);
                        }
                        Err(e) => {
                            let msg = format!("Failed to parse templated service {:?} with id {}: {:?}", path, id, e);
                            console_logger.message(bloom::status::LogLevel::Fail, &msg, std::time::Duration::ZERO);
                            file_logger.log(bloom::status::LogLevel::Fail, &msg);
                        }
                    }
                }
            } else {
                // Non-template service
                match parse_service_file(&path, &HashMap::new(), console_logger, file_logger) {
                    Ok(service) => services.push(service),
                    Err(e) => {
                        let msg = format!("Failed to parse service file {:?}: {:?}", path, e);
                        console_logger.message(bloom::status::LogLevel::Fail, &msg, std::time::Duration::ZERO);
                        file_logger.log(bloom::status::LogLevel::Fail, &msg);
                    }
                }
            }
        }
    }

    let summary_msg = format!("Loaded {} service file(s) from {}", services.len(), SERVICE_DIR);
    console_logger.message(bloom::status::LogLevel::Info, &summary_msg, std::time::Duration::ZERO);
    file_logger.log(bloom::status::LogLevel::Info, &summary_msg);

    Ok(services)
}

fn find_tty_devices() -> Result<Vec<String>, BloomError> {
    let mut ids = Vec::new();

    for entry in std::fs::read_dir("/dev").map_err(BloomError::Io)? {
        let entry = entry.map_err(BloomError::Io)?;
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with("tty") && name[3..].chars().all(|c| c.is_ascii_digit()) {
            if let Ok(num) = name[3..].parse::<u32>() {
                if num >= 1 && num <= 6 {
                    ids.push(name);
                }
            }
        }
    }

    Ok(ids)
}

