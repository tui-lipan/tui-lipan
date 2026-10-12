//! Immutable file snapshots for terminal frames. Pixels enter heap memory only for a capture,
//! composition, or a host which cannot read local files. Host uploads use bounded temporary hard
//! links, so dropping a cached frame cannot delete pixels the host has not opened yet.

use std::fs::File;
use std::io::{self, Read, Seek};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
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

// A stream keeps at most two retired allocations, still charged to SNAPSHOT_BYTES. The pool
// belongs to the terminal screen, not a global cache, so closing/resetting it releases its spares.
const SPARE_LIMIT: usize = 2;
const SPARE_BUDGET: usize = 32 * 1024 * 1024;

#[derive(Debug)]
struct SnapshotFile {
    file: tempfile::NamedTempFile,
    reservation: Reservation,
    path: Arc<std::path::Path>,
}

#[derive(Debug)]
struct Spares {
    files: Vec<SnapshotFile>,
    budget: usize,
}

impl Spares {
    fn bytes(&self) -> usize {
        self.files.iter().map(|file| file.reservation.0).sum()
    }

    fn put(&mut self, file: SnapshotFile) {
        if self.files.len() < SPARE_LIMIT
            && self.bytes().saturating_add(file.reservation.0) <= self.budget
        {
            self.files.push(file);
        }
    }
}

#[derive(Debug)]
pub(crate) struct SnapshotPool {
    spares: Mutex<Spares>,
}

impl SnapshotPool {
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            spares: Mutex::new(Spares {
                files: Vec::new(),
                budget: budget.min(SPARE_BUDGET),
            }),
        }
    }

    fn spares(&self) -> std::sync::MutexGuard<'_, Spares> {
        self.spares
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn set_budget(&self, budget: usize) {
        let mut spares = self.spares();
        spares.budget = budget.min(SPARE_BUDGET);
        while spares.bytes() > spares.budget {
            spares.files.pop();
        }
    }

    fn destination(&self, len: usize) -> io::Result<SnapshotFile> {
        let mut spares = self.spares();
        // Resize retires obsolete capacities instead of accumulating one pool per frame size.
        spares.files.retain(|file| file.reservation.0 == len);
        if let Some(mut file) = spares.files.pop() {
            file.file.as_file_mut().rewind()?;
            return Ok(file);
        }
        drop(spares);
        let reservation = Reservation::new(len)?;
        let file = tempfile::Builder::new()
            .prefix("tty-graphics-protocol-frame-")
            .tempfile()?;
        let path = Arc::from(file.path());
        Ok(SnapshotFile {
            file,
            reservation,
            path,
        })
    }

    pub(crate) fn snapshot(
        self: &Arc<Self>,
        path: &std::path::Path,
        len: usize,
        format: u32,
    ) -> io::Result<FilePixels> {
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
        let mut file = self.destination(len)?;
        // Do not truncate an equal-size retired destination: tmpfs can overwrite its allocated
        // pages. Bounded copying still detects a producer changing length during this operation.
        // A failed copy drops the destination, never returning partly written storage to the pool.
        let copied = io::copy(&mut source.take(len as u64 + 1), file.file.as_file_mut())?;
        if copied != len as u64 {
            return Err(io::Error::other(
                "raw frame file changed length during snapshot",
            ));
        }
        Ok(FilePixels {
            path: Arc::clone(&file.path),
            len,
            storage: Arc::new(Snapshot {
                file: Some(file),
                pool: Arc::downgrade(self),
                host_exposed: AtomicBool::new(false),
            }),
            format,
        })
    }
}

#[derive(Debug)]
struct Snapshot {
    file: Option<SnapshotFile>,
    pool: Weak<SnapshotPool>,
    host_exposed: AtomicBool,
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        // Ghostty can unlink a handoff path while still reading an open descriptor. Neither
        // disappearance of the link nor the handoff timeout proves the inode is safe to rewrite.
        // Any host-exposed allocation is therefore permanently excluded from reuse.
        if self.host_exposed.load(Ordering::Acquire) {
            return;
        }
        if let Some(pool) = self.pool.upgrade()
            && let Some(file) = self.file.take()
        {
            pool.spares().put(file);
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FilePixels {
    path: Arc<std::path::Path>,
    len: usize,
    storage: Arc<Snapshot>,
    pub(crate) format: u32,
}

impl FilePixels {
    #[cfg(test)]
    pub(crate) fn snapshot(path: &std::path::Path, len: usize, format: u32) -> io::Result<Self> {
        Arc::new(SnapshotPool::new(SPARE_BUDGET)).snapshot(path, len, format)
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn read(&self) -> io::Result<Vec<u8>> {
        std::fs::read(self.path())
    }

    pub(crate) fn host_link(&self) -> io::Result<HostFile> {
        let mut pending = pending();
        pending.reap(Instant::now());
        if !pending.can_admit(self.len()) {
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
        std::fs::hard_link(self.path(), &path)?;
        self.storage.host_exposed.store(true, Ordering::Release);
        let path = path.keep().map_err(|error| error.error)?;
        pending.bytes += self.len();
        pending.links.push(PendingLink {
            path: path.clone(),
            bytes: self.len(),
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
        let path = frame.path().to_owned();
        drop(frame);
        assert!(!path.exists());
    }

    #[test]
    fn retired_storage_is_reused_only_after_the_last_reader() {
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), [1; 4]).unwrap();
        let pool = Arc::new(SnapshotPool::new(8));
        let first = pool.snapshot(source.path(), 4, 32).unwrap();
        let first_path = first.path().to_owned();
        let reader = first.clone();
        drop(first);
        std::fs::write(source.path(), [2; 4]).unwrap();
        let second = pool.snapshot(source.path(), 4, 32).unwrap();
        assert_ne!(second.path(), first_path);
        assert_eq!(reader.read().unwrap(), [1; 4]);
        drop(reader);
        std::fs::write(source.path(), [3; 4]).unwrap();
        let third = pool.snapshot(source.path(), 4, 32).unwrap();
        assert_eq!(third.path(), first_path);
        assert_eq!(third.read().unwrap(), [3; 4]);
        assert_eq!(second.read().unwrap(), [2; 4]);
    }

    #[test]
    fn unlinked_host_storage_is_never_reused() {
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), [1; 4]).unwrap();
        let pool = Arc::new(SnapshotPool::new(8));
        let frame = pool.snapshot(source.path(), 4, 32).unwrap();
        let snapshot_path = frame.path().to_owned();
        let mut link = frame.host_link().unwrap();
        let mut host_reader = File::open(link.path()).unwrap();
        link.handed_over();
        std::fs::remove_file(link.path()).unwrap();
        drop(link);
        drop(frame);
        assert!(pool.spares().files.is_empty());
        std::fs::write(source.path(), [2; 4]).unwrap();
        let next = pool.snapshot(source.path(), 4, 32).unwrap();
        assert_ne!(next.path(), snapshot_path);
        let mut pixels = Vec::new();
        host_reader.read_to_end(&mut pixels).unwrap();
        assert_eq!(pixels, [1; 4]);
        reap_host_links();
    }

    #[test]
    fn pool_drop_releases_spares_but_preserves_live_frames() {
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), [1; 4]).unwrap();
        let pool = Arc::new(SnapshotPool::new(8));
        let live = pool.snapshot(source.path(), 4, 32).unwrap();
        let retired = pool.snapshot(source.path(), 4, 32).unwrap();
        let retired_path = retired.path().to_owned();
        let live_path = live.path().to_owned();
        drop(retired);
        assert!(retired_path.exists());
        drop(pool);
        assert!(!retired_path.exists());
        assert_eq!(live.read().unwrap(), [1; 4]);
        drop(live);
        assert!(!live_path.exists());
    }

    #[test]
    fn spares_obey_count_budget_resize_and_budget_reduction() {
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), [1; 4]).unwrap();
        let pool = Arc::new(SnapshotPool::new(12));
        let frames: Vec<_> = (0..3)
            .map(|_| pool.snapshot(source.path(), 4, 32).unwrap())
            .collect();
        let paths: Vec<_> = frames.iter().map(|frame| frame.path().to_owned()).collect();
        drop(frames);
        assert_eq!(pool.spares().files.len(), 2);
        assert_eq!(pool.spares().bytes(), 8);
        assert_eq!(paths.iter().filter(|path| path.exists()).count(), 2);
        pool.set_budget(4);
        assert_eq!(pool.spares().bytes(), 4);
        std::fs::write(source.path(), [2; 8]).unwrap();
        let resized = pool.snapshot(source.path(), 8, 32).unwrap();
        assert!(paths.iter().all(|path| !path.exists()));
        let resized_path = resized.path().to_owned();
        drop(resized);
        assert!(!resized_path.exists());
        assert!(pool.spares().files.is_empty());
    }

    #[test]
    fn invalid_source_does_not_consume_retired_storage() {
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), [1; 4]).unwrap();
        let pool = Arc::new(SnapshotPool::new(8));
        let frame = pool.snapshot(source.path(), 4, 32).unwrap();
        let path = frame.path().to_owned();
        drop(frame);
        std::fs::write(source.path(), [2; 3]).unwrap();
        assert!(pool.snapshot(source.path(), 4, 32).is_err());
        assert_eq!(pool.spares().bytes(), 4);
        assert_eq!(std::fs::read(path).unwrap(), [1; 4]);
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
