use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

use crate::service_file::{ServiceFile, RestartPolicy};

/// Replace each `{}` in the content with the instance string
fn apply_template(content: &str, instance: Option<&str>) -> String {
    if let Some(id) = instance {
        content.replace("{}", id)
    } else {
        content.to_string()
    }
}

/// Parse .vs service file at `path`, applying `{}` template substitution if `instance` is Some
pub fn parse_service_file(
    path: &Path,
    instance: Option<&str>,
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
) -> Result<ServiceFile, BloomError> {
    let timer = ProcessTimer::start();

    let raw_content = fs::read_to_string(path).map_err(BloomError::Io)?;
    let content = apply_template(&raw_content, instance);
    let reader = BufReader::new(content.as_bytes());

    let mut service = ServiceFile::new(String::new(), String::new());
    let mut current_key: Option<String> = None;

    for line_res in reader.lines() {
        let line = line_res.map_err(BloomError::Io)?;
        let line = line.trim_end();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with("  ") || line.starts_with('\t') {
            if let Some(ref key) = current_key {
                let val = line.trim_start_matches(|c: char| c == '-' || c.is_whitespace()).trim_end();
                match key.as_str() {
                    "env" => {
                        service.env.get_or_insert(Vec::new()).push(val.to_string());
                    }
                    "dependencies" => {
                        service.dependencies.get_or_insert(Vec::new()).push(val.to_string());
                    }
                    "tags" => {
                        service.tags.get_or_insert(Vec::new()).push(val.to_string());
                    }
                    "instances" => {
                        service.instances.get_or_insert(Vec::new()).push(val.to_string());
                    }
                    _ => {}
                }
                continue;
            } else {
                continue;
            }
        }

        if let Some((key, val)) = line.split_once(':') {
            let key = key.trim();
            let val = val.trim();
            current_key = Some(key.to_string());

            match key {
                "name" => service.name = val.to_string(),
                "desc" => service.desc = if val.is_empty() { None } else { Some(val.to_string()) },
                "cmd" => service.cmd = val.to_string(),
                "args" => {
                    service.args = if val.is_empty() {
                        None
                    } else {
                        Some(val.split_whitespace().map(|s| s.to_string()).collect())
                    };
                }
                "pre-cmd" => service.pre_cmd = if val.is_empty() { None } else { Some(val.to_string()) },
                "post-cmd" => service.post_cmd = if val.is_empty() { None } else { Some(val.to_string()) },
                "package" => service.package = if val.is_empty() { None } else { Some(val.to_string()) },
                "startup-package" => service.startup_package = if val.is_empty() { None } else { Some(val.to_string()) },
                "user" => service.user = if val.is_empty() { None } else { Some(val.to_string()) },
                "group" => service.group = if val.is_empty() { None } else { Some(val.to_string()) },
                "working-dir" => service.working_dir = if val.is_empty() { None } else { Some(val.to_string()) },
                "restart" => {
                    service.restart = match val {
                        "on-failure" => RestartPolicy::OnFailure,
                        "always" => RestartPolicy::Always,
                        _ => RestartPolicy::Never,
                    };
                }
                "restart-delay" => {
                    service.restart_delay = val.parse().ok();
                }
                "stop-cmd" => service.stop_cmd = if val.is_empty() { None } else { Some(val.to_string()) },
                "timeout-start" => {
                    service.timeout_start = val.parse().ok();
                }
                "timeout-stop" => {
                    service.timeout_stop = val.parse().ok();
                }
                "priority" => {
                    service.priority = val.parse().ok();
                }
                "stdout-log" => service.stdout_log = if val.is_empty() { None } else { Some(val.to_string()) },
                "stderr-log" => service.stderr_log = if val.is_empty() { None } else { Some(val.to_string()) },
                "umask" => service.umask = if val.is_empty() { None } else { Some(val.to_string()) },
                "nice" => {
                    service.nice = val.parse().ok();
                }
                "env" => { service.env = Some(Vec::new()); }
                "dependencies" => { service.dependencies = Some(Vec::new()); }
                "tags" => { service.tags = Some(Vec::new()); }
                "instances" => { service.instances = Some(Vec::new()); }  // <-- Added here
                _ => {}
            }
        }
    }

    if service.name.is_empty() {
        let msg = format!("Service file {:?} missing required 'name'", path);
        log_error(console_logger, file_logger, &timer, LogLevel::Fail, &msg);
        return Err(BloomError::Custom(msg));
    }

    if service.cmd.is_empty() {
        let msg = format!("Service file {:?} missing required 'cmd'", path);
        log_error(console_logger, file_logger, &timer, LogLevel::Fail, &msg);
        return Err(BloomError::Custom(msg));
    }

    if service.restart_delay.is_none() { service.restart_delay = Some(0); }
    if service.timeout_start.is_none() { service.timeout_start = Some(10); }
    if service.timeout_stop.is_none() { service.timeout_stop = Some(5); }
    if service.umask.is_none() { service.umask = Some("022".to_string()); }
    if service.nice.is_none() { service.nice = Some(0); }
    if service.priority.is_none() { service.priority = Some(50); }
    if service.stdout_log.is_none() {
        service.stdout_log = Some(format!("/var/log/verdant/services/{}.out.log", service.name));
    }
    if service.stderr_log.is_none() {
        service.stderr_log = Some(format!("/var/log/verdant/services/{}.err.log", service.name));
    }

    Ok(service)
}

fn log_error(
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
    timer: &ProcessTimer,
    level: LogLevel,
    msg: &str,
) {
    let elapsed = timer.elapsed();
    console_logger.message(level, msg, elapsed);
    file_logger.log(level, msg);
}

