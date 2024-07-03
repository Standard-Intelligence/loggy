use std::{
    env,
    fs::File,
    io::{self, ErrorKind},
    os::{
        fd::{FromRawFd, IntoRawFd},
        unix::fs::PermissionsExt,
    },
    path::{Path, PathBuf},
};

use libc::{c_int, fcntl, F_GETFL, F_SETFL, O_NONBLOCK};
use polling::AsRawSource;

pub fn which_super<T: AsRef<Path>>(binary_path: T) -> io::Result<PathBuf> {
    let binary_name = binary_path.as_ref().file_name().unwrap();
    let me = env::current_exe()
        .expect("failed to get current executable path")
        .canonicalize()
        .expect("failed to canonicalize current executable path");
    let mut path = PathBuf::new();
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            path.push(&dir);
            path.push(binary_name);
            match path.canonicalize() {
                Ok(bin) => {
                    if bin != me {
                        let metadata = bin.metadata().expect("failed to get file metadata");
                        if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
                            return Ok(path);
                        }
                    }
                }
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            }
            path.clear();
        }
    }

    Err(io::Error::from(ErrorKind::NotFound))
}

pub fn handle_errno(ret: c_int) -> io::Result<c_int> {
    match ret {
        -1 => Err(io::Error::last_os_error()),
        ret => Ok(ret),
    }
}

pub fn fd_or_dev_null(s: Option<impl IntoRawFd>) -> File {
    match s {
        Some(fd) => unsafe { File::from_raw_fd(fd.into_raw_fd()) },
        None => File::open("/dev/null").expect("failed to open /dev/null"),
    }
}

pub fn nohup() {
    unsafe {
        // only call setsid if not a process group (i.e. session) leader
        if libc::getpid() != libc::getpgid(0) {
            handle_errno(libc::setsid()).expect("failed to setsid");
        }

        // ignore SIGHUP
        if libc::signal(libc::SIGHUP, libc::SIG_IGN) == libc::SIG_ERR {
            panic!("failed to ignore SIGHUP: {}", io::Error::last_os_error());
        }
    }
}

pub fn set_nonblocking(fd: impl AsRawSource) {
    let fd = fd.raw();
    unsafe {
        let flags = handle_errno(fcntl(fd, F_GETFL)).expect("failed to get file descriptor flags");
        handle_errno(fcntl(fd, F_SETFL, flags | O_NONBLOCK))
            .expect("failed to set file descriptor flags");
    }
}
