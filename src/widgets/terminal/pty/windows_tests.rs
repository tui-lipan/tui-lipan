//! Exercise the byte-mode anonymous pipe used for ConPTY output, without a console or a child.
use std::fs::File;
use std::io::Write;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::ptr;

use super::super::read_gate::MAXIMUM_CREDIT;
use super::*;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreatePipe(
        read: *mut RawHandle,
        write: *mut RawHandle,
        attributes: *const std::ffi::c_void,
        size: u32,
    ) -> i32;
}

fn pipe() -> (File, File) {
    let mut read = ptr::null_mut();
    let mut write = ptr::null_mut();
    // Anonymous pipes are byte-mode, as are portable-pty's ConPTY output pipes. Give this fixture
    // enough capacity to queue the whole test before reading; no writer thread or sleeps needed.
    assert_ne!(
        unsafe { CreatePipe(&mut read, &mut write, ptr::null(), 65536) },
        0
    );
    // SAFETY: CreatePipe succeeded and returned two distinct owned handles.
    unsafe { (File::from_raw_handle(read), File::from_raw_handle(write)) }
}

fn credit(bytes: usize) -> ReadBudget {
    let gate = TerminalReadGate::default();
    gate.set_credit(bytes, true, Duration::from_millis(250));
    gate.read_budget()
}

#[test]
fn one_credit_drains_more_than_eight_kib_queued_across_multiple_writes() {
    let (mut reader, mut writer) = pipe();
    writer.write_all(&[7; 12000]).unwrap();
    writer.write_all(&[9; 12000]).unwrap();
    let mut buffer = [0; INITIAL_READ_BUFFER_SIZE];
    let budget = credit(MAXIMUM_CREDIT);
    let read = read_with_budget(&mut reader, &mut buffer, budget).unwrap();
    let mut output = buffer[..read].to_vec();
    drain_pending(&mut reader, &mut buffer, &mut output, budget.bytes);
    assert_eq!(output.len(), 24000);
    assert_eq!(&output[..12000], &[7; 12000]);
    assert_eq!(&output[12000..], &[9; 12000]);
    // The writer stays open: a full-sized request must not wait for future output to fill it.
}

#[test]
fn a_small_credit_leaves_the_remaining_bytes_for_the_next_credit() {
    let (mut reader, mut writer) = pipe();
    writer.write_all(&[5; 24000]).unwrap();
    let mut buffer = [0; INITIAL_READ_BUFFER_SIZE];
    assert_eq!(
        read_with_budget(&mut reader, &mut buffer, credit(10000)).unwrap(),
        10000
    );
    assert_eq!(
        read_with_budget(&mut reader, &mut buffer, credit(MAXIMUM_CREDIT)).unwrap(),
        14000
    );
}

#[test]
fn unrestricted_reads_keep_the_eight_kib_limit() {
    let (mut reader, mut writer) = pipe();
    writer.write_all(&[5; 24000]).unwrap();
    let mut buffer = [0; INITIAL_READ_BUFFER_SIZE];
    let budget = TerminalReadGate::default().read_budget();
    assert_eq!(
        read_with_budget(&mut reader, &mut buffer, budget).unwrap(),
        PTY_READ_BUFFER_SIZE
    );
}
