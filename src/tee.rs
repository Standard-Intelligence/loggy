use std::{
    fs::{self, File},
    io::{self, ErrorKind, Read, Write},
    mem::MaybeUninit,
    os::{
        fd::AsRawFd,
        unix::io::{FromRawFd, IntoRawFd, RawFd},
    },
    path::PathBuf,
};

use libc::{STDERR_FILENO, STDOUT_FILENO};
use memchr::memrchr;
use polling::{Event, Events, Poller};

use crate::{log::open_log_file, util};

const INITIAL_BUF_LEN: usize = 32768;

fn handle_read(
    buf: &mut Vec<u8>,
    reader: &mut File,
    writers: &mut [&mut File],
    mut on_write: impl FnMut(),
) -> bool {
    loop {
        let read_slice = buf.spare_capacity_mut();
        match reader.read(unsafe { &mut *(read_slice as *mut [MaybeUninit<u8>] as *mut [u8]) }) {
            Ok(0) => return true,
            Ok(n) => {
                assert!(n <= read_slice.len());
                let old_len = buf.len();
                unsafe { buf.set_len(buf.len() + n) };

                if let Some(idx) = memrchr(b'\n', &buf[old_len..]) {
                    on_write();
                    for writer in writers.iter_mut() {
                        match writer.write_all(&buf[..old_len + idx + 1]) {
                            Ok(()) => {}
                            // TODO: stop even trying to write to fds once broken
                            Err(e) if e.kind() == ErrorKind::BrokenPipe => {}
                            Err(e) => panic!("failed to write to output: {e}"),
                        }
                    }
                    if old_len + idx + 1 == buf.len() {
                        buf.clear();
                    } else {
                        buf.drain(..old_len + idx + 1);
                    }
                }

                if old_len + n >= buf.capacity() {
                    buf.reserve(buf.capacity()); // double the capacity
                    continue;
                } else {
                    buf.shrink_to(INITIAL_BUF_LEN);
                    return false;
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => return false,
            Err(e) => panic!("failed to read from input: {e}"),
        }
    }
}

pub fn child(
    mut child_stdout: File,
    mut child_stderr: File,
    mut log_file: File,
    log_filename: PathBuf,
) {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();

    let mut stdout_buf = Vec::with_capacity(INITIAL_BUF_LEN);
    let mut stderr_buf = Vec::with_capacity(INITIAL_BUF_LEN);

    util::set_nonblocking(&child_stdout);
    util::set_nonblocking(&child_stderr);

    let poller = Poller::new().expect("failed to create poller");
    unsafe {
        poller
            .add(&child_stdout, Event::readable(STDOUT_FILENO as _))
            .expect("failed to add fd to poller");
        poller
            .add(&child_stderr, Event::readable(STDERR_FILENO as _))
            .expect("failed to add fd to poller");
    }

    let mut active_fds = 2;
    let mut written_any = false;
    let mut events = Events::new();
    loop {
        poller
            .wait(&mut events, None)
            .expect("failed to wait for events");
        for ev in events.iter() {
            let (buf, reader) = match ev.key as RawFd {
                STDOUT_FILENO => (&mut stdout_buf, &mut child_stdout),
                STDERR_FILENO => (&mut stderr_buf, &mut child_stderr),
                _ => unreachable!(),
            };
            let mut writer = unsafe { File::from_raw_fd(ev.key as RawFd) };

            let eof = handle_read(buf, reader, &mut [&mut writer, &mut log_file], || {
                if !written_any {
                    eprintln!("[loggy] logging to {}", log_filename.display());
                    written_any = true;
                }
            });

            writer.into_raw_fd(); // avoid closing the fd

            if eof {
                poller
                    .delete(reader)
                    .expect("failed to delete reader from poller");
                active_fds -= 1;
            } else {
                poller
                    .modify(reader, Event::readable(ev.key as _))
                    .expect("failed to modify reader in poller");
            }
        }

        if active_fds == 0 {
            break;
        }

        events.clear();
    }

    stdout
        .write_all(&stdout_buf)
        .expect("failed to write to stdout");
    stderr
        .write_all(&stderr_buf)
        .expect("failed to write to stderr");

    if written_any {
        log_file
            .write_all(&stdout_buf)
            .expect("failed to write stdout to log file");
        log_file
            .write_all(&stderr_buf)
            .expect("failed to write stderr to log file");
    } else {
        fs::remove_file(&log_filename).expect("failed to remove empty log file");
    }
}

pub fn passthrough() {
    let (mut log_file, log_filename) = open_log_file("loggy").expect("failed to open log file");
    writeln!(log_file, "[loggy] command: loggy").expect("failed to write to log file");

    let stdin = io::stdin().lock();
    let stdout = io::stdout().lock();
    let mut stdin_file = unsafe { File::from_raw_fd(stdin.as_raw_fd()) };
    let mut stdout_file = unsafe { File::from_raw_fd(stdout.as_raw_fd()) };

    let mut buf = Vec::with_capacity(INITIAL_BUF_LEN);

    // TODO: don't fail if writing to stdout becomes impossible because the other side has closed it (e.g. `loggy | head -n1`)
    let mut written_any = false;
    while !handle_read(
        &mut buf,
        &mut stdin_file,
        &mut [&mut stdout_file, &mut log_file],
        || {
            if !written_any {
                eprintln!("[loggy] logging to {}", log_filename.display());
                written_any = true;
            }
        },
    ) {}

    if written_any {
        log_file
            .write_all(&buf)
            .expect("failed to write stdin to log file");
    } else {
        fs::remove_file(&log_filename).expect("failed to remove empty log file");
    }
}
