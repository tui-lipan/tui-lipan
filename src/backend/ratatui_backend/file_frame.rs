//! Immutable file snapshots for terminal frames. Pixels enter heap memory only for a capture,
//! composition, or a host which cannot read local files. Host uploads use bounded temporary hard
//! links, so dropping a cached frame cannot delete pixels the host has not opened yet.

use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SNAPSHOT_BUDGET: usize = 256 * 1024 * 1024;
const HOST_BUDGET: usize = 64 * 1024 * 1024;
const HOST_LINK_LIMIT: usize = 32;
const HOST_TIMEOUT: Duration = Duration::from_secs(10);
static SNAPSHOT_BYTES: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct Reservation(usize);

impl Reservation {
    fn new(bytes: usize) -> io::Result<Self> {
        let mut used = SNAPSHOT_BYTES.load(Ordering::Acquire);
        loop {
            let next = used
                .checked_add(bytes)
                .filter(|sum| *sum <= SNAPSHOT_BUDGET)
                .ok_or_else(|| io::Error::other("terminal frame file budget exceeded"))?;
            match SNAPSHOT_BYTES.compare_exchange_weak(
                used,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(Self(bytes)),
                Err(actual) => used = actual,
            }
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        SNAPSHOT_BYTES.fetch_sub(self.0, Ordering::AcqRel);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FilePixels {
    file: Arc<tempfile::NamedTempFile>,
    reservation: Arc<Reservation>,
    pub(crate) format: u32,
}

impl FilePixels {
    pub(crate) fn snapshot(path: &std::path::Path, len: usize, format: u32) -> io::Result<Self> {
        let reservation = Reservation::new(len)?;
        let mut options = File::options();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let source = options.open(path)?;
        let metadata = source.metadata()?;
        if !metadata.is_file() || metadata.len() != len as u64 {
            return Err(io::Error::other("invalid raw frame file length or type"));
        }
        let mut file = tempfile::Builder::new()
            .prefix("tty-graphics-protocol-frame-")
            .tempfile()?;
        // Bounded even if the producer grows its source after the metadata check. std::io::copy
        // uses the kernel file-copy path where available, without a frame-sized heap allocation.
        let copied = io::copy(&mut source.take(len as u64 + 1), file.as_file_mut())?;
        if copied != len as u64 {
            return Err(io::Error::other(
                "raw frame file changed length during snapshot",
            ));
        }
        Ok(Self {
            file: Arc::new(file),
            reservation: Arc::new(reservation),
            format,
        })
    }

    pub(crate) fn read(&self) -> io::Result<Vec<u8>> {
        std::fs::read(self.file.path())
    }

    pub(crate) fn host_link(&self) -> io::Result<HostFile> {
        let mut pending = pending();
        pending.reap(Instant::now());
        if !pending.can_admit(self.reservation.0) {
            return Err(io::Error::other(
                "terminal host file handoff budget exceeded",
            ));
        }
        // tempfile reserves a random name securely. Replace our empty file with a hard link to
        // the immutable snapshot. The producer's original path is never handed to the host.
        let name = tempfile::Builder::new()
            .prefix("tty-graphics-protocol-host-")
            .tempfile()?;
        let path = name.into_temp_path();
        std::fs::remove_file(&path)?;
        std::fs::hard_link(self.file.path(), &path)?;
        let path = path.keep().map_err(|error| error.error)?;
        pending.bytes += self.reservation.0;
        pending.links.push(PendingLink {
            path: path.clone(),
            bytes: self.reservation.0,
            handed_at: None,
        });
        Ok(HostFile {
            path,
            handed: false,
            backing: Some(self.clone()),
        })
    }
}

struct PendingLink {
    path: PathBuf,
    bytes: usize,
    handed_at: Option<Instant>,
}

#[derive(Default)]
struct Pending {
    links: Vec<PendingLink>,
    bytes: usize,
}

impl Pending {
    fn can_admit(&self, bytes: usize) -> bool {
        self.links.len() < HOST_LINK_LIMIT && self.bytes.saturating_add(bytes) <= HOST_BUDGET
    }

    fn reap(&mut self, now: Instant) {
        self.links.retain(|link| {
            if !link.path.exists()
                || link
                    .handed_at
                    .is_some_and(|at| now.duration_since(at) >= HOST_TIMEOUT)
            {
                let _ = std::fs::remove_file(&link.path);
                self.bytes -= link.bytes;
                false
            } else {
                true
            }
        });
    }
}

fn pending() -> std::sync::MutexGuard<'static, Pending> {
    static PENDING: Mutex<Pending> = Mutex::new(Pending {
        links: Vec::new(),
        bytes: 0,
    });
    PENDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn reap_host_links() {
    pending().reap(Instant::now());
}

pub(crate) struct HostFile {
    path: PathBuf,
    handed: bool,
    backing: Option<FilePixels>,
}

impl HostFile {
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }
    pub(crate) fn renew(&mut self) -> io::Result<()> {
        // Retire the failed attempt before reserving another link, so a full budget can retry.
        let _ = std::fs::remove_file(&self.path);
        reap_host_links();
        let fresh = self
            .backing
            .as_ref()
            .ok_or_else(|| io::Error::other("file upload already handed over"))?
            .host_link()?;
        *self = fresh;
        Ok(())
    }

    pub(crate) fn handed_over(&mut self) {
        self.backing = None;
        self.handed = true;
        if let Some(link) = pending()
            .links
            .iter_mut()
            .find(|link| link.path == self.path)
        {
            link.handed_at = Some(Instant::now());
        }
    }
}

impl Drop for HostFile {
    fn drop(&mut self) {
        if !self.handed {
            let _ = std::fs::remove_file(&self.path);
        }
        reap_host_links();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn snapshot_survives_producer_rewrite_and_removal() {
        let mut source = tempfile::NamedTempFile::new().unwrap();
        source.write_all(&[1, 2, 3, 255]).unwrap();
        let frame = FilePixels::snapshot(source.path(), 4, 32).unwrap();
        std::fs::write(source.path(), [5, 6, 7, 255]).unwrap();
        drop(source);
        assert_eq!(frame.read().unwrap(), [1, 2, 3, 255]);
        let path = frame.file.path().to_owned();
        drop(frame);
        assert!(!path.exists());
    }

    #[test]
    fn submitted_link_survives_snapshot_and_cache_drop() {
        let mut source = tempfile::NamedTempFile::new().unwrap();
        source.write_all(&[1, 2, 3, 255]).unwrap();
        let frame = FilePixels::snapshot(source.path(), 4, 32).unwrap();
        let mut link = frame.host_link().unwrap();
        let path = link.path().to_owned();
        link.handed_over();
        drop(frame);
        drop(link);
        assert_eq!(std::fs::read(&path).unwrap(), [1, 2, 3, 255]);
        std::fs::remove_file(path).unwrap();
        reap_host_links();
    }

    #[test]
    fn unsubmitted_link_is_removed() {
        let mut source = tempfile::NamedTempFile::new().unwrap();
        source.write_all(&[1, 2, 3, 255]).unwrap();
        let frame = FilePixels::snapshot(source.path(), 4, 32).unwrap();
        let link = frame.host_link().unwrap();
        let path = link.path().to_owned();
        drop(link);
        assert!(!path.exists());
    }

    #[test]
    fn pending_links_expire_and_return_their_budget() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.into_temp_path().keep().unwrap();
        let now = Instant::now();
        let mut pending = Pending {
            links: vec![PendingLink {
                path: path.clone(),
                bytes: 4,
                handed_at: Some(now - HOST_TIMEOUT),
            }],
            bytes: 4,
        };
        pending.reap(now);
        assert_eq!(pending.bytes, 0);
        assert!(pending.links.is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn host_admission_bounds_bytes_and_link_count() {
        let mut pending = Pending {
            bytes: HOST_BUDGET - 4,
            links: Vec::new(),
        };
        assert!(pending.can_admit(4));
        assert!(!pending.can_admit(5));
        pending.bytes = 0;
        pending.links = (0..HOST_LINK_LIMIT)
            .map(|_| PendingLink {
                path: PathBuf::new(),
                bytes: 0,
                handed_at: None,
            })
            .collect();
        assert!(!pending.can_admit(1));
    }

    #[test]
    fn invalid_lengths_do_not_create_snapshots() {
        let source = tempfile::NamedTempFile::new().unwrap();
        assert!(FilePixels::snapshot(source.path(), 4, 32).is_err());
        assert!(FilePixels::snapshot(source.path(), SNAPSHOT_BUDGET + 1, 32).is_err());
    }
}
