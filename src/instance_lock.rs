use fs2::FileExt;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use crate::error::{BridgeError, Result};

pub struct InstanceLock {
    _file: std::fs::File,
}

impl InstanceLock {
    fn daemon_lock_path(instance_id: &str) -> Result<PathBuf> {
        let dir = crate::config::config_dir()?;
        Self::lock_path_in_dir(&dir, instance_id)
    }

    fn lock_path_in_dir(dir: &Path, instance_id: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(dir).map_err(|e| BridgeError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        Ok(dir.join(format!("oc-bridge.{}.lock", instance_id)))
    }

    fn is_contended_lock_error(e: &std::io::Error) -> bool {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            return true;
        }

        // On Windows, file locking returns OS error codes rather than WouldBlock.
        #[cfg(windows)]
        {
            match e.raw_os_error() {
                // ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION
                Some(32) | Some(33) => return true,
                _ => {}
            }
        }

        false
    }

    pub fn acquire_daemon(instance_id: &str) -> Result<Self> {
        let path = Self::daemon_lock_path(instance_id)?;
        Self::acquire_from_path(path)
    }

    fn acquire_from_path(path: PathBuf) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| BridgeError::InstanceLock {
                path: path.clone(),
                source: e,
            })?;

        match file.try_lock_exclusive() {
            Ok(()) => Ok(Self { _file: file }),
            Err(e) if Self::is_contended_lock_error(&e) => {
                Err(BridgeError::InstanceAlreadyRunning { lock_path: path })
            }
            Err(e) => Err(BridgeError::InstanceLock { path, source: e }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "oc-bridge-lock-test-{}-{}",
            std::process::id(),
            stamp
        ))
    }

    fn acquire_daemon_in_dir(instance_id: &str, dir: &Path) -> Result<InstanceLock> {
        let path = InstanceLock::lock_path_in_dir(dir, instance_id)?;
        InstanceLock::acquire_from_path(path)
    }

    #[test]
    fn test_instance_lock_allows_different_instance_ids() {
        let dir = unique_test_dir();
        let lock_a = acquire_daemon_in_dir("test-a", &dir).unwrap();
        let lock_b = acquire_daemon_in_dir("test-b", &dir).unwrap();
        drop(lock_a);
        drop(lock_b);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_instance_lock_rejects_duplicate_same_instance_id() {
        let dir = unique_test_dir();
        let lock = acquire_daemon_in_dir("test-duplicate", &dir).unwrap();
        let err = match acquire_daemon_in_dir("test-duplicate", &dir) {
            Ok(_) => panic!("expected duplicate instance lock to fail"),
            Err(err) => err,
        };
        assert!(matches!(err, BridgeError::InstanceAlreadyRunning { .. }));
        drop(lock);
        let _ = std::fs::remove_dir_all(dir);
    }
}
