//! Losing the terminal to another process group while the app is drawing.
//!
//! A standalone binary because it needs a real pty with a session on it, and
//! job control is process-global. The test runs itself in two more roles: a
//! minimal job-control shell that leads the pty's session, and the app that
//! shell runs as its foreground job. The parent plays the terminal, answering
//! the startup queries, and records every byte for the assertions.
#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::ffi::CStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tui_lipan::prelude::*;

const ROLE_ENV: &str = "TUI_LIPAN_JOB_CONTROL_ROLE";
const REPORT_ENV: &str = "TUI_LIPAN_JOB_CONTROL_REPORT";
const TEST_NAME: &str = "a_background_stop_hands_back_the_terminal_and_reclaims_it";

const STOLEN: &[u8] = b"<STOLEN>";
const BG: &[u8] = b"<BG>";
const FG: &[u8] = b"<FG>";
const FRAME_START: &[u8] = b"\x1b[?2026h";
/// How the background release begins: CAN, then the end of synchronized output.
const RELEASE: &[u8] = b"\x18\x1b[?2026l";

#[test]
fn a_background_stop_hands_back_the_terminal_and_reclaims_it() {
    match std::env::var(ROLE_ENV).as_deref() {
        Ok("app") => return run_app(),
        Ok("shell") => return run_shell(),
        _ => {}
    }

    let report = std::env::temp_dir().join(format!("tui-lipan-job-control-{}", std::process::id()));
    let _ = fs::remove_file(&report);
    let (master, slave) = open_pty();

    let mut command = Command::new(std::env::current_exe().expect("test binary path"));
    command
        .args([TEST_NAME, "--exact", "--nocapture", "--test-threads=1"])
        .env(ROLE_ENV, "shell")
        .env(REPORT_ENV, &report)
        .stdin(Stdio::from(slave.try_clone().expect("dup slave")))
        .stdout(Stdio::from(slave.try_clone().expect("dup slave")))
        .stderr(Stdio::from(slave));
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut shell = command.spawn().expect("spawn shell role");
    // The command holds the slave ends; the master reads end-of-file only once
    // nothing but the session does.
    drop(command);

    let output = Arc::new(Mutex::new(Vec::new()));
    let terminal = std::thread::spawn({
        let output = Arc::clone(&output);
        let report = report.clone();
        move || play_terminal(File::from(master), &output, &report)
    });

    let deadline = Instant::now() + Duration::from_secs(60);
    while shell.try_wait().expect("wait for shell role").is_none() {
        if Instant::now() >= deadline {
            let _ = shell.kill();
            let _ = shell.wait();
            panic!("the shell role did not finish");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // The master reads end-of-file once nothing holds the pty open any more.
    let _ = terminal.join();
    let output = output.lock().unwrap().clone();
    let report = read_report(&report);
    let shown = String::from_utf8_lossy(&output);

    assert_eq!(report.get("error"), None, "{report:?}");
    for step in [
        "stopped_after_steal",
        "stopped_after_bg",
        "running_after_fg",
    ] {
        assert_eq!(
            report.get(step).map(String::as_str),
            Some("1"),
            "{step}: {report:?}"
        );
    }

    let stolen = find(&output, STOLEN, 0).expect("foreground stolen");
    let bg = find(&output, BG, stolen).expect("bg");
    let fg = find(&output, FG, bg).expect("fg");
    assert!(
        count(&output[..stolen], FRAME_START) >= 3,
        "the app was repainting continuously before it lost the terminal:\n{shown:?}"
    );

    let release = find(&output, RELEASE, stolen).unwrap_or_else(|| {
        panic!("the terminal was released after the foreground was stolen:\n{shown:?}")
    });
    assert!(release < bg, "released before bg:\n{shown:?}");
    let handed_back = &output[release..fg];
    for reset in [&b"\x1b[?1000l"[..], b"\x1b[?1003l", b"\x1b[?1049l"] {
        assert!(
            find(handed_back, reset, 0).is_some(),
            "{:?} is reset before the shell gets the terminal:\n{shown:?}",
            String::from_utf8_lossy(reset)
        );
    }
    assert_eq!(
        count(handed_back, FRAME_START),
        0,
        "no frame reaches the shell's screen, before bg or after it:\n{shown:?}"
    );

    let reentered = find(&output, b"\x1b[?1049h", fg)
        .unwrap_or_else(|| panic!("the alternate screen comes back after fg:\n{shown:?}"));
    let first_frame = find(&output, FRAME_START, fg)
        .unwrap_or_else(|| panic!("frames resume after fg:\n{shown:?}"));
    assert!(
        reentered < first_frame,
        "the terminal is taken back before the first frame after fg:\n{shown:?}"
    );
    assert!(
        find(&output[fg..first_frame], b"\x1b[?1000h", 0).is_some(),
        "mouse reporting comes back before the first frame:\n{shown:?}"
    );
}

/// The app: a spinner, which repaints on its own every 50ms.
fn run_app() {
    struct Spinning;

    impl Component for Spinning {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Spinner::new().label("drawing").into()
        }
    }

    let _ = App::new().mount(Spinning).run();
}

/// A job-control shell reduced to the steps under test: run the app in the
/// foreground, take the terminal away while it draws, `bg` it, then `fg` it.
fn run_shell() {
    let report = Report(std::env::var(REPORT_ENV).expect("report path"));
    // SAFETY: a shell ignores SIGTTOU so it can hand the terminal around.
    unsafe { libc::signal(libc::SIGTTOU, libc::SIG_IGN) };

    let mut app = Command::new(std::env::current_exe().expect("test binary path"));
    app.args([TEST_NAME, "--exact", "--nocapture", "--test-threads=1"])
        .env(ROLE_ENV, "app")
        .process_group(0);
    let mut app = app.spawn().expect("spawn app role");
    let job = app.id() as libc::pid_t;
    let tty = 0;
    // SAFETY: plain syscalls on the controlling terminal and our own job.
    let own_group = unsafe { libc::getpgrp() };
    let set_foreground = |group: libc::pid_t| unsafe { libc::tcsetpgrp(tty, group) };
    let marker = |text: &[u8]| {
        // SAFETY: the pointer and length describe the live slice.
        unsafe { libc::write(1, text.as_ptr().cast(), text.len()) };
    };
    let continue_job = || unsafe { libc::kill(-job, libc::SIGCONT) };

    set_foreground(job);
    wait_for_signal_file(&report.signal("drawing"));

    // Another process group takes the terminal; the parent then types, and
    // the app's next read finds it is in the background.
    set_foreground(own_group);
    marker(STOLEN);
    report.line("stopped_after_steal", u8::from(wait_for_stop(job)));
    // The shell's line editor reads whatever was typed while it has the
    // terminal, including the input that stopped the app.
    // SAFETY: a plain syscall on the controlling terminal.
    unsafe { libc::tcflush(tty, libc::TCIFLUSH) };

    // `bg`: continued, but the terminal stays with the shell.
    marker(BG);
    continue_job();
    std::thread::sleep(Duration::from_millis(200));
    report.line("stopped_after_bg", u8::from(wait_for_stop(job)));

    // `fg`: the terminal goes back first, then the job is continued.
    marker(FG);
    set_foreground(job);
    continue_job();
    wait_for_signal_file(&report.signal("redrawn"));
    let mut status = 0;
    // SAFETY: `status` is a valid out-pointer.
    let changed = unsafe { libc::waitpid(job, &mut status, libc::WNOHANG | libc::WUNTRACED) };
    report.line("running_after_fg", u8::from(changed == 0));
    if changed != 0 {
        report.line(
            "after_fg",
            format!(
                "stopped={} signal={} exited={} code={}",
                libc::WIFSTOPPED(status),
                libc::WSTOPSIG(status),
                libc::WIFEXITED(status),
                libc::WEXITSTATUS(status)
            ),
        );
    }

    let _ = app.kill();
    let _ = app.wait();
}

/// Wait for the parent to create `path`, which it does once the pty shows
/// what the next step needs. Gives up after a while; the assertions then say
/// what was missing.
fn wait_for_signal_file(path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether the job reports a stop within a few seconds.
fn wait_for_stop(job: libc::pid_t) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let mut status = 0;
        // SAFETY: `status` is a valid out-pointer.
        let changed = unsafe { libc::waitpid(job, &mut status, libc::WNOHANG | libc::WUNTRACED) };
        if changed == job {
            return libc::WIFSTOPPED(status);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Record everything the pty shows, answer the queries a terminal would, type
/// once the foreground has been stolen, and tell the shell role when the app is
/// drawing, before the steal and again after `fg`.
fn play_terminal(mut terminal: File, output: &Mutex<Vec<u8>>, report: &std::path::Path) {
    let report = Report(report.to_string_lossy().into_owned());
    let mut typed = false;
    let mut drawing = false;
    let mut redrawn = false;
    let mut buffer = [0; 8192];
    loop {
        let read = match terminal.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };
        let chunk = &buffer[..read];
        let mut output = output.lock().unwrap();
        output.extend_from_slice(chunk);
        if find(chunk, b"\x1b[6n", 0).is_some() {
            let _ = terminal.write_all(b"\x1b[1;1R");
        }
        if find(chunk, b"\x1b[c", 0).is_some() {
            let _ = terminal.write_all(b"\x1b[?62c");
        }
        if !drawing && count(&output, FRAME_START) >= 3 {
            drawing = true;
            let _ = File::create(report.signal("drawing"));
        }
        if !typed && find(&output, STOLEN, 0).is_some() {
            typed = true;
            // Mouse motion, as a user moving the pointer over the frozen UI.
            let _ = terminal.write_all(b"\x1b[<35;10;5M");
        }
        if !redrawn
            && let Some(fg) = find(&output, FG, 0)
            && count(&output[fg..], FRAME_START) >= 3
        {
            redrawn = true;
            let _ = File::create(report.signal("redrawn"));
        }
    }
}

fn open_pty() -> (OwnedFd, File) {
    // SAFETY: straightforward pty allocation; every return value is checked.
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        assert!(master >= 0, "posix_openpt failed");
        let master = OwnedFd::from_raw_fd(master);
        assert_eq!(libc::grantpt(master.as_raw_fd()), 0, "grantpt failed");
        assert_eq!(libc::unlockpt(master.as_raw_fd()), 0, "unlockpt failed");
        let mut name = [0 as libc::c_char; 128];
        assert_eq!(
            libc::ptsname_r(master.as_raw_fd(), name.as_mut_ptr(), name.len()),
            0,
            "ptsname_r failed"
        );
        let path = CStr::from_ptr(name.as_ptr()).to_string_lossy().into_owned();
        let winsize = libc::winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &winsize);
        let slave = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(path)
            .expect("open pty slave");
        (master, slave)
    }
}

struct Report(String);

impl Report {
    /// A file next to the report that one role creates to tell another to go on.
    fn signal(&self, name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(format!("{}.{name}", self.0))
    }

    fn line(&self, key: &str, value: impl std::fmt::Display) {
        if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(&self.0) {
            let _ = writeln!(file, "{key}={value}");
        }
    }
}

fn read_report(path: &std::path::Path) -> HashMap<String, String> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let report = Report(path.to_string_lossy().into_owned());
    for file in [
        path.to_path_buf(),
        report.signal("drawing"),
        report.signal("redrawn"),
    ] {
        let _ = fs::remove_file(file);
    }
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| at + from)
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}
