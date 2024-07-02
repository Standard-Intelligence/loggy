use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader},
    num::NonZero,
    path::{Path, PathBuf},
};

use regex::Regex;

fn config_path() -> Option<PathBuf> {
    let mut cfg = PathBuf::from(env::var_os("HOME").expect("${HOME} must be set"));
    cfg.push(".config/loggy");
    if cfg.exists() {
        Some(cfg)
    } else {
        cfg.clear();
        cfg.push("/etc/loggy");
        if cfg.exists() {
            Some(cfg)
        } else {
            None
        }
    }
}

fn config_name_len(config: impl AsRef<Path>, args_str: &str) -> Option<NonZero<usize>> {
    for line in BufReader::new(File::open(config).expect("failed to open config file")).lines() {
        let line = line.expect("failed to read line from config file");
        if !line.is_empty() && !line.starts_with('#') {
            match Regex::new(&line) {
                Ok(re) => {
                    if let Some(m) = re.find(&args_str) {
                        return NonZero::new(m.range().end);
                    }
                }
                Err(e) => panic!("could not compile regex\nregex: {line}\nerror: {e}"),
            }
        }
    }

    None
}

pub fn args_str_and_prefix(argv0_loggy: bool) -> Option<(String, String)> {
    if env::var_os("NO_LOGGY").map_or(false, |s| s != "0") {
        return None;
    }

    let mut args = env::args();
    if argv0_loggy {
        args.next();
    }

    let mut args_str = args.next().unwrap();
    let mut program_name_len = args_str.len();
    for arg in args {
        args_str.push(' ');
        args_str.push_str(&arg);
    }
    args_str.shrink_to_fit();

    if !argv0_loggy {
        if let Some(config) = config_path() {
            program_name_len = config_name_len(config, &args_str).map(NonZero::get)?;
        }
    }

    let mut args = env::args();
    if argv0_loggy {
        args.next();
    }

    let mut prefix = args.next().unwrap();
    prefix.reserve_exact(args_str.len() - prefix.len());
    for arg in args {
        if prefix.len() + 1 < program_name_len
            || (!arg.starts_with('-') && Path::new(&arg).exists())
        {
            prefix.push('-');
            prefix.push_str(arg.trim_matches('-'));
        }
    }
    prefix.shrink_to_fit();

    unsafe {
        for b in prefix.as_bytes_mut() {
            if let b' ' | b'\t' | b'\n' | b'!' | b'"' | b'#' | b'$' | b'&' | b'\'' | b'(' | b')'
            | b'*' | b';' | b'<' | b'=' | b'>' | b'?' | b'[' | b'\\' | b']' | b'^' | b'`'
            | b'{' | b'|' | b'}' = b
            {
                *b = b'-';
            }
        }
    }

    Some((args_str, prefix))
}

pub fn open_log_file(prefix: &str) -> io::Result<(File, PathBuf)> {
    let mut log_path = PathBuf::from(env::var_os("HOME").expect("${HOME} must be set"));
    log_path.push("logs");

    let mut i = 0;
    loop {
        log_path.push(format!("{prefix}-{i}.log"));

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&log_path)
        {
            Ok(file) => return Ok((file, log_path)),
            Err(e) => match e.kind() {
                io::ErrorKind::NotFound => {
                    log_path.pop();
                    fs::create_dir_all(&log_path).expect("failed to create log directory");
                }
                io::ErrorKind::AlreadyExists => {
                    log_path.pop();
                    i += 1;
                }
                _ => return Err(e),
            },
        }
    }
}
