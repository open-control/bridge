//! Rotating file logger for daemon mode.
//!
//! The bridge dataplane must stay responsive, so file logging is implemented as:
//! - a bounded queue (non-blocking `try_send`)
//! - a dedicated thread with buffered writes and periodic flush

use super::{Direction, LogEntry, LogKind, LogLevel};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct FileLogFilter {
    pub include_protocol: bool,
    pub include_debug: bool,
    pub include_system: bool,
}

impl FileLogFilter {
    pub fn should_write(&self, entry: &LogEntry) -> bool {
        match &entry.kind {
            LogKind::Protocol { .. } => self.include_protocol,
            LogKind::Debug { .. } => self.include_debug,
            LogKind::System { .. } => self.include_system,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileLoggerConfig {
    pub path: PathBuf,
    pub max_bytes: u64,
    pub max_files: usize,
    pub flush_interval: Duration,
    pub channel_capacity: usize,
}

pub fn spawn_file_logger(cfg: FileLoggerConfig) -> io::Result<SyncSender<LogEntry>> {
    if let Some(parent) = cfg.path.parent() {
        fs::create_dir_all(parent)?;
    }

    let (file, size) = open_append(&cfg.path)?;
    let (tx, rx) = sync_channel::<LogEntry>(cfg.channel_capacity.max(1));

    thread::Builder::new()
        .name("oc-bridge-file-logger".to_string())
        .spawn(move || run_logger(rx, cfg, file, size))
        .map_err(|e| io::Error::other(e.to_string()))?;

    Ok(tx)
}

fn run_logger(rx: Receiver<LogEntry>, cfg: FileLoggerConfig, file: File, start_size: u64) {
    let mut max_bytes = cfg.max_bytes;
    if max_bytes < 1024 {
        max_bytes = 1024;
    }
    let max_files = cfg.max_files.max(1);
    let flush_interval = if cfg.flush_interval.is_zero() {
        Duration::from_millis(250)
    } else {
        cfg.flush_interval
    };

    let mut writer = BufWriter::new(file);
    let mut size = start_size;
    let mut dirty = false;
    let mut last_flush = Instant::now();

    loop {
        match rx.recv_timeout(flush_interval) {
            Ok(entry) => {
                let line = format_entry(&entry);
                if write_line(&mut writer, &line).is_ok() {
                    size = size.saturating_add(line.len() as u64 + 1);
                    dirty = true;
                }

                if size >= max_bytes {
                    let _ = writer.flush();
                    drop(writer);
                    let _ = rotate_files(&cfg.path, max_files);
                    match open_truncate(&cfg.path) {
                        Ok(f) => {
                            writer = BufWriter::new(f);
                            size = 0;
                            dirty = false;
                            last_flush = Instant::now();
                        }
                        Err(_) => {
                            // If we cannot reopen the file, stop logging.
                            break;
                        }
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if dirty && last_flush.elapsed() >= flush_interval {
                    let _ = writer.flush();
                    dirty = false;
                    last_flush = Instant::now();
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let _ = writer.flush();
                break;
            }
        }
    }
}

fn write_line(writer: &mut BufWriter<File>, line: &str) -> io::Result<()> {
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn format_entry(entry: &LogEntry) -> String {
    match &entry.kind {
        LogKind::System { message } => format!("{} [SYS] {}", entry.timestamp, message),
        LogKind::Debug { level, message } => {
            let level_str = match level {
                Some(LogLevel::Debug) => "[DEBUG]",
                Some(LogLevel::Info) => "[INFO]",
                Some(LogLevel::Warn) => "[WARN]",
                Some(LogLevel::Error) => "[ERROR]",
                None => "[LOG]",
            };
            format!("{} {} {}", entry.timestamp, level_str, message)
        }
        LogKind::Protocol {
            direction,
            message_name,
            size,
        } => {
            let dir = match direction {
                Direction::In => "IN",
                Direction::Out => "OUT",
            };
            format!(
                "{} [PROTO] {} {} ({} B)",
                entry.timestamp, dir, message_name, size
            )
        }
    }
}

fn open_append(path: &Path) -> io::Result<(File, u64)> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    Ok((file, size))
}

fn open_truncate(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
}

fn rotate_files(path: &Path, max_files: usize) -> io::Result<()> {
    if max_files == 0 {
        return Ok(());
    }

    let stem = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "bridge.log".to_string());
    let dir = path.parent().unwrap_or_else(|| Path::new("."));

    // Remove the oldest.
    let oldest = dir.join(format!("{}.{}", stem, max_files));
    let _ = fs::remove_file(&oldest);

    // Shift: N-1 -> N, ... 1 -> 2.
    for i in (1..max_files).rev() {
        let src = dir.join(format!("{}.{}", stem, i));
        let dst = dir.join(format!("{}.{}", stem, i + 1));
        if src.exists() {
            let _ = fs::rename(&src, &dst);
        }
    }

    // Active -> .1
    let first = dir.join(format!("{}.1", stem));
    if path.exists() {
        let _ = fs::rename(path, first);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir() -> PathBuf {
        let base = std::env::temp_dir();
        let pid = std::process::id();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        base.join(format!("oc-bridge-filelog-{}-{}", pid, ts))
    }

    #[test]
    fn test_rotate_files_keeps_max_files() {
        let dir = unique_temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bridge.log");

        fs::write(&path, "active").unwrap();
        fs::write(dir.join("bridge.log.1"), "one").unwrap();
        fs::write(dir.join("bridge.log.2"), "two").unwrap();

        rotate_files(&path, 2).unwrap();

        assert!(dir.join("bridge.log.1").exists());
        assert!(dir.join("bridge.log.2").exists());
        assert!(!dir.join("bridge.log.3").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
