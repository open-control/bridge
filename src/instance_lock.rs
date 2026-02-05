use fs2::FileExt;
use std::fs::OpenOptions;

use crate::error::{BridgeError, Result};

pub struct InstanceLock {
    _file: std::fs::File,
}

impl InstanceLock {
    fn daemon_lock_path() -> Result<std::path::PathBuf> {
        let dir = crate::config::config_dir()?;
        std::fs::create_dir_all(&dir).map_err(|e| BridgeError::Io {
            path: dir.clone(),
            source: e,
        })?;
        Ok(dir.join("oc-bridge.lock"))
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

    pub fn acquire_daemon() -> Result<Self> {
        let path = Self::daemon_lock_path()?;
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
