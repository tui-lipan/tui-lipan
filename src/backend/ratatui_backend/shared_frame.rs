//! Frame pixels handed to the host terminal through POSIX shared memory (`t=s`).
//!
//! The Kitty graphics protocol's usual medium is the escape sequence itself: compress the pixels,
//! base64 them, chunk the result, write it down stdout. For a still picture that is fine. For a pane
//! whose child redraws a full window every frame it is the frame rate: a few megabytes of pixels
//! cost a deflate pass, a base64 pass that grows them by a third, and a write of the whole thing,
//! every frame, and the terminal pays to undo all three.
//!
//! Shared memory replaces that with one `write` into an object the host reads directly. The escape
//! sequence carries only the object's *name*, about a hundred bytes. The host unlinks the object
//! once it has the pixels, which is what makes the medium self-cleaning rather than a temporary file
//! somebody has to remember to delete.
//!
//! A terminal that declines shared memory may still read a temporary file (`t=t`) - Ghostty on
//! macOS does. The objects here then live in the temporary directory instead and everything else is
//! the same: the host deletes the name it was handed, and pooling works through hard links there.
//!
//! Not every terminal implements either, so [`host_medium`] records what the startup probe
//! found and the caller keeps the inline path for everything else. The probe is not put to every
//! host either: see [`worth_asking_about_shared_memory`] for which ones are asked, and why asking
//! the rest costs more than the answer is worth.
//!
//! # Why the objects are pooled
//!
//! A fresh object per frame is a fresh *allocation* per frame: every byte written lands on a page
//! that does not exist yet, so the kernel faults it in and zeroes it first. Measured on 1.9 MB
//! frames that is 0.80 ms, about 2.4 GB/s - far below what the machine copies memory at. Writing
//! the same bytes into an object whose pages are already resident costs 0.05 ms.
//!
//! So an object is created once and written again every frame, and each frame hands the host a
//! fresh *hard link* to it rather than a fresh object. The host still gets a name of its own to
//! unlink, and that unlink is what says it is finished: a pool slot is reusable once the name it
//! handed out has gone. Nothing here bets on the host being quick - a slot whose name is still
//! there is simply not reused, and a frame that finds every slot busy falls back to allocating,
//! which is what every frame used to do.

#[cfg(all(unix, not(target_arch = "wasm32")))]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicU8, Ordering};

/// Where a named frame lives, which is also the Kitty `t=` key that names it.
///
/// Shared memory is the cheaper of the two, but not every terminal that reads named frames reads
/// it: Ghostty on macOS declines `t=s` and accepts `t=t`. A temporary file costs a filesystem
/// write rather than a memory copy, which is still nothing next to deflating and base64-ing the
/// pixels down the PTY.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Medium {
    /// `t=s`: a POSIX shared-memory object.
    SharedMemory,
    /// `t=t`: a file in the temporary directory, whose path names the graphics protocol as the
    /// protocol requires. The host deletes it once read.
    TempFile,
}

impl Medium {
    /// The transmission-medium key for this medium.
    pub(crate) fn key(self) -> char {
        match self {
            Self::SharedMemory => 's',
            Self::TempFile => 't',
        }
    }
}

/// What the startup probe found: 0 for neither medium, else `1 + Medium as u8`.
static HOST_MEDIUM: AtomicU8 = AtomicU8::new(0);

#[cfg(all(unix, not(target_arch = "wasm32")))]
/// Names are per-process and monotonic, so a frame never collides with one still in flight, and a
/// previous run's leftovers cannot be mistaken for this run's.
static NEXT_NAME: AtomicU64 = AtomicU64::new(0);

/// Record what the startup probe found. See [`host_medium`].
pub(crate) fn note_host_medium(medium: Option<Medium>) {
    let value = match medium {
        None => 0,
        Some(Medium::SharedMemory) => 1,
        Some(Medium::TempFile) => 2,
    };
    HOST_MEDIUM.store(value, Ordering::Release);
}

/// The medium to hand frames to the host through, or `None` to send them inline.
pub(crate) fn host_medium() -> Option<Medium> {
    match HOST_MEDIUM.load(Ordering::Acquire) {
        1 => Some(Medium::SharedMemory),
        2 => Some(Medium::TempFile),
        _ => None,
    }
}

/// Whether the host reads named frames at all, through either medium.
///
/// A host that reads either one also reads the `t=t` hard links file-backed pane frames are
/// forwarded as: both media are named, consumed reads of a local object.
pub(crate) fn host_reads_named_frames() -> bool {
    host_medium().is_some()
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
/// The prefix of every temporary file this process hands the host. Kitty and Ghostty only read a
/// `t=t` path that contains `tty-graphics-protocol`.
const TEMP_PREFIX: &str = "tty-graphics-protocol-tui-lipan-";

#[cfg(all(unix, not(target_arch = "wasm32")))]
/// A fresh name for `medium`: a shared-memory name with one leading slash and no others, or a
/// path in the temporary directory. `pool` marks an object the pool keeps rather than hands over.
fn next_name(medium: Medium, pool: bool) -> Option<String> {
    let pool = if pool { "pool-" } else { "" };
    let pid = std::process::id();
    let n = NEXT_NAME.fetch_add(1, Ordering::Relaxed);
    match medium {
        Medium::SharedMemory => Some(format!("/tui-lipan-{pool}{pid}-{n}")),
        Medium::TempFile => std::env::temp_dir()
            .join(format!("{TEMP_PREFIX}{pool}{pid}-{n}"))
            .into_os_string()
            .into_string()
            .ok(),
    }
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
mod imp {
    #![allow(unsafe_code)]

    use std::ffi::{CStr, CString};
    use std::io::Write as _;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use super::Medium;

    /// How many objects to keep alive for reuse.
    ///
    /// Read together with [`GRACE`], which the two of them are only meaningful as a pair: a slot
    /// cannot come round again until its grace has passed, so the pool sustains
    /// `SLOTS / GRACE` frames a second and *every* frame above that allocates instead. Eight slots
    /// against a 50 ms grace is 160 fps, comfortably past the 120 fps loop; four would have put the
    /// cliff at 80, close enough to a fast producer to fall off it silently.
    const SLOTS: usize = 8;

    /// A slot is not reused within this long of being handed over, however quickly the name
    /// disappears. The protocol has the host read the object and then unlink it, so the name going
    /// away means it is finished - but a host that unlinked first and read second would be within
    /// its rights to be slow about the read, and nothing here can see that happen.
    ///
    /// Raising this lowers the frame rate the pool can sustain. See [`SLOTS`].
    const GRACE: Duration = Duration::from_millis(50);

    /// Total resident bytes the pool may hold. A pane large enough to blow through this keeps the
    /// allocate-per-frame path rather than pinning a large multiple of itself in RAM.
    const POOL_BUDGET: usize = 64 * 1024 * 1024;

    static POOL: Mutex<Pool> = Mutex::new(Pool {
        slots: Vec::new(),
        claimed: 0,
        claimed_bytes: 0,
    });

    struct Pool {
        slots: Vec<Slot>,
        /// Slots currently out with a frame. Counted so the pool cannot grow past [`SLOTS`] by
        /// creating a new slot for every frame while the others are away.
        claimed: usize,
        /// What those slots hold, so [`POOL_BUDGET`] counts the memory that is pinned rather than
        /// only the part of it sitting idle here.
        claimed_bytes: usize,
    }

    impl Pool {
        /// Every byte this pool is keeping resident, out with a frame or not.
        fn resident(&self) -> usize {
            self.claimed_bytes + self.slots.iter().map(|slot| slot.len).sum::<usize>()
        }
    }

    /// The pool, through a poisoned lock as readily as a healthy one.
    ///
    /// A panic anywhere under this lock would otherwise turn pooling off for the rest of the
    /// process, and the state it guards is a list of mappings - there is no invariant a panic could
    /// leave half-built that reusing a slot would then act on.
    fn pool() -> std::sync::MutexGuard<'static, Pool> {
        POOL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// One reusable object: created and sized once, written again every frame.
    struct Slot {
        /// The pool object's own name, which is never handed to the host. Frames are handed hard
        /// links to it instead, so the host's unlink takes a link and leaves the object.
        source: CString,
        /// The same object as a filesystem path, which is what `link` needs.
        source_path: CString,
        medium: Medium,
        backing: Backing,
        len: usize,
        /// The link most recently handed to the host, and when. `None` means nothing is outstanding.
        handed: Option<(CString, Instant)>,
    }

    /// How a slot's pixels are written.
    enum Backing {
        /// Shared memory, written through a mapping: a copy into pages that are already resident.
        Mapped(*mut libc::c_void),
        /// A temporary file, written with `pwrite`. Never mapped: a store into a mapped file page
        /// that the filesystem cannot back - a full disk, or any overwrite on a copy-on-write
        /// filesystem such as APFS - raises `SIGBUS` and takes the process down, where a write
        /// just fails and the frame falls back to inline.
        File(std::fs::File),
    }

    // SAFETY: the mapping is `MAP_SHARED` memory owned by this slot alone; the pool hands a slot to
    // one thread at a time and takes it back before another can claim it.
    unsafe impl Send for Slot {}

    impl Slot {
        /// Put `pixels`, exactly `len` bytes, into the slot.
        fn write(&self, pixels: &[u8]) -> std::io::Result<()> {
            debug_assert_eq!(self.len, pixels.len());
            match &self.backing {
                Backing::Mapped(map) => {
                    // SAFETY: the mapping is exactly `self.len` bytes, which is the length the
                    // slot was claimed for, and no other thread holds this slot.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            pixels.as_ptr(),
                            *map as *mut u8,
                            pixels.len().min(self.len),
                        )
                    };
                    Ok(())
                }
                Backing::File(file) => {
                    use std::os::unix::fs::FileExt as _;
                    file.write_all_at(pixels, 0)
                }
            }
        }

        /// Whether the host is demonstrably finished with what this slot last handed over.
        fn reusable(&self) -> bool {
            let Some((link, at)) = &self.handed else {
                return true;
            };
            // SAFETY: `link` is a CString this slot created; `F_OK` only asks whether the name
            // still exists, which is the host's completion signal.
            at.elapsed() >= GRACE && unsafe { libc::access(link.as_ptr(), libc::F_OK) } != 0
        }

        fn destroy(self) {
            if let Backing::Mapped(map) = self.backing {
                unsafe { libc::munmap(map, self.len) };
            }
            remove(self.medium, &self.source);
            if let Some((link, _)) = &self.handed {
                unsafe { libc::unlink(link.as_ptr()) };
            }
        }
    }

    /// Where a name lands in the filesystem, which is the only way to hard-link one object under a
    /// second name. Linux puts the shared-memory namespace on a tmpfs mount; a platform that does
    /// not is simply left with the allocate-per-frame path. A temporary file is its own path.
    fn link_path(medium: Medium, name: &str) -> Option<CString> {
        match medium {
            Medium::SharedMemory if cfg!(target_os = "linux") => {
                CString::new(format!("/dev/shm{name}")).ok()
            }
            Medium::SharedMemory => None,
            Medium::TempFile => CString::new(name).ok(),
        }
    }

    /// Create `name` exclusively, readable and writable by this user only. `O_EXCL` so a name that
    /// somehow already exists is an error rather than a frame silently overwriting something.
    fn create(medium: Medium, name: &CStr) -> libc::c_int {
        let flags = libc::O_CREAT | libc::O_EXCL | libc::O_RDWR;
        // SAFETY: `name` is a valid C string for the duration of the call.
        unsafe {
            match medium {
                Medium::SharedMemory => libc::shm_open(name.as_ptr(), flags, 0o600),
                Medium::TempFile => libc::open(
                    name.as_ptr(),
                    flags | libc::O_CLOEXEC,
                    0o600 as libc::c_uint,
                ),
            }
        }
    }

    /// Remove `name`, whether or not anyone still holds it open.
    fn remove(medium: Medium, name: &CStr) {
        // SAFETY: `name` is a valid C string for the duration of the call.
        unsafe {
            match medium {
                Medium::SharedMemory => libc::shm_unlink(name.as_ptr()),
                Medium::TempFile => libc::unlink(name.as_ptr()),
            };
        }
    }

    /// Remove pool objects left behind by a run that was killed before it could clean up. Their
    /// names carry the process id that made them, so a dead one is unambiguous.
    fn sweep_dead_runs(medium: Medium) {
        let (directory, prefix) = match medium {
            Medium::SharedMemory => (std::path::PathBuf::from("/dev/shm"), "tui-lipan-"),
            Medium::TempFile => (std::env::temp_dir(), super::TEMP_PREFIX),
        };
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(rest) = name.strip_prefix(prefix) else {
                continue;
            };
            let rest = rest.strip_prefix("pool-").unwrap_or(rest);
            let Some(pid) = rest
                .split('-')
                .next()
                .and_then(|pid| pid.parse::<i32>().ok())
            else {
                continue;
            };
            if pid == std::process::id() as i32 {
                continue;
            }
            // ESRCH is the only answer that means the owner is gone; EPERM says it is alive and
            // belongs to somebody else, which is not ours to remove.
            let alive = unsafe { libc::kill(pid, 0) } == 0
                || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
            // The temporary directory can be shared, so a matching name proves nothing about who
            // made it: only this user's own regular files are leftovers of ours.
            let ours = || {
                use std::os::unix::fs::MetadataExt as _;
                std::fs::symlink_metadata(entry.path()).is_ok_and(|metadata| {
                    metadata.is_file() && metadata.uid() == unsafe { libc::geteuid() }
                })
            };
            if !alive && ours() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    fn create_slot(len: usize, medium: Medium) -> Option<Slot> {
        use std::os::fd::FromRawFd as _;

        let name = super::next_name(medium, true)?;
        let source_path = link_path(medium, &name)?;
        let source = CString::new(name).ok()?;
        let descriptor = create(medium, &source);
        if descriptor < 0 {
            return None;
        }
        // SAFETY: `create` just produced this descriptor and nothing else holds a copy, so the
        // `File` owns it and closes it on every path out of here. The mapping outlives the
        // descriptor, which is what `mmap` guarantees.
        let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
        if medium == Medium::TempFile {
            // Sized by its first write, which can fail rather than fault. See `Backing::File`.
            return Some(Slot {
                source,
                source_path,
                medium,
                backing: Backing::File(file),
                len,
                handed: None,
            });
        }
        let Some(length) = libc::off_t::try_from(len).ok() else {
            remove(medium, &source);
            return None;
        };
        if unsafe { libc::ftruncate(descriptor, length) } < 0 {
            remove(medium, &source);
            return None;
        }
        let map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                descriptor,
                0,
            )
        };
        drop(file);
        if map == libc::MAP_FAILED {
            remove(medium, &source);
            return None;
        }
        // Touch every page once here rather than during the first frame, so the frame that happens
        // to create a slot is not the slow one.
        unsafe { std::ptr::write_bytes(map as *mut u8, 0, len) };
        Some(Slot {
            source,
            source_path,
            medium,
            backing: Backing::Mapped(map),
            len,
            handed: None,
        })
    }

    /// Take a slot able to hold `len` bytes, or `None` to say the caller should allocate.
    fn claim(len: usize, medium: Medium) -> Option<Slot> {
        // Swept outside the lock: a machine with a lot of shared memory (a browser leaves hundreds
        // of objects there) makes this a directory walk plus a signal per entry, and no frame
        // should wait behind it.
        static SWEEP_SHARED: std::sync::Once = std::sync::Once::new();
        static SWEEP_TEMP: std::sync::Once = std::sync::Once::new();
        match medium {
            Medium::SharedMemory => SWEEP_SHARED.call_once(|| sweep_dead_runs(medium)),
            Medium::TempFile => SWEEP_TEMP.call_once(|| sweep_dead_runs(medium)),
        }

        let mut pool = pool();
        if let Some(index) = pool
            .slots
            .iter()
            .position(|slot| slot.len == len && slot.medium == medium && slot.reusable())
        {
            pool.claimed += 1;
            pool.claimed_bytes += len;
            return Some(pool.slots.swap_remove(index));
        }
        let resident = pool.resident();
        if pool.slots.len() + pool.claimed >= SLOTS || resident + len > POOL_BUDGET {
            // A pane that changed size leaves slots no frame of this size can use. One of those is
            // what makes room; if there is none, every slot is either the wrong size and still out
            // with a frame, or the right size and not finished with, so this frame allocates.
            let index = pool
                .slots
                .iter()
                .position(|slot| (slot.len != len || slot.medium != medium) && slot.reusable())?;
            pool.slots.swap_remove(index).destroy();
            if pool.resident() + len > POOL_BUDGET {
                return None;
            }
        }
        let slot = create_slot(len, medium)?;
        pool.claimed += 1;
        pool.claimed_bytes += len;
        Some(slot)
    }

    fn release(slot: Slot) {
        let mut pool = pool();
        pool.claimed = pool.claimed.saturating_sub(1);
        pool.claimed_bytes = pool.claimed_bytes.saturating_sub(slot.len);
        pool.slots.push(slot);
    }

    /// Give up a claimed slot without returning it to the pool.
    fn discard(slot: Slot) {
        let mut pool = pool();
        pool.claimed = pool.claimed.saturating_sub(1);
        pool.claimed_bytes = pool.claimed_bytes.saturating_sub(slot.len);
        drop(pool);
        slot.destroy();
    }

    impl Pool {
        fn trim_idle(&mut self) {
            let mut index = 0;
            while index < self.slots.len() {
                let slot = &self.slots[index];
                let idle = slot
                    .handed
                    .as_ref()
                    .is_none_or(|(_, at)| at.elapsed() >= Duration::from_secs(1));
                if idle && slot.reusable() {
                    self.slots.swap_remove(index).destroy();
                } else {
                    index += 1;
                }
            }
        }
    }

    pub(crate) fn trim_idle_pool() {
        pool().trim_idle();
    }

    /// Pixels in a named object - shared memory or a temporary file - waiting for the host to read
    /// them.
    ///
    /// Owns the name until the host is told about it. A frame that is encoded and then never
    /// drawn - a pane closed mid-encode, a placement replaced by a newer one - would otherwise leave
    /// its object behind, one per frame, until the machine reboots.
    pub(crate) struct SharedFrame {
        name: CString,
        medium: Medium,
        /// The pool slot backing this frame, held until the transmission is written so nothing can
        /// overwrite the pixels the host has not been told about yet. `None` for a frame that
        /// allocated its own object.
        slot: Option<Slot>,
        backing: Option<std::fs::File>,
        // Unlinking an unread retry name cannot prove that a reader with an open descriptor is
        // finished. Retire that backing object instead of ever overwriting its pixels.
        reuse_after_retry: bool,
        /// Cleared once the host has been told the name, since reading is what unlinks the name
        /// and the host is the reader.
        owned: bool,
    }

    impl SharedFrame {
        /// Put `pixels` in shared memory, or `None` if the namespace refused.
        #[cfg(test)]
        pub(crate) fn write(pixels: &[u8]) -> Option<Self> {
            Self::write_in(Medium::SharedMemory, pixels)
        }

        /// Put `pixels` where the host can read them through `medium`, or `None` if that
        /// namespace refused.
        pub(crate) fn write_in(medium: Medium, pixels: &[u8]) -> Option<Self> {
            match Self::pooled(medium, pixels) {
                Some(frame) => Some(frame),
                None => Self::allocated(medium, pixels),
            }
        }

        /// Copy into a reused object and hand the host a fresh link to it.
        fn pooled(medium: Medium, pixels: &[u8]) -> Option<Self> {
            let slot = claim(pixels.len(), medium)?;
            let named = super::next_name(medium, false)
                .and_then(|name| Some((link_path(medium, &name)?, CString::new(name).ok()?)));
            let Some((link, name)) = named else {
                release(slot);
                return None;
            };
            if slot.write(pixels).is_err() {
                // Out of space, most likely. Allocating will fail the same way and send the frame
                // inline, which is the right outcome.
                discard(slot);
                return None;
            }
            if unsafe { libc::link(slot.source_path.as_ptr(), link.as_ptr()) } < 0 {
                // Hard links inside the namespace are what the whole scheme rests on, so a failure
                // here means this platform cannot pool at all rather than that this frame is
                // unlucky. Drop the slot and let the caller allocate.
                discard(slot);
                return None;
            }
            Some(Self {
                name,
                medium,
                // The clock starts when the host is told, not now. See `handed_over`.
                slot: Some(Slot {
                    handed: Some((link, Instant::now())),
                    ..slot
                }),
                owned: true,
                backing: None,
                reuse_after_retry: true,
            })
        }

        /// Put `pixels` in a fresh object, which is what happens when every slot is still out.
        fn allocated(medium: Medium, pixels: &[u8]) -> Option<Self> {
            use std::os::fd::FromRawFd as _;

            let length = libc::off_t::try_from(pixels.len()).ok()?;
            let name = CString::new(super::next_name(medium, false)?).ok()?;
            let descriptor = create(medium, &name);
            if descriptor < 0 {
                return None;
            }
            // SAFETY: `create` just produced this descriptor and nothing else holds a copy, so the
            // `File` is its sole owner and closes it on every path out of here.
            let mut file = unsafe { std::fs::File::from_raw_fd(descriptor) };
            let mut frame = Self {
                name,
                medium,
                slot: None,
                backing: None,
                reuse_after_retry: true,
                owned: true,
            };
            if unsafe { libc::ftruncate(descriptor, length) } < 0 {
                return None;
            }
            file.write_all(pixels).ok()?;
            frame.backing = Some(file);
            Some(frame)
        }

        /// Give a retry its own consumable name while keeping the pixels alive independently.
        pub(crate) fn renew(&mut self) -> std::io::Result<()> {
            use std::io::{Seek as _, SeekFrom};
            use std::os::fd::FromRawFd as _;

            let medium = self.medium;
            let name = super::next_name(medium, false)
                .ok_or_else(|| std::io::Error::other("frame name unavailable"))?;
            let name = CString::new(name).map_err(std::io::Error::other)?;
            if let Some(slot) = self.slot.as_mut() {
                let link = link_path(medium, name.to_str().unwrap_or_default())
                    .ok_or_else(|| std::io::Error::other("frame path unavailable"))?;
                if unsafe { libc::link(slot.source_path.as_ptr(), link.as_ptr()) } < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if let Some((old_link, _)) = &slot.handed {
                    // SAFETY: the slot owns this CString; access only checks whether the host has
                    // removed the link after reading. An existing link requires retiring the slot.
                    self.reuse_after_retry &=
                        unsafe { libc::access(old_link.as_ptr(), libc::F_OK) } != 0;
                }
                slot.handed = Some((link, Instant::now()));
            } else {
                let source = self
                    .backing
                    .as_mut()
                    .ok_or_else(|| std::io::Error::other("shared upload already handed over"))?;
                let descriptor = create(medium, &name);
                if descriptor < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // SAFETY: this descriptor was just created and File becomes its sole owner.
                let mut file = unsafe { std::fs::File::from_raw_fd(descriptor) };
                let copied = (|| {
                    let length = libc::off_t::try_from(source.metadata()?.len())
                        .map_err(std::io::Error::other)?;
                    if unsafe { libc::ftruncate(descriptor, length) } < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    source.seek(SeekFrom::Start(0))?;
                    std::io::copy(source, &mut file)
                })();
                if let Err(error) = copied {
                    remove(medium, &name);
                    return Err(error);
                }
                self.backing = Some(file);
            }
            remove(medium, &self.name);
            self.name = name;
            Ok(())
        }

        /// A failed attempt may already have an open reader even when no retry is painted.
        pub(crate) fn abandon_attempt(&mut self) {
            self.reuse_after_retry = false;
        }

        /// The object's name, for the transmission payload.
        pub(crate) fn name(&self) -> &str {
            self.name.to_str().unwrap_or_default()
        }

        /// The medium the name belongs to, which picks the transmission's `t=` key.
        pub(crate) fn medium(&self) -> Medium {
            self.medium
        }

        /// The host has been told the name and will unlink it once it has read the pixels.
        ///
        /// This is where a pooled slot goes back: the pixels are the host's to read now, and the
        /// slot's own record of the link it handed out is what keeps the next frame from
        /// overwriting them before it has.
        pub(crate) fn handed_over(&mut self) {
            self.owned = false;
            self.backing = None;
            if let Some(mut slot) = self.slot.take() {
                // The grace runs from here rather than from the write. An encode can finish long
                // before the frame it produced is painted - the widget path encodes on a worker -
                // and a grace that had already elapsed by the time the host first heard the name
                // would be no grace at all.
                if let Some((_, at)) = slot.handed.as_mut() {
                    *at = Instant::now();
                }
                self.finish_slot(slot);
            }
        }

        fn finish_slot(&self, mut slot: Slot) {
            if self.reuse_after_retry {
                release(slot);
            } else {
                // Leave the current host link alive after handoff, while closing our mapping and
                // private source. Any already-open reader keeps its own immutable inode alive.
                slot.handed = None;
                discard(slot);
            }
        }
    }

    impl Drop for SharedFrame {
        fn drop(&mut self) {
            if self.owned {
                remove(self.medium, &self.name);
            }
            if let Some(mut slot) = self.slot.take() {
                // Never handed over, so nothing outside this process ever knew the link existed and
                // the slot is free the moment it goes back.
                slot.handed = None;
                self.finish_slot(slot);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn shm_name() -> String {
            super::super::next_name(Medium::SharedMemory, false).unwrap()
        }

        #[cfg(target_os = "linux")]
        fn private_pooled_frame(pixels: &[u8]) -> SharedFrame {
            // A private slot avoids depending on other tests filling the process-wide pool.
            let mut slot = create_slot(pixels.len(), Medium::SharedMemory).unwrap();
            let name = CString::new(shm_name()).unwrap();
            let link = link_path(Medium::SharedMemory, name.to_str().unwrap()).unwrap();
            assert_eq!(
                unsafe { libc::link(slot.source_path.as_ptr(), link.as_ptr()) },
                0
            );
            slot.write(pixels).unwrap();
            slot.handed = Some((link, Instant::now()));
            {
                let mut pool = pool();
                pool.claimed += 1;
                pool.claimed_bytes += slot.len;
            }
            SharedFrame {
                name,
                medium: Medium::SharedMemory,
                slot: Some(slot),
                backing: None,
                owned: true,
                reuse_after_retry: true,
            }
        }

        #[test]
        #[cfg(target_os = "linux")]
        fn abandoned_attempt_retires_backing_without_a_retry() {
            let pixels = [1, 2, 3, 255];
            let mut frame = private_pooled_frame(&pixels);
            let source = frame
                .slot
                .as_ref()
                .unwrap()
                .source_path
                .to_str()
                .unwrap()
                .to_owned();
            let mut reader = std::fs::File::open(format!("/dev/shm{}", frame.name())).unwrap();
            frame.abandon_attempt();
            drop(frame);
            assert!(!std::path::Path::new(&source).exists());
            use std::io::Read as _;
            let mut read = Vec::new();
            reader.read_to_end(&mut read).unwrap();
            assert_eq!(read, pixels);
        }

        #[test]
        #[cfg(target_os = "linux")]
        fn retry_of_an_unread_pooled_upload_retires_its_backing() {
            let pixels = [1, 2, 3, 255];
            let mut frame = private_pooled_frame(&pixels);
            let slot = frame.slot.as_ref().unwrap();
            let source = slot.source.to_str().unwrap().to_owned();
            let source_path = slot.source_path.to_str().unwrap().to_owned();
            let old_path = format!("/dev/shm{}", frame.name());
            let mut reader = std::fs::File::open(&old_path).unwrap();
            frame.renew().unwrap();
            assert!(!frame.reuse_after_retry);
            let current = format!("/dev/shm{}", frame.name());
            frame.handed_over();
            assert!(!std::path::Path::new(&source_path).exists());
            assert!(
                !pool()
                    .slots
                    .iter()
                    .any(|slot| slot.source.to_str().unwrap() == source)
            );
            use std::io::Read as _;
            let mut read = Vec::new();
            reader.read_to_end(&mut read).unwrap();
            assert_eq!(
                read, pixels,
                "an already-open reader keeps immutable pixels"
            );
            assert_eq!(std::fs::read(&current).unwrap(), pixels);
            std::fs::remove_file(current).unwrap();
        }

        #[test]
        #[cfg(target_os = "linux")]
        fn allocated_upload_can_renew_after_the_host_unlinks_its_name() {
            let pixels = [1, 2, 3, 255];
            let mut frame = SharedFrame::allocated(Medium::SharedMemory, &pixels).unwrap();
            let first = frame.name().to_owned();
            std::fs::remove_file(format!("/dev/shm{first}")).unwrap();
            frame.renew().unwrap();
            assert_ne!(frame.name(), first);
            assert_eq!(
                std::fs::read(format!("/dev/shm{}", frame.name())).unwrap(),
                pixels
            );
        }

        #[test]
        #[cfg(target_os = "linux")]
        fn idle_pool_slots_expire_after_the_host_finishes_reading() {
            let mut idle = create_slot(4096, Medium::SharedMemory).unwrap();
            let mut pending = create_slot(4096, Medium::SharedMemory).unwrap();
            let mut recent = create_slot(4096, Medium::SharedMemory).unwrap();
            let gone = CString::new(format!("/dev/shm{}", shm_name())).unwrap();
            idle.handed = Some((gone.clone(), Instant::now() - Duration::from_secs(2)));
            recent.handed = Some((gone, Instant::now()));
            let outstanding = CString::new(format!("/dev/shm{}", shm_name())).unwrap();
            assert_eq!(
                unsafe { libc::link(pending.source_path.as_ptr(), outstanding.as_ptr()) },
                0
            );
            pending.handed = Some((outstanding.clone(), Instant::now() - Duration::from_secs(2)));
            let mut pool = Pool {
                slots: vec![idle, pending, recent],
                claimed: 0,
                claimed_bytes: 0,
            };
            pool.trim_idle();
            assert_eq!(
                pool.resident(),
                8192,
                "recent and unread buffers remain resident"
            );
            std::fs::remove_file(outstanding.to_str().unwrap()).unwrap();
            for slot in &mut pool.slots {
                slot.handed.as_mut().unwrap().1 = Instant::now() - Duration::from_secs(2);
            }
            pool.trim_idle();
            assert_eq!(
                pool.resident(),
                0,
                "old buffers are unmapped once safe to release"
            );
        }

        /// These three tests share the process-wide pool, so they take turns.
        #[cfg(target_os = "linux")]
        static TEST_POOL: Mutex<()> = Mutex::new(());

        /// The whole scheme rests on this predicate: reuse is allowed exactly when the host has
        /// removed the link it was handed, and never merely because time has passed or because the
        /// name looks old. Tested on a slot of its own so no other test's frames can influence it.
        #[test]
        #[cfg(target_os = "linux")]
        fn a_slot_is_reusable_only_once_the_host_removed_the_link_it_was_given() {
            let _lock = TEST_POOL.lock().unwrap_or_else(|e| e.into_inner());
            let mut slot = create_slot(4096, Medium::SharedMemory).expect("create slot");
            assert!(slot.reusable(), "a slot that handed nothing out is free");

            let link = CString::new(format!("/dev/shm/tui-lipan-test-{}", std::process::id()))
                .expect("link name");
            assert_eq!(
                unsafe { libc::link(slot.source_path.as_ptr(), link.as_ptr()) },
                0,
                "the pool needs hard links inside the namespace"
            );
            slot.handed = Some((link.clone(), Instant::now() - GRACE * 2));
            assert!(
                !slot.reusable(),
                "a link the host has not removed means it may still read those pixels"
            );

            std::fs::remove_file(link.to_str().expect("utf-8 link")).expect("remove link");
            assert!(
                slot.reusable(),
                "the host removing the link is what frees the slot"
            );

            slot.handed = Some((link, Instant::now()));
            assert!(
                !slot.reusable(),
                "a link that vanished this instant is still inside the grace period"
            );
            slot.handed = None;
            slot.destroy();
        }

        /// Pixels the host has been told about must not move under it. A frame that was handed over
        /// and whose link is still there is one the host may not have read yet, so no later frame
        /// may be written into the same object - it either finds another slot or allocates.
        #[test]
        #[cfg(target_os = "linux")]
        fn a_frame_the_host_still_holds_is_never_overwritten_by_a_later_one() {
            let _lock = TEST_POOL.lock().unwrap_or_else(|e| e.into_inner());
            let pixels = vec![7u8; 48 * 1024];
            let mut held = SharedFrame::write(&pixels).expect("first frame");
            let Some(source) = held.slot.as_ref().map(|slot| slot.source.clone()) else {
                return; // pool full of other tests' slots; nothing to assert about reuse
            };
            held.handed_over();

            let mut later = Vec::new();
            for _ in 0..SLOTS + 2 {
                let frame = SharedFrame::write(&pixels).expect("later frame");
                assert_ne!(
                    frame.slot.as_ref().map(|slot| slot.source.clone()),
                    Some(source.clone()),
                    "an outstanding frame's object was handed to a later frame"
                );
                later.push(frame);
            }

            let handed = format!("/dev/shm{}", held.name());
            assert!(
                std::path::Path::new(&handed).exists(),
                "the link the host was given must survive until the host removes it"
            );
            std::fs::remove_file(&handed).ok();
        }

        /// An encode can finish long before its frame is painted, so a grace measured from the
        /// write can be spent before the host has heard the name at all - which is the one moment
        /// it exists to cover.
        #[test]
        #[cfg(target_os = "linux")]
        fn the_grace_runs_from_the_handover_not_from_the_write() {
            let _lock = TEST_POOL.lock().unwrap_or_else(|e| e.into_inner());
            let pixels = vec![5u8; 20 * 1024];
            let mut frame = SharedFrame::write(&pixels).expect("frame");
            let Some(source) = frame.slot.as_ref().map(|slot| slot.source.clone()) else {
                return;
            };
            // Encoded, and not painted for longer than the grace.
            std::thread::sleep(GRACE + Duration::from_millis(5));
            let handed = format!("/dev/shm{}", frame.name());
            frame.handed_over();
            std::fs::remove_file(&handed).expect("host unlinks");

            let next = SharedFrame::write(&pixels).expect("next frame");
            assert_ne!(
                next.slot.as_ref().map(|slot| slot.source.clone()),
                Some(source),
                "the grace has to start when the host was told the name"
            );
        }

        /// The matching claim: once the host has unlinked and the grace has passed, the next frame
        /// of the same size writes into that object rather than allocating another.
        #[test]
        #[cfg(target_os = "linux")]
        fn a_slot_is_written_again_once_the_host_has_finished() {
            let _lock = TEST_POOL.lock().unwrap_or_else(|e| e.into_inner());
            let pixels = vec![3u8; 33 * 1024];
            let mut first = SharedFrame::write(&pixels).expect("first frame");
            let Some(source) = first.slot.as_ref().map(|slot| slot.source.clone()) else {
                return;
            };
            let handed = format!("/dev/shm{}", first.name());
            first.handed_over();
            std::fs::remove_file(&handed).expect("host unlinks");
            std::thread::sleep(GRACE + Duration::from_millis(5));

            let second = SharedFrame::write(&pixels).expect("second frame");
            assert_eq!(
                second.slot.as_ref().map(|slot| slot.source.clone()),
                Some(source),
                "a finished slot must be the one the next same-size frame writes into"
            );
        }
    }
}

#[cfg(not(all(unix, not(target_arch = "wasm32"))))]
mod imp {
    /// Stub for hosts without a POSIX shared-memory namespace, where the probe never finds support
    /// and no frame is ever written.
    pub(crate) struct SharedFrame;

    impl SharedFrame {
        pub(crate) fn write_in(_medium: super::Medium, _pixels: &[u8]) -> Option<Self> {
            None
        }

        pub(crate) fn name(&self) -> &str {
            ""
        }

        pub(crate) fn medium(&self) -> super::Medium {
            super::Medium::SharedMemory
        }

        pub(crate) fn renew(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("shared memory unavailable"))
        }

        pub(crate) fn abandon_attempt(&mut self) {}

        pub(crate) fn handed_over(&mut self) {}
    }
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
pub(crate) use imp::trim_idle_pool;

#[cfg(not(all(unix, not(target_arch = "wasm32"))))]
pub(crate) fn trim_idle_pool() {}

pub(crate) use imp::SharedFrame;

/// Environment override for the shared-memory question, for a host judged wrongly.
///
/// `1`/`true`/`on`/`yes` asks a host that would not have been asked - a terminal that implements
/// the protocol under a `TERM` nothing here recognizes. The opposites never ask, which is the
/// escape hatch for a host that mangles the question but is not reached over a network.
#[cfg(feature = "terminal-images")]
const ASK_ENV: &str = "TUI_LIPAN_GRAPHICS_SHM";

/// Whether it is worth asking the host whether it reads shared memory, judged from the environment
/// before anything at all is written to the terminal.
///
/// The question is a graphics-protocol query, and a query is an `APC` string. ECMA-48 has a terminal
/// consume an `APC` it does not implement and most do, but the ones that do not print the whole
/// thing as text - and this question is asked before the alternate screen is entered, so on those
/// hosts the base64 lands in the user's scrollback and stays there. That is a poor trade for a
/// question whose answer is already known in two ordinary cases:
///
/// - A terminal on the other end of a network connection cannot resolve a name in *this* machine's
///   shared-memory namespace, whatever protocol it speaks. This was always so; the difference is
///   that it used to be learned by asking.
/// - A terminal that does not implement the graphics protocol has no `t=s` medium to support.
///
/// So the question is put only to a local host recognizable as one of the terminals that implement
/// the protocol. A host that is capable but unrecognized loses the shared-memory path and keeps the
/// inline one - the same fallback every unsupported host already takes - and [`ASK_ENV`] settles it
/// either way for a host judged wrongly.
#[cfg(feature = "terminal-images")]
pub(crate) fn worth_asking_about_shared_memory() -> bool {
    worth_asking(|name| std::env::var(name).ok())
}

/// The decision behind [`worth_asking_about_shared_memory`], taken over a lookup rather than the
/// real environment so it can be tested without a process-wide `set_var`.
#[cfg(feature = "terminal-images")]
fn worth_asking(var: impl Fn(&str) -> Option<String>) -> bool {
    let set = |name: &str| var(name).is_some_and(|value| !value.trim().is_empty());

    if let Some(forced) = var(ASK_ENV).as_deref().and_then(parse_override) {
        return forced;
    }
    if ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        .any(|name| set(name))
    {
        return false;
    }
    if [
        "KITTY_WINDOW_ID",
        "GHOSTTY_RESOURCES_DIR",
        "GHOSTTY_BIN_DIR",
        "WEZTERM_PANE",
        "KONSOLE_VERSION",
    ]
    .iter()
    .any(|name| set(name))
    {
        return true;
    }
    if matches!(
        var("TERM_PROGRAM")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "ghostty" | "wezterm"
    ) {
        return true;
    }
    let term = var("TERM").unwrap_or_default().to_ascii_lowercase();
    ["kitty", "ghostty", "wezterm"]
        .iter()
        .any(|name| term.contains(name))
}

/// A boolean spelled the way an environment variable usually spells one. Anything else is not an
/// answer, and leaves the judgement to make itself.
#[cfg(feature = "terminal-images")]
fn parse_override(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" => Some(false),
        _ => None,
    }
}

/// Ask whether the host can read a transmission through `medium`, and the object to ask about.
///
/// The question cannot be asked in the abstract: the protocol's query action reports on a real
/// transmission, so a terminal handed a name it cannot resolve answers that the *name* was bad rather
/// than that the medium is unsupported. Nothing here hands the object over - a terminal that declines
/// does not read it, and one that reads it unlinks it, so the caller's drop is right either way.
///
/// This only builds the question. Whether it is worth putting to this host is
/// [`worth_asking_about_shared_memory`], which the caller decides first.
#[cfg(feature = "terminal-images")]
pub(crate) fn kitty_frame_probe(id: u32, medium: Medium) -> Option<(SharedFrame, String)> {
    use base64::Engine as _;

    let frame = SharedFrame::write_in(medium, &[0, 0, 0])?;
    let mut query = format!("\x1b_Ga=q,i={id},f=24,s=1,v=1,t={};", medium.key());
    base64::engine::general_purpose::STANDARD.encode_string(frame.name(), &mut query);
    query.push_str("\x1b\\");
    Some((frame, query))
}

#[cfg(all(test, feature = "terminal-images"))]
mod worth_asking_tests {
    use super::worth_asking;

    /// A stand-in environment, so the decision is tested without a process-wide `set_var` that
    /// every other test in the binary would race against.
    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + use<'a> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        }
    }

    /// The case this gate exists for. `TERM` survives `ssh`, so a session opened from a terminal
    /// that does implement the protocol still looks capable from in here - and is still the wrong
    /// thing to hand a name in this machine's memory, because it is not this machine's terminal.
    #[test]
    fn a_host_reached_over_a_network_is_not_asked_however_capable_it_looks() {
        assert!(!worth_asking(env(&[
            ("TERM", "xterm-kitty"),
            ("SSH_CONNECTION", "10.0.0.2 51000 10.0.0.1 22"),
        ])));
        assert!(!worth_asking(env(&[
            ("TERM", "xterm-256color"),
            ("SSH_TTY", "/dev/pts/3"),
        ])));
    }

    /// An empty variable is not a session. Login shells and CI images both leave these defined and
    /// blank, and reading that as "remote" would switch the medium off on the machine it works on.
    #[test]
    fn an_empty_ssh_variable_does_not_make_a_host_remote() {
        assert!(worth_asking(env(&[
            ("TERM", "xterm-kitty"),
            ("SSH_CLIENT", ""),
        ])));
    }

    /// The terminals that implement the protocol, by the marker each one actually sets. Konsole is
    /// the reason a bare `TERM` match is not enough: it reports `xterm-256color` and says who it is
    /// only in its own variable.
    #[test]
    fn a_local_terminal_that_implements_the_protocol_is_asked() {
        for pairs in [
            vec![("TERM", "xterm-kitty")],
            vec![("TERM", "xterm-ghostty")],
            vec![("TERM", "xterm-256color"), ("KITTY_WINDOW_ID", "1")],
            vec![("TERM", "xterm-256color"), ("KONSOLE_VERSION", "230804")],
            vec![("TERM", "xterm-256color"), ("TERM_PROGRAM", "WezTerm")],
        ] {
            assert!(worth_asking(env(&pairs)), "{pairs:?} implements it");
        }
    }

    /// Everything else keeps the inline path rather than being asked a question it may echo. A
    /// multiplexer is in this set on its own merits: the reader of the sequence is `tmux`, not the
    /// terminal, so the answer would not have been the terminal's anyway.
    #[test]
    fn an_unrecognized_local_host_is_not_asked() {
        for pairs in [
            vec![("TERM", "xterm-256color")],
            vec![("TERM", "tmux-256color")],
            vec![("TERM", "dumb")],
            vec![],
        ] {
            assert!(!worth_asking(env(&pairs)), "{pairs:?} is not recognized");
        }
    }

    /// The escape hatch in both directions, for the host judged wrongly: a terminal that implements
    /// the protocol under an unknown `TERM`, and a local one that mangles the question. A value
    /// that is not a boolean is not an instruction, and leaves the judgement to make itself.
    #[test]
    fn the_override_settles_it_either_way() {
        assert!(worth_asking(env(&[
            ("TERM", "xterm-256color"),
            ("SSH_TTY", "/dev/pts/3"),
            ("TUI_LIPAN_GRAPHICS_SHM", "1"),
        ])));
        assert!(!worth_asking(env(&[
            ("TERM", "xterm-kitty"),
            ("TUI_LIPAN_GRAPHICS_SHM", "off"),
        ])));
        assert!(worth_asking(env(&[
            ("TERM", "xterm-kitty"),
            ("TUI_LIPAN_GRAPHICS_SHM", "maybe"),
        ])));
    }
}
