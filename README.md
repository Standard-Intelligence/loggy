# `loggy`
`loggy` wraps commands and automatically tees their output to log files without modifying original programs or scripts.

## Features
- Logs command output (stdout and stderr) to automatically-created files
- Passthrough mode for piping: like `tee` without the need to specify a name
- High performance: gigabytes of output per second, sub-millisecond startup time
- Handles SIGHUP and closed stdout/stderr (continues after terminal or SSH disconnection)
- Wraps any command via symlinks, aliases, or Bash magic; configurable via regex patterns

## Installation

### From source

```
sudo -E "$(command -v cargo)" install --root /usr/local si-loggy
```

### From Git

```sh
sudo -E "$(command -v cargo)" install --root /usr/local --git https://github.com/Standard-Intelligence/loggy
```

## Usage

### Manual invocation

Wrap a command with `loggy` to log its output:
```
$ loggy echo hello world
[loggy] logging to /home/robert/logs/echo-0.log
hello world
```

Or, pipe a command to `loggy`:
```
$ echo hello world | loggy
[loggy] logging to /home/robert/logs/loggy-14.log
hello world
```

Wrapping a command (i.e. running `loggy command` or using one of the below methods) is preferable to piping to `loggy` because `loggy` can handle SIGHUP and closed stdout/stderr (such as from a detached terminal) on behalf of wrapped processes.

### Symlinks and aliases

Symlink `loggy` to each program to log:
```
ln -s /usr/local/bin/loggy /usr/local/bin/pip
ln -s /usr/local/bin/loggy /usr/local/bin/python
ln -s /usr/local/bin/loggy /usr/local/bin/python3
```

When a wrapped program is run, `loggy` creates a log file in `~/logs` based on the command name and any arguments corresponding to existing files:

```
$ pip install -r requirements.txt
[loggy] logging to /home/user/logs/pip-requirements.txt-0.log
```

If the wrapped program produces no output (e.g. `loggy true`), no log file is created.

### Environment variable

`loggy` can be temporarily disabled for a wrapped command by setting the `NO_LOGGY` environment variable (to any value except `0`):

```sh
NO_LOGGY= python3 -m print-all-my-secrets
```

### Exec hook

For `loggy` to handle broad patterns that could occur in any command (such as the `/sandbox` example below), it must run for every command (at which point a config file determines which commands are actually logged). Add this to your `.bashrc` to run every command under `loggy`:

```bash
loggy() {
    if [[ -v AT_PROMPT ]]; then
        unset AT_PROMPT
        local t="$(type -t $@)"
        if [[ "$t" == "file" ]]; then
            command loggy $@
            shopt -s extdebug
            return 1
        elif [[ "$t" == "function" ]]; then
            $@
            shopt -s extdebug
            return 1
        fi
    fi
}

unset -f command_not_found_handle
trap 'loggy ${BASH_COMMAND}' DEBUG
PROMPT_COMMAND="shopt -u extdebug; ${PROMPT_COMMAND}; AT_PROMPT="
```

### Config file

By default, `loggy` creates logs for anything it wraps. You may want finer control than symlinks and `NO_LOGGY` offer, e.g. to log only invocations of a certain Python module. As a last resort, `loggy` loads a list of regular expressions from the first existing file out of `~/.config/loggy` and `/etc/loggy`. If one of these files exists, *and* a command is wrapped by `loggy`, it will be logged if and only if it matches one of the regular expressions from the config file.

Example configuration:
```
# Log `make` commands with any arguments
^make( |$)

# Log all Python scripts, but not e.g. `python -m pip`
^([^ ]*/)?(python3?|python-[0-9.]+) .*\.py\b

# Log invocations of a specific Python module
^([^ ]*/)?(python3?|python-[0-9.]+) -m ?torch\.distributed\.run

# Log any commands involving the /sandbox directory
/sandbox
```

We recommend using config files as a last resort, because writing regex is hard :)

## Tips

If you've run `loggy` and then disconnected the terminal or SSH connection, run `tail -f ~/logs/example.log` in a new terminal to stream its output.

If installed, `loggy` automatically uses [`stdbuf`](https://www.gnu.org/software/coreutils/manual/html_node/stdbuf-invocation.html) to ensure wrapped commands write their output to the terminal and log file line by line instead of with [full buffering](https://www.gnu.org/software/libc/manual/html_node/Buffering-Concepts.html). Ensure `stdbuf` is installed for optimal performance.
