//! Small helpers mirroring yadm's utility and echo-replacement functions.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::context::Context;
use crate::hooks;

/// Interpret backslash escapes like `printf '%b'` does (yadm's echo_e).
pub fn expand_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('a') => out.push('\u{7}'),
            Some('b') => out.push('\u{8}'),
            Some('e') => out.push('\u{1b}'),
            Some('f') => out.push('\u{c}'),
            Some('v') => out.push('\u{b}'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// yadm's debug(): print "DEBUG: ..." (with %b escapes) when -d was given.
pub fn debug(ctx: &Context, msg: &str) {
    if ctx.debug {
        println!("DEBUG: {}", expand_escapes(msg));
    }
}

/// yadm's error_out(): print "ERROR: ..." to stderr, run post hook, exit 1.
pub fn error_out(ctx: &Context, msg: &str) -> ! {
    eprintln!("ERROR: {}", expand_escapes(msg));
    hooks::exit_with_hook(ctx, 1)
}

/// `command -v` equivalent (PATH lookup; names with '/' checked directly).
pub fn command_exists(cmd: &str) -> bool {
    command_path(cmd).is_some()
}

pub fn command_path(cmd: &str) -> Option<String> {
    if cmd.contains('/') {
        return is_executable_file(Path::new(cmd)).then(|| cmd.to_string());
    }
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        let dir = if dir.is_empty() { "." } else { dir };
        let cand = format!("{dir}/{cmd}");
        if is_executable_file(Path::new(&cand)) {
            return Some(cand);
        }
    }
    None
}

pub fn is_executable_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(p) {
        Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

/// `[ -x path ]` (also true for executable directories, unlike command lookup).
pub fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(p) {
        Ok(m) => m.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// `$(...)` strips all trailing newlines from captured output.
pub fn trim_trailing_newlines(s: &str) -> String {
    s.trim_end_matches('\n').to_string()
}

/// yadm's get_mode(): permission bits as a 4-digit octal string (BSD stat -f %p style).
pub fn get_mode(filename: &str) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(filename).ok()?;
    Some(format!("{:04o}", meta.mode() & 0o7777))
}

/// yadm's copy_perms(): copy permission bits from source to target; debug on failure.
pub fn copy_perms(ctx: &Context, source: &str, target: &str) {
    use std::os::unix::fs::PermissionsExt;
    let mode = get_mode(source);
    let ok = match &mode {
        Some(m) => {
            let bits = u32::from_str_radix(m, 8).unwrap_or(0);
            std::fs::set_permissions(target, std::fs::Permissions::from_mode(bits)).is_ok()
        }
        None => false,
    };
    if !ok {
        debug(
            ctx,
            &format!(
                "Unable to copy perms '{}' from '{}' to '{}'",
                mode.unwrap_or_default(),
                source,
                target
            ),
        );
    }
}

/// `read -r answer </dev/tty` — one line from the controlling terminal,
/// trimmed of surrounding IFS whitespace. None when /dev/tty can't be read.
pub fn read_tty_line() -> Option<String> {
    let tty = File::open("/dev/tty").ok()?;
    let mut line = String::new();
    BufReader::new(tty).read_line(&mut line).ok()?;
    Some(line.trim_matches(['\n', '\t', ' ']).to_string())
}

/// Read a file like bash `while IFS='' read -r line || [ -n "$line" ]` loops:
/// yields every line, including a final line without a trailing newline.
pub fn read_lines(path: &str) -> Option<Vec<String>> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut lines: Vec<String> = content.split('\n').map(|s| s.to_string()).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    Some(lines)
}
