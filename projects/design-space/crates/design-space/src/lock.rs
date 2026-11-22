//! Repository ref lock shared by push, gc and repack.

use anyhow::{Context, Result, bail};
use std::fs;
use std::io;
use std::os::fd::AsRawFd;
use std::path::Path;

pub struct RepoLock {
    file: fs::File,
}

impl RepoLock {
    pub fn acquire(root: &Path) -> Result<Self> {
        let path = root.join("refs.lock");
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("open lock {}", path.display()))?;
        for _ in 0..3000 {
            let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if result == 0 {
                return Ok(Self { file });
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::WouldBlock {
                return Err(error.into());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        bail!("timed out waiting for repository ref lock")
    }
}

impl Drop for RepoLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
