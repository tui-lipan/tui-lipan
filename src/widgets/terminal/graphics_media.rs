//! Out-of-band graphics payloads: the pixels a child leaves in a file or a shared-memory object
//! and only *names* in the escape sequence (`t=f`, `t=t`, `t=s`).
//!
//! This is what keeps a picture out of the byte stream entirely. A program redrawing a full window
//! every frame would otherwise have to compress and base64 several megabytes per frame just to get
//! them through the PTY, and the terminal would have to undo both. Naming the pixels instead costs
//! a hundred bytes on the wire and one read here.
//!
//! Two of the three media are consumed by reading: `t=t` deletes the file and `t=s` unlinks the
//! object, so exactly one reader may claim them. `t=f` is left in place and may be read again,
//! which is what lets several readers - or a multiplexer's several attached clients - share one
//! stream. [`GraphicsMedium::consumes_source`] is what a caller checks before advertising support.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Where the pixels come from (`t=`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum GraphicsMedium {
    /// `t=d` - inline in the escape sequence.
    #[default]
    Direct,
    /// `t=f` - a path the sender leaves in place, so it can be read more than once.
    File,
    /// `t=t` - a path this terminal owns: read once, then deleted.
    TempFile,
    /// `t=s` - a shared-memory object this terminal owns: read once, then unlinked.
    SharedMemory,
}

impl GraphicsMedium {
    pub(super) fn from_key(value: u8) -> Self {
        match value {
            b'f' => Self::File,
            b't' => Self::TempFile,
            b's' => Self::SharedMemory,
            _ => Self::Direct,
        }
    }

    pub(super) fn is_out_of_band(self) -> bool {
        !matches!(self, Self::Direct)
    }

    /// Whether reading destroys the source, so only one reader can ever succeed.
    pub(super) fn consumes_source(self) -> bool {
        matches!(self, Self::TempFile | Self::SharedMemory)
    }

    /// Whether a source of this kind may be read under `policy`.
    pub(super) fn allowed_by(self, policy: GraphicsMediaPolicy) -> bool {
        match self {
            Self::Direct => true,
            _ if self.consumes_source() => policy.consuming,
            _ => policy.file,
        }
    }
}

/// Which media a screen will accept, so an embedder that cannot guarantee a single reader can
/// decline the consuming ones rather than racing another reader for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphicsMediaPolicy {
    /// Accept `t=f`, reading the named file and leaving it alone.
    pub file: bool,
    /// Accept `t=t` and `t=s`, which are read once and then removed.
    pub consuming: bool,
}

impl Default for GraphicsMediaPolicy {
    fn default() -> Self {
        Self {
            file: true,
            consuming: true,
        }
    }
}

impl GraphicsMediaPolicy {
    /// Accept nothing out of band; every picture must arrive inside the escape sequence.
    pub const NONE: Self = Self {
        file: false,
        consuming: false,
    };

    /// Accept only the re-readable medium. This is the setting for anything that fans one terminal
    /// stream out to several readers.
    pub const SHARED: Self = Self {
        file: true,
        consuming: false,
    };

    pub(super) fn allows(self, medium: GraphicsMedium) -> bool {
        medium.allowed_by(self)
    }
}

/// A `t=t` file is deleted once read, so a sender must not be able to aim that at an arbitrary
/// path. This is the reference implementation's rule: the file lives somewhere temporary, or says
/// in its own name what it is for.
const TEMP_FILE_MARKER: &str = "tty-graphics-protocol";

/// Read the pixels a command named, consuming the source when the medium says to.
///
/// `offset` and `limit` are the protocol's `O=` and `S=`; a zero `limit` means "to the end".
/// `budget` caps what any single transmission may claim, since the sender chooses the size.
pub(super) fn load(
    medium: GraphicsMedium,
    name: &[u8],
    offset: u64,
    limit: usize,
    budget: usize,
) -> Result<Vec<u8>, &'static str> {
    match medium {
        GraphicsMedium::Direct => Err("EINVAL:not an out-of-band medium"),
        GraphicsMedium::File => read_file(&decode_path(name)?, offset, limit, budget, false),
        GraphicsMedium::TempFile => {
            let path = decode_path(name)?;
            if !is_claimable_temp_file(&path) {
                return Err("EINVAL:temporary file not named correctly");
            }
            read_file(&path, offset, limit, budget, true)
        }
        GraphicsMedium::SharedMemory => shared_memory::read(name, offset, limit, budget),
    }
}

#[cfg(unix)]
pub(super) fn decode_path(name: &[u8]) -> Result<PathBuf, &'static str> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    if name.is_empty() {
        return Err("EINVAL:missing path");
    }
    Ok(PathBuf::from(OsStr::from_bytes(name)))
}

#[cfg(not(unix))]
pub(super) fn decode_path(name: &[u8]) -> Result<PathBuf, &'static str> {
    if name.is_empty() {
        return Err("EINVAL:missing path");
    }
    std::str::from_utf8(name)
        .map(PathBuf::from)
        .map_err(|_| "EINVAL:path is not valid UTF-8")
}

/// Whether deleting this path after reading it is something the sender is entitled to ask for.
fn is_claimable_temp_file(path: &Path) -> bool {
    if path.to_string_lossy().contains(TEMP_FILE_MARKER) {
        return true;
    }
    let temp = std::env::temp_dir();
    path.starts_with(&temp) || path.starts_with("/tmp") || path.starts_with("/dev/shm")
}

fn read_file(
    path: &Path,
    offset: u64,
    limit: usize,
    budget: usize,
    consume: bool,
) -> Result<Vec<u8>, &'static str> {
    let result = read_file_contents(path, offset, limit, budget);
    // The sender handed the file over, so it is this terminal's to clean up whether or not the
    // contents turned out to be usable; leaving it behind on error would leak one file per frame.
    if consume {
        let _ = fs::remove_file(path);
    }
    result
}

fn read_file_contents(
    path: &Path,
    offset: u64,
    limit: usize,
    budget: usize,
) -> Result<Vec<u8>, &'static str> {
    let file = fs::File::open(path).map_err(|_| "EBADF:cannot open file")?;
    read_source(file, offset, limit, budget)
}

/// Take the requested window out of an already-open source.
///
/// Shared memory reaches this through a descriptor rather than a path everywhere but Apple
/// platforms, so the size, offset and budget arithmetic lives here once.
fn read_source(
    mut file: fs::File,
    offset: u64,
    limit: usize,
    budget: usize,
) -> Result<Vec<u8>, &'static str> {
    let length = file
        .metadata()
        .map_err(|_| "EBADF:cannot stat source")?
        .len()
        .saturating_sub(offset);
    let wanted = usable_length(length, limit, budget)?;
    if offset > 0 {
        use std::io::Seek as _;
        file.seek(std::io::SeekFrom::Start(offset))
            .map_err(|_| "EINVAL:bad offset")?;
    }
    let mut data = Vec::new();
    data.try_reserve_exact(wanted)
        .map_err(|_| "ENOMEM:cannot hold payload")?;
    file.take(wanted as u64)
        .read_to_end(&mut data)
        .map_err(|_| "EIO:cannot read source")?;
    Ok(data)
}

/// How much of a named source to take, given what it holds, what the sender asked for, and what
/// this terminal is willing to hold.
fn usable_length(available: u64, limit: usize, budget: usize) -> Result<usize, &'static str> {
    let available = usize::try_from(available).unwrap_or(usize::MAX);
    let wanted = if limit == 0 {
        available
    } else {
        limit.min(available)
    };
    if wanted == 0 {
        return Err("EINVAL:empty payload");
    }
    if wanted > budget {
        return Err("EFBIG:payload too large");
    }
    Ok(wanted)
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
mod shared_memory {
    #![allow(unsafe_code)]

    /// Read and unlink a POSIX shared-memory object.
    ///
    /// `shm_open` hands back an ordinary descriptor, so outside Apple platforms the object is
    /// drained through the same size and budget arithmetic as a file.
    pub(super) fn read(
        name: &[u8],
        offset: u64,
        limit: usize,
        budget: usize,
    ) -> Result<Vec<u8>, &'static str> {
        use std::ffi::CString;
        use std::os::fd::FromRawFd as _;

        // The name goes to `shm_open` unchanged, which owns the namespace rules. Kitty accepts
        // names without a leading slash because sandboxed macOS applications can only create
        // `group-id/name` objects; libc implementations that back objects with files refuse
        // names that would leave their directory.
        if name.is_empty() {
            return Err("EINVAL:bad shared memory name");
        }
        let name = CString::new(name).map_err(|_| "EINVAL:bad shared memory name")?;
        let descriptor = unsafe { libc::shm_open(name.as_ptr(), libc::O_RDONLY, 0) };
        if descriptor < 0 {
            return Err("EBADF:cannot open shared memory");
        }
        // SAFETY: `shm_open` just produced this descriptor and nothing else holds a copy, so the
        // `File` is its sole owner and closes it on every path out of here.
        let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
        let result = acquire(file, offset, limit, budget);
        // Unlink only after acquisition finishes. Kitty consumes an opened object even when its
        // contents are unusable; do the same on stat, budget, allocation or mapping failure so a
        // sender that has handed it over does not leak one object per failed frame. Failed opens
        // and invalid names never claim ownership. This is the sole unlink for a claimed object.
        // In particular, successful acquisition copies before unlink: pooled senders may reuse
        // the backing storage as soon as they observe that the name is gone.
        unsafe { libc::shm_unlink(name.as_ptr()) };
        result
    }

    /// Copy through `read`, never a mapping: Linux lets the sender shrink the object while it is
    /// being copied, and touching a mapped page past the new end raises `SIGBUS` in the terminal.
    /// A descriptor read just comes up short.
    #[cfg(not(target_vendor = "apple"))]
    fn acquire(
        file: std::fs::File,
        offset: u64,
        limit: usize,
        budget: usize,
    ) -> Result<Vec<u8>, &'static str> {
        super::read_source(file, offset, limit, budget)
    }

    /// Darwin shared-memory descriptors support `fstat` and `mmap`, but not `read` or `lseek`.
    /// Mapping is safe there because Darwin cannot resize an object once its size is set.
    #[cfg(target_vendor = "apple")]
    fn acquire(
        file: std::fs::File,
        offset: u64,
        limit: usize,
        budget: usize,
    ) -> Result<Vec<u8>, &'static str> {
        use std::os::fd::AsRawFd as _;

        // File::metadata uses fstat on this descriptor, including on Darwin.
        let length = file
            .metadata()
            .map_err(|_| "EBADF:cannot stat source")?
            .len();
        let wanted = super::usable_length(length.saturating_sub(offset), limit, budget)?;
        // mmap offsets must be page aligned; protocol offsets need not be.
        let page = u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })
            .ok()
            .filter(|page| *page > 0)
            .ok_or("EIO:cannot map shared memory")?;
        let page_start = offset - offset % page;
        let delta = (offset - page_start) as usize;
        let map_offset = libc::off_t::try_from(page_start).map_err(|_| "EINVAL:bad offset")?;
        let mapped_len = delta
            .checked_add(wanted)
            .filter(|length| *length <= isize::MAX as usize)
            .ok_or("EINVAL:bad offset")?;
        let mut data = Vec::new();
        data.try_reserve_exact(wanted)
            .map_err(|_| "ENOMEM:cannot hold payload")?;
        // SAFETY: fstat bounded this nonempty range within the object, and Darwin objects cannot
        // shrink afterwards.
        let mapping = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                mapped_len,
                libc::PROT_READ,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                map_offset,
            )
        };
        if mapping == libc::MAP_FAILED {
            return Err("EIO:cannot map shared memory");
        }
        // SAFETY: mmap succeeded, and the requested window lies within mapped_len. Capacity was
        // reserved before mapping, so copying neither reallocates nor outlives the mapping.
        let pixels = unsafe { std::slice::from_raw_parts(mapping.cast::<u8>().add(delta), wanted) };
        data.extend_from_slice(pixels);
        // SAFETY: this is the mapping returned above, with its original length.
        unsafe { libc::munmap(mapping, mapped_len) };
        Ok(data)
    }
}

#[cfg(not(all(unix, not(target_arch = "wasm32"))))]
mod shared_memory {
    pub(super) fn read(
        _name: &[u8],
        _offset: u64,
        _limit: usize,
        _budget: usize,
    ) -> Result<Vec<u8>, &'static str> {
        Err("ENOTSUPP:shared memory transmission")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str, contents: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("tui-lipan-media-{name}"));
        fs::write(&path, contents).expect("write scratch file");
        path
    }

    #[test]
    fn file_medium_reads_without_consuming_so_several_readers_can_share_it() {
        let path = scratch("shared", b"pixels");
        let name = path.to_string_lossy().into_owned().into_bytes();

        for _ in 0..3 {
            let data = load(GraphicsMedium::File, &name, 0, 0, 1024).expect("read file");
            assert_eq!(data, b"pixels");
        }
        assert!(path.exists(), "t=f must leave the file for the next reader");
        fs::remove_file(&path).ok();
    }

    #[test]
    fn temp_file_medium_deletes_what_it_read() {
        let path = scratch("tty-graphics-protocol-claim", b"pixels");
        let name = path.to_string_lossy().into_owned().into_bytes();

        let data = load(GraphicsMedium::TempFile, &name, 0, 0, 1024).expect("read temp file");
        assert_eq!(data, b"pixels");
        assert!(!path.exists(), "t=t must consume the file it was handed");
    }

    #[test]
    fn temp_file_medium_refuses_a_path_it_was_not_offered() {
        let dir = std::env::current_dir().expect("cwd");
        let path = dir.join("tui-lipan-not-temporary");
        fs::write(&path, b"pixels").expect("write");
        let name = path.to_string_lossy().into_owned().into_bytes();

        let error = load(GraphicsMedium::TempFile, &name, 0, 0, 1024).unwrap_err();
        assert!(error.contains("named correctly"));
        assert!(path.exists(), "a refused path must not be deleted");
        fs::remove_file(&path).ok();
    }

    #[test]
    fn offset_and_size_select_a_window_of_the_file() {
        let path = scratch("window", b"0123456789");
        let name = path.to_string_lossy().into_owned().into_bytes();

        assert_eq!(
            load(GraphicsMedium::File, &name, 3, 4, 1024).unwrap(),
            b"3456"
        );
        assert_eq!(
            load(GraphicsMedium::File, &name, 8, 0, 1024).unwrap(),
            b"89"
        );
        fs::remove_file(&path).ok();
    }

    #[test]
    fn a_payload_over_budget_is_refused_rather_than_allocated() {
        let path = scratch("budget", &vec![0u8; 4096]);
        let name = path.to_string_lossy().into_owned().into_bytes();

        let error = load(GraphicsMedium::File, &name, 0, 0, 1024).unwrap_err();
        assert!(error.starts_with("EFBIG"));
        fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_source_reports_rather_than_panics() {
        let name = b"/nonexistent/tui-lipan/frame.rgba".to_vec();
        assert!(load(GraphicsMedium::File, &name, 0, 0, 1024).is_err());
        assert!(
            load(
                GraphicsMedium::SharedMemory,
                b"/tui-lipan-absent",
                0,
                0,
                1024
            )
            .is_err()
        );
    }

    #[test]
    fn shared_memory_names_that_cannot_reach_shm_open_are_refused() {
        for name in [&b""[..], &b"/a\0b"[..]] {
            let error = load(GraphicsMedium::SharedMemory, name, 0, 0, 1024).unwrap_err();
            assert!(error.starts_with("EINVAL") || error.starts_with("ENOTSUPP"));
        }
    }

    /// `shm_open` decides what a name means, and none of these names an object it can open.
    #[test]
    fn shared_memory_names_outside_the_namespace_are_not_opened() {
        for name in [&b"/a/b"[..], &b"/"[..], &b"../etc/passwd"[..]] {
            assert!(load(GraphicsMedium::SharedMemory, name, 0, 0, 1024).is_err());
        }
    }

    /// The two directions have to agree on the namespace and on who unlinks: this reads back a frame
    /// written by the same code that hands frames to a host terminal, then checks the object is gone.
    #[cfg(all(unix, feature = "terminal-images", not(target_arch = "wasm32")))]
    #[test]
    fn a_frame_written_for_a_host_reads_back_through_the_shared_memory_medium() {
        use crate::backend::ratatui_backend::shared_frame::SharedFrame;

        let mut frame = SharedFrame::write(b"pixels").expect("write shared frame");
        let name = frame.name().to_owned();
        frame.handed_over();

        assert_eq!(
            load(GraphicsMedium::SharedMemory, name.as_bytes(), 0, 0, 1024).unwrap(),
            b"pixels"
        );
        assert!(
            load(GraphicsMedium::SharedMemory, name.as_bytes(), 0, 0, 1024).is_err(),
            "reading a t=s object unlinks it, so the second read finds nothing"
        );
    }

    /// A frame nobody ever draws must not leave its object behind, or a pane closed mid-encode leaks
    /// one per frame until the machine reboots.
    #[cfg(all(unix, feature = "terminal-images", not(target_arch = "wasm32")))]
    #[test]
    fn a_frame_the_host_was_never_told_about_is_unlinked_on_drop() {
        use crate::backend::ratatui_backend::shared_frame::SharedFrame;

        let frame = SharedFrame::write(b"pixels").expect("write shared frame");
        let name = frame.name().to_owned();
        drop(frame);

        assert!(load(GraphicsMedium::SharedMemory, name.as_bytes(), 0, 0, 1024).is_err());
    }

    /// Exercises the Darwin path in macOS CI too, without the terminal-images feature.
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    mod posix_shm {
        #![allow(unsafe_code)]

        use super::*;
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT: AtomicU64 = AtomicU64::new(0);

        struct Object(CString);

        impl Object {
            fn new(contents: &[u8]) -> Self {
                Self::with_prefix("/", contents)
            }

            fn with_prefix(prefix: &str, contents: &[u8]) -> Self {
                let name = CString::new(format!(
                    "{prefix}tl-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ))
                .unwrap();
                // SAFETY: unique name, valid flags, owned descriptor, and checked mapping.
                unsafe {
                    let fd = libc::shm_open(
                        name.as_ptr(),
                        libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                        0o600,
                    );
                    assert!(fd >= 0, "{}", std::io::Error::last_os_error());
                    let file = fs::File::from_raw_fd(fd);
                    assert_eq!(
                        libc::ftruncate(file.as_raw_fd(), contents.len() as libc::off_t),
                        0
                    );
                    if !contents.is_empty() {
                        let map = libc::mmap(
                            std::ptr::null_mut(),
                            contents.len(),
                            libc::PROT_WRITE,
                            libc::MAP_SHARED,
                            file.as_raw_fd(),
                            0,
                        );
                        assert_ne!(map, libc::MAP_FAILED);
                        std::ptr::copy_nonoverlapping(
                            contents.as_ptr(),
                            map.cast(),
                            contents.len(),
                        );
                        assert_eq!(libc::munmap(map, contents.len()), 0);
                    }
                }
                Self(name)
            }

            fn read(
                &self,
                offset: u64,
                size: usize,
                budget: usize,
            ) -> Result<Vec<u8>, &'static str> {
                load(
                    GraphicsMedium::SharedMemory,
                    self.0.as_bytes(),
                    offset,
                    size,
                    budget,
                )
            }

            fn assert_consumed(&self) {
                assert_eq!(
                    self.read(0, 0, 1024),
                    Err("EBADF:cannot open shared memory")
                );
            }
        }

        impl Drop for Object {
            fn drop(&mut self) {
                // Cleanup only if a failed assertion left the object unclaimed.
                unsafe {
                    let fd = libc::shm_open(self.0.as_ptr(), libc::O_RDONLY, 0);
                    if fd >= 0 {
                        drop(fs::File::from_raw_fd(fd));
                        libc::shm_unlink(self.0.as_ptr());
                    }
                }
            }
        }

        #[test]
        fn unaligned_window_and_tail_are_acquired_before_consumption() {
            for (offset, size, expected) in [(3, 4, &b"3456"[..]), (8, 2, &b"89"[..])] {
                let object = Object::new(b"0123456789");
                assert_eq!(object.read(offset, size, 4).unwrap(), expected);
                object.assert_consumed();
            }
        }

        #[test]
        fn an_offset_past_the_first_page_is_acquired() {
            let mut pixels = vec![0; 64 * 1024];
            pixels[40_000..40_004].copy_from_slice(b"tail");
            let object = Object::new(&pixels);
            assert_eq!(object.read(40_000, 4, 4).unwrap(), b"tail");
            object.assert_consumed();
        }

        /// Sandboxed macOS senders can only create `group-id/name` objects, so a name without a
        /// leading slash is passed to `shm_open` as written.
        #[test]
        fn a_name_without_a_leading_slash_is_acquired() {
            let object = Object::with_prefix("", b"pixels");
            assert_eq!(object.read(0, 6, 1024).unwrap(), b"pixels");
            object.assert_consumed();
        }

        #[test]
        fn an_unlimited_read_uses_the_stat_size() {
            // Darwin rounds shm lengths to VM pages. Use a page-sized payload rather than
            // assuming ftruncate preserves a sub-page logical length as Linux does.
            let pixels = vec![42; 64 * 1024];
            let object = Object::new(&pixels);
            assert_eq!(object.read(0, 0, pixels.len()).unwrap(), pixels);
            object.assert_consumed();
        }

        #[test]
        fn failed_acquisition_still_cleans_up_the_offered_object() {
            for (contents, offset, budget, error) in [
                (&b""[..], 0, 1024, "EINVAL:empty payload"),
                (&b"pixels"[..], 0, 2, "EFBIG:payload too large"),
                (&b"pixels"[..], u64::MAX, 1024, "EINVAL:empty payload"),
            ] {
                let object = Object::new(contents);
                assert_eq!(object.read(offset, 0, budget), Err(error));
                object.assert_consumed();
            }
        }
    }

    #[test]
    fn policies_describe_which_media_a_reader_can_claim() {
        assert!(GraphicsMediaPolicy::default().allows(GraphicsMedium::TempFile));
        assert!(GraphicsMediaPolicy::SHARED.allows(GraphicsMedium::File));
        assert!(!GraphicsMediaPolicy::SHARED.allows(GraphicsMedium::SharedMemory));
        assert!(!GraphicsMediaPolicy::NONE.allows(GraphicsMedium::File));
        assert!(GraphicsMediaPolicy::NONE.allows(GraphicsMedium::Direct));
        assert!(GraphicsMedium::TempFile.consumes_source());
        assert!(!GraphicsMedium::File.consumes_source());
    }
}
