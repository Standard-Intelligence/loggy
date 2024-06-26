use core::panic;
use std::{
    env,
    fs::File,
    io::{self, ErrorKind, Write},
    os::unix::{
        io::{FromRawFd, IntoRawFd},
        process::CommandExt,
    },
    path::PathBuf,
    process::{self, Command, Stdio},
};

mod log;
mod tee;
mod util;

fn main() {
    let mut args = env::args_os();
    let mut wrapped = PathBuf::from(args.next().unwrap());

    let argv0_loggy = wrapped.file_name().map_or(false, |s| s == "loggy");
    if argv0_loggy {
        match args.next() {
            Some(arg) => wrapped = PathBuf::from(arg),
            None => return tee::passthrough(),
        }
    }

    wrapped = match util::which_super(&wrapped) {
        Ok(w) => w,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            eprintln!("{}: command not found", wrapped.display());
            process::exit(127)
        }
        Err(e) => panic!("failed to find command: {e}"),
    };

    let (args_str, prefix) = match log::args_str_and_prefix(argv0_loggy) {
        Some(s) => s,
        None => {
            let err = Command::new(&wrapped).args(args).exec();
            panic!("failed to exec command: {err}")
        }
    };

    let (mut log_file, log_filename) =
        log::open_log_file(&prefix).expect("failed to open log file");
    writeln!(log_file, "[loggy] command: {args_str}").expect("failed to write to log file");

    util::nohup();

    // TODO: write our own implementation of stdbuf that we can run on pre_exec
    // which can set argv[0] sans path (e.g. argv[0] = "mv", not "/usr/bin/mv")
    let mut child = match Command::new("stdbuf")
        .args(["-oL", "-eL", "--"])
        .arg(&wrapped)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_LOGGY", "1")
        .spawn()
    {
        Ok(child) => child,
        Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
            eprintln!("[loggy] stdbuf not found, running command directly");
            let mut args = env::args_os();
            if argv0_loggy {
                args.next();
            }
            Command::new(wrapped)
                .args(args)
                .stdin(Stdio::inherit())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .env("NO_LOGGY", "1")
                .spawn()
                .expect("failed to spawn child process")
        }
        Err(e) => panic!("failed to spawn child process: {e}"),
    };

    let child_stdout = unsafe { File::from_raw_fd(child.stdout.take().unwrap().into_raw_fd()) };
    let child_stderr = unsafe { File::from_raw_fd(child.stderr.take().unwrap().into_raw_fd()) };

    tee::child(child_stdout, child_stderr, log_file, log_filename);

    let status = child.wait().expect("failed to wait for child process");
    process::exit(status.code().unwrap_or(1));
}