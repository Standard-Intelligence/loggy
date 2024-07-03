use std::{
    fs::{self, File},
    io::{self, ErrorKind, Read, Write},
    mem::MaybeUninit,
    os::unix::io::{FromRawFd, IntoRawFd, RawFd},
    path::PathBuf,
};

use libc::{STDERR_FILENO, STDOUT_FILENO};
use memchr::memrchr;
use polling::{Event, Events, Poller};

use crate::util;

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

pub fn tee(
    child_stdout: Option<File>,
    child_stderr: Option<File>,
    mut log_file: File,
    log_filename: PathBuf,
) {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();

    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();

    let poller = Poller::new().expect("failed to create poller");
    let mut active_fds = 0;

    if let Some(ref child_stdout) = child_stdout {
        active_fds += 1;
        stdout_buf.reserve(INITIAL_BUF_LEN);
        util::set_nonblocking(child_stdout);
        unsafe {
            poller
                .add(child_stdout, Event::readable(STDOUT_FILENO as _))
                .expect("failed to add fd to poller");
        }
    }

    if let Some(ref child_stderr) = child_stderr {
        active_fds += 1;
        stderr_buf.reserve(INITIAL_BUF_LEN);
        util::set_nonblocking(child_stderr);
        unsafe {
            poller
                .add(child_stderr, Event::readable(STDERR_FILENO as _))
                .expect("failed to add fd to poller");
        }
    }

    let mut child_stdout = util::fd_or_dev_null(child_stdout);
    let mut child_stderr = util::fd_or_dev_null(child_stderr);

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
