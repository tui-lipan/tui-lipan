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

    /// Pixels in a shared-memory object, waiting for the host to read them.
    ///
    /// Owns the object until the host is told about it. A frame that is encoded and then never
    /// drawn - a pane closed mid-encode, a placement replaced by a newer one - would otherwise leave
    /// its object behind, one per frame, until the machine reboots.
    pub(crate) struct SharedFrame {
        name: CString,
        /// Cleared once the host has been told the name, since reading is what unlinks the object
        /// and the host is the reader.
        owned: bool,
    }

    impl SharedFrame {
        /// Put `pixels` in a fresh object, or `None` if the shared-memory namespace refused.
        pub(crate) fn write(pixels: &[u8]) -> Option<Self> {
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
            let frame = Self { name, owned: true };
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

        /// The host has been told the name and will unlink the object once it has read it.
        pub(crate) fn handed_over(&mut self) {
            self.owned = false;
        }
    }

    impl Drop for SharedFrame {
        fn drop(&mut self) {
            if self.owned {
                unsafe { libc::shm_unlink(self.name.as_ptr()) };
            }
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
