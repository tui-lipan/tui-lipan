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
//! Not every terminal implements it, so [`host_reads_shared_memory`] records what the startup probe
//! found and the caller keeps the inline path for everything else.
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

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Whether the host answered the startup probe saying it can read a shared-memory transmission.
static HOST_READS_SHARED_MEMORY: AtomicBool = AtomicBool::new(false);

/// Names are per-process and monotonic, so a frame never collides with one still in flight, and a
/// previous run's leftovers cannot be mistaken for this run's.
static NEXT_NAME: AtomicU64 = AtomicU64::new(0);

/// Record what the startup probe found. See [`host_reads_shared_memory`].
pub(crate) fn note_host_support(supported: bool) {
    HOST_READS_SHARED_MEMORY.store(supported, Ordering::Release);
}

/// Whether to hand frames to the host through shared memory rather than inline.
pub(crate) fn host_reads_shared_memory() -> bool {
    HOST_READS_SHARED_MEMORY.load(Ordering::Acquire)
}

/// A name in the POSIX shared-memory namespace: one leading slash and no others.
fn next_name() -> String {
    format!(
        "/tui-lipan-{}-{}",
        std::process::id(),
        NEXT_NAME.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
mod imp {
    #![allow(unsafe_code)]

    use std::ffi::CString;
    use std::io::Write as _;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// How many objects to keep alive for reuse. Four is a frame's worth of slack at 60 fps against
    /// a host reading them one at a time, and four frames of resident memory.
    const SLOTS: usize = 4;

    /// A slot is not reused within this long of being handed over, however quickly the name
    /// disappears. The protocol has the host read the object and then unlink it, so the name going
    /// away means it is finished - but a host that unlinked first and read second would be within
    /// its rights to be slow about the read, and nothing here can see that happen.
    const GRACE: Duration = Duration::from_millis(50);

    /// Total resident bytes the pool may hold. A pane large enough to blow through this keeps the
    /// allocate-per-frame path rather than pinning a large multiple of itself in RAM.
    const POOL_BUDGET: usize = 64 * 1024 * 1024;

    static POOL: Mutex<Pool> = Mutex::new(Pool {
        slots: Vec::new(),
        claimed: 0,
        swept: false,
    });

    struct Pool {
        slots: Vec<Slot>,
        /// Slots currently out with a frame. Counted so the pool cannot grow past [`SLOTS`] by
        /// creating a new slot for every frame while the others are away.
        claimed: usize,
        swept: bool,
    }

    /// One reusable object: created and sized once, written again every frame.
    struct Slot {
        /// The pool object's own name, which is never handed to the host. Frames are handed hard
        /// links to it instead, so the host's unlink takes a link and leaves the object.
        source: CString,
        /// The same object as a filesystem path, which is what `link` needs.
        source_path: CString,
        map: *mut libc::c_void,
        len: usize,
        /// The link most recently handed to the host, and when. `None` means nothing is outstanding.
        handed: Option<(CString, Instant)>,
    }

    // SAFETY: the mapping is `MAP_SHARED` memory owned by this slot alone; the pool hands a slot to
    // one thread at a time and takes it back before another can claim it.
    unsafe impl Send for Slot {}

    impl Slot {
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
            unsafe {
                libc::munmap(self.map, self.len);
                libc::shm_unlink(self.source.as_ptr());
            }
            if let Some((link, _)) = &self.handed {
                unsafe { libc::unlink(link.as_ptr()) };
            }
        }
    }

    /// Where a POSIX shared-memory name lands in the filesystem, which is the only way to hard-link
    /// one object under a second name. Linux puts the namespace on a tmpfs mount; a platform that
    /// does not is simply left with the allocate-per-frame path.
    fn shm_path(name: &str) -> Option<CString> {
        if !cfg!(target_os = "linux") {
            return None;
        }
        CString::new(format!("/dev/shm{name}")).ok()
    }

    /// Remove pool objects left behind by a run that was killed before it could clean up. Their
    /// names carry the process id that made them, so a dead one is unambiguous.
    fn sweep_dead_runs() {
        let Ok(entries) = std::fs::read_dir("/dev/shm") else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(rest) = name.strip_prefix("tui-lipan-") else {
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
            if !alive {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    fn create_slot(len: usize) -> Option<Slot> {
        use std::os::fd::FromRawFd as _;

        let name = format!(
            "/tui-lipan-pool-{}-{}",
            std::process::id(),
            super::NEXT_NAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let source_path = shm_path(&name)?;
        let source = CString::new(name).ok()?;
        let descriptor = unsafe {
            libc::shm_open(
                source.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                0o600,
            )
        };
        if descriptor < 0 {
            return None;
        }
        // SAFETY: `shm_open` just produced this descriptor and nothing else holds a copy, so the
        // `File` owns it and closes it on every path out of here. The mapping outlives the
        // descriptor, which is what `mmap` guarantees.
        let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
        let Some(length) = libc::off_t::try_from(len).ok() else {
            unsafe { libc::shm_unlink(source.as_ptr()) };
            return None;
        };
        if unsafe { libc::ftruncate(descriptor, length) } < 0 {
            unsafe { libc::shm_unlink(source.as_ptr()) };
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
            unsafe { libc::shm_unlink(source.as_ptr()) };
            return None;
        }
        // Touch every page once here rather than during the first frame, so the frame that happens
        // to create a slot is not the slow one.
        unsafe { std::ptr::write_bytes(map as *mut u8, 0, len) };
        Some(Slot {
            source,
            source_path,
            map,
            len,
            handed: None,
        })
    }

    /// Take a slot able to hold `len` bytes, or `None` to say the caller should allocate.
    fn claim(len: usize) -> Option<Slot> {
        let mut pool = POOL.lock().ok()?;
        if !pool.swept {
            pool.swept = true;
            sweep_dead_runs();
        }
        if let Some(index) = pool
            .slots
            .iter()
            .position(|slot| slot.len == len && slot.reusable())
        {
            pool.claimed += 1;
            return Some(pool.slots.swap_remove(index));
        }
        let resident: usize = pool.slots.iter().map(|slot| slot.len).sum();
        if pool.slots.len() + pool.claimed >= SLOTS || resident + len > POOL_BUDGET {
            // A pane that changed size leaves slots no frame of this size can use. One of those is
            // what makes room; if there is none, every slot is either the wrong size and still out
            // with a frame, or the right size and not finished with, so this frame allocates.
            let index = pool
                .slots
                .iter()
                .position(|slot| slot.len != len && slot.reusable())?;
            pool.slots.swap_remove(index).destroy();
            let resident: usize = pool.slots.iter().map(|slot| slot.len).sum();
            if resident + len > POOL_BUDGET {
                return None;
            }
        }
        let slot = create_slot(len)?;
        pool.claimed += 1;
        Some(slot)
    }

    fn release(slot: Slot) {
        if let Ok(mut pool) = POOL.lock() {
            pool.claimed = pool.claimed.saturating_sub(1);
            pool.slots.push(slot);
        } else {
            slot.destroy();
        }
    }

    /// Pixels in a shared-memory object, waiting for the host to read them.
    ///
    /// Owns the name until the host is told about it. A frame that is encoded and then never
    /// drawn - a pane closed mid-encode, a placement replaced by a newer one - would otherwise leave
    /// its object behind, one per frame, until the machine reboots.
    pub(crate) struct SharedFrame {
        name: CString,
        /// The pool slot backing this frame, held until the transmission is written so nothing can
        /// overwrite the pixels the host has not been told about yet. `None` for a frame that
        /// allocated its own object.
        slot: Option<Slot>,
        /// Cleared once the host has been told the name, since reading is what unlinks the name
        /// and the host is the reader.
        owned: bool,
    }

    impl SharedFrame {
        /// Put `pixels` where the host can read them, or `None` if the shared-memory namespace
        /// refused.
        pub(crate) fn write(pixels: &[u8]) -> Option<Self> {
            match Self::pooled(pixels) {
                Some(frame) => Some(frame),
                None => Self::allocated(pixels),
            }
        }

        /// Copy into a reused object and hand the host a fresh link to it.
        fn pooled(pixels: &[u8]) -> Option<Self> {
            let slot = claim(pixels.len())?;
            let named = CString::new(super::next_name())
                .ok()
                .and_then(|name| Some((shm_path(name.to_str().ok()?)?, name)));
            let Some((link, name)) = named else {
                release(slot);
                return None;
            };
            debug_assert_eq!(slot.len, pixels.len());
            // SAFETY: the mapping is exactly `slot.len` bytes, which is the length the slot was
            // claimed for, and no other thread holds this slot.
            unsafe {
                std::ptr::copy_nonoverlapping(pixels.as_ptr(), slot.map as *mut u8, pixels.len())
            };
            if unsafe { libc::link(slot.source_path.as_ptr(), link.as_ptr()) } < 0 {
                // Hard links inside the namespace are what the whole scheme rests on, so a failure
                // here means this platform cannot pool at all rather than that this frame is
                // unlucky. Drop the slot and let the caller allocate.
                slot.destroy();
                if let Ok(mut pool) = POOL.lock() {
                    pool.claimed = pool.claimed.saturating_sub(1);
                }
                return None;
            }
            Some(Self {
                name,
                slot: Some(Slot {
                    handed: Some((link, Instant::now())),
                    ..slot
                }),
                owned: true,
            })
        }

        /// Put `pixels` in a fresh object, which is what happens when every slot is still out.
        fn allocated(pixels: &[u8]) -> Option<Self> {
            use std::os::fd::FromRawFd as _;

            let length = libc::off_t::try_from(pixels.len()).ok()?;
            let name = CString::new(super::next_name()).ok()?;
            // `O_EXCL` so a name that somehow already exists is an error rather than a frame
            // silently overwriting something; 0600 so only this user can read the pixels.
            let descriptor = unsafe {
                libc::shm_open(
                    name.as_ptr(),
                    libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                    0o600,
                )
            };
            if descriptor < 0 {
                return None;
            }
            // SAFETY: `shm_open` just produced this descriptor and nothing else holds a copy, so the
            // `File` is its sole owner and closes it on every path out of here.
            let mut file = unsafe { std::fs::File::from_raw_fd(descriptor) };
            let frame = Self {
                name,
                slot: None,
                owned: true,
            };
            if unsafe { libc::ftruncate(descriptor, length) } < 0 {
                return None;
            }
            file.write_all(pixels).ok()?;
            Some(frame)
        }

        /// The object's name, for the `t=s` payload.
        pub(crate) fn name(&self) -> &str {
            self.name.to_str().unwrap_or_default()
        }

        /// The host has been told the name and will unlink it once it has read the pixels.
        ///
        /// This is where a pooled slot goes back: the pixels are the host's to read now, and the
        /// slot's own record of the link it handed out is what keeps the next frame from
        /// overwriting them before it has.
        pub(crate) fn handed_over(&mut self) {
            self.owned = false;
            if let Some(slot) = self.slot.take() {
                release(slot);
            }
        }
    }

    impl Drop for SharedFrame {
        fn drop(&mut self) {
            if self.owned {
                unsafe { libc::shm_unlink(self.name.as_ptr()) };
            }
            if let Some(mut slot) = self.slot.take() {
                // Never handed over, so nothing outside this process ever knew the link existed and
                // the slot is free the moment it goes back.
                slot.handed = None;
                release(slot);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

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
            let mut slot = create_slot(4096).expect("create slot");
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
        pub(crate) fn write(_pixels: &[u8]) -> Option<Self> {
            None
        }

        pub(crate) fn name(&self) -> &str {
            ""
        }

        pub(crate) fn handed_over(&mut self) {}
    }
}

pub(crate) use imp::SharedFrame;

/// Ask whether the host can read a transmission out of shared memory, and the object to ask about.
///
/// The question cannot be asked in the abstract: the protocol's query action reports on a real
/// transmission, so a terminal handed a name it cannot resolve answers that the *name* was bad rather
/// than that the medium is unsupported. Nothing here hands the object over - a terminal that declines
/// does not read it, and one that reads it unlinks it, so the caller's drop is right either way.
#[cfg(feature = "terminal-images")]
pub(crate) fn kitty_shared_memory_probe(id: u32) -> Option<(SharedFrame, String)> {
    use base64::Engine as _;

    let frame = SharedFrame::write(&[0, 0, 0])?;
    let mut query = format!("\x1b_Ga=q,i={id},f=24,s=1,v=1,t=s;");
    base64::engine::general_purpose::STANDARD.encode_string(frame.name(), &mut query);
    query.push_str("\x1b\\");
    Some((frame, query))
}
