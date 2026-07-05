//! Operating system / distro detection (yadm's set_operating_system,
//! set_awk, query_distro, query_distro_family).

use std::cell::RefCell;
use std::collections::HashMap;
use std::process::Command;

use crate::context::Context;
use crate::util;

thread_local! {
    /// Cache of `command name -> absolute path` for the helper programs
    /// `capture` spawns (uname, id, ...), so `Command` skips a PATH re-scan per
    /// call. Same file PATH would find, so output is unchanged.
    static EXE_PATHS: RefCell<HashMap<String, Option<String>>> = RefCell::new(HashMap::new());
}

/// Resolve `cmd` to an absolute path (cached), falling back to `cmd` verbatim.
fn resolve_exe(cmd: &str) -> String {
    if cmd.contains('/') {
        return cmd.to_string();
    }
    EXE_PATHS.with(|c| {
        c.borrow_mut()
            .entry(cmd.to_string())
            .or_insert_with(|| util::command_path(cmd))
            .clone()
            .unwrap_or_else(|| cmd.to_string())
    })
}

/// Run a command and capture stdout with `$(...)` semantics (trailing
/// newlines stripped, stderr discarded, failures yield "").
pub fn capture(cmd: &str, args: &[&str]) -> String {
    util::record_spawn(cmd, args);
    match Command::new(resolve_exe(cmd))
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(out) => util::trim_trailing_newlines(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => String::new(),
    }
}

pub fn set_operating_system(ctx: &mut Context) {
    let proc_version = std::fs::read_to_string(&ctx.proc_version).unwrap_or_default();
    if proc_version.contains("Microsoft") || proc_version.contains("microsoft") {
        ctx.operating_system = "WSL".to_string();
    } else {
        ctx.operating_system = capture("uname", &["-s"]);
    }

    if ctx.operating_system.starts_with("CYGWIN")
        || ctx.operating_system.starts_with("MINGW")
        || ctx.operating_system.starts_with("MSYS")
    {
        let git_version = capture(&ctx.git_program, &["--version"]);
        if git_version.contains("windows") {
            ctx.use_cygpath = true;
        }
        ctx.operating_system = capture("uname", &["-o"]);
    }
}

/// Narrow awk_program to the first available candidate (gawk, then awk).
pub fn set_awk(ctx: &mut Context) {
    for pgm in ctx.awk_program.clone() {
        if util::command_exists(&pgm) {
            ctx.awk_program = vec![pgm];
            return;
        }
    }
}

pub fn query_distro(ctx: &Context) -> String {
    if util::command_exists(&ctx.lsb_release_program) {
        return capture(&ctx.lsb_release_program, &["-si"]);
    }
    if let Some(lines) = util::read_lines(&ctx.os_release) {
        for line in lines {
            if let Some(rest) = line.strip_prefix("ID=") {
                return rest.replace('"', "");
            }
        }
    }
    String::new()
}

pub fn query_distro_family(ctx: &Context) -> String {
    let mut family = String::new();
    if let Some(lines) = util::read_lines(&ctx.os_release) {
        for line in lines {
            if let Some(rest) = line.strip_prefix("ID_LIKE=") {
                family = rest.to_string();
                break;
            } else if let Some(rest) = line.strip_prefix("ID=") {
                // no break; only used as fallback in case ID_LIKE isn't found
                family = rest.to_string();
            }
        }
    }
    family.replace('"', "")
}
