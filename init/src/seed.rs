use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

const SEED_PATH: &str = "/var/lib/verdant/random-seed";
const SEED_SIZE: usize = 512;

/// Seeds the kernel RNG early using saved entropy from previous boot.
///
/// Reads a seed file, writes it to /dev/urandom, and regenerates a new seed.
pub fn seed_entropy(
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    // Step 1: Read previous seed
    let seed = match fs::read(SEED_PATH) {
        Ok(data) if data.len() >= SEED_SIZE => data,
        _ => {
            file_logger.log(LogLevel::Warn, "No usable entropy seed found, skipping seeding");
            console_logger.message(LogLevel::Warn, "Entropy seed missing or too short", timer.elapsed());
            return Ok(()); // Not fatal
        }
    };

    // Step 2: Feed seed to kernel RNG
    match OpenOptions::new().write(true).open("/dev/urandom") {
        Ok(mut urandom) => {
            if let Err(e) = urandom.write_all(&seed) {
                file_logger.log(LogLevel::Warn, &format!("Failed to write seed to /dev/urandom: {}", e));
            }
        }
        Err(_) => {
            file_logger.log(LogLevel::Warn, "Could not open /dev/urandom for writing");
        }
    }

    // Step 3: Generate new seed and persist
    let mut new_seed = vec![0u8; SEED_SIZE];
    let mut rng = File::open("/dev/urandom").map_err(BloomError::Io)?;
    rng.read_exact(&mut new_seed).map_err(BloomError::Io)?;

    if let Some(parent) = Path::new(SEED_PATH).parent() {
        fs::create_dir_all(parent).map_err(BloomError::Io)?;
    }

    fs::write(SEED_PATH, &new_seed).map_err(BloomError::Io)?;

    console_logger.message(LogLevel::Ok, "Kernel RNG seeded", timer.elapsed());
    file_logger.log(LogLevel::Ok, "Early entropy seed loaded and refreshed");

    Ok(())
}

