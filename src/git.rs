//! Git plumbing: requirement checks, passthrough execution, repo configuration.

use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use crate::config;
use crate::context::Context;
use crate::paths;
use crate::privdirs;
use crate::util;

/// Build a git command (GIT_DIR is inherited from the process environment).
pub fn cmd(ctx: &Context) -> Command {
    Command::new(&ctx.git_program)
}

/// Run git with the given args, capture stdout ($(...) semantics), return
/// (stdout, success). stderr goes to the given destination.
pub fn capture(ctx: &Context, args: &[&str], silence_stderr: bool) -> (String, bool) {
    let mut c = cmd(ctx);
    c.args(args);
    if silence_stderr {
        c.stderr(Stdio::null());
    } else {
        c.stderr(Stdio::inherit());
    }
    match c.output() {
        Ok(o) => (
            util::trim_trailing_newlines(&String::from_utf8_lossy(&o.stdout)),
            o.status.success(),
        ),
        Err(_) => (String::new(), false),
    }
}

/// Run git with the given args, stdio inherited; return the exit code.
pub fn run(ctx: &Context, args: &[&str]) -> i32 {
    exit_code(cmd(ctx).args(args).status())
}

pub fn exit_code(status: std::io::Result<ExitStatus>) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    match status {
        Ok(s) => s.code().unwrap_or_else(|| 128 + s.signal().unwrap_or(0)),
        Err(_) => 127,
    }
}

pub fn require_repo(ctx: &Context) {
    if !Path::new(&ctx.repo).is_dir() {
        util::error_out(
            ctx,
            "Git repo does not exist. did you forget to run 'init' or 'clone'?",
        );
    }
}

pub fn require_git(ctx: &mut Context) {
    let alt_git = config::config_output(ctx, &["yadm.git-program"]);
    let mut more_info = "";
    if !alt_git.is_empty() {
        ctx.git_program = alt_git;
        more_info = "\\nThis command has been set via the yadm.git-program configuration.";
    }
    if !util::command_exists(&ctx.git_program) {
        util::error_out(
            ctx,
            &format!(
                "This functionality requires Git to be installed, but the command '{}' cannot be located.{}",
                ctx.git_program, more_info
            ),
        );
    }
}

/// Any non-internal command is passed through to git.
pub fn git_command(ctx: &mut Context, args: &[String]) -> i32 {
    require_repo(ctx);

    // translate 'gitconfig' to 'config' -- 'config' is reserved for yadm
    let mut args = args.to_vec();
    if args[0] == "gitconfig" {
        args[0] = "config".to_string();
    }

    // ensure private .ssh and .gnupg directories exist first
    if ctx.work == ctx.home {
        let auto_private_dirs = config::config_output(ctx, &["--bool", "yadm.auto-private-dirs"]);
        if auto_private_dirs != "false" {
            let pdirs = privdirs::private_dirs_all(ctx);
            privdirs::assert_private_dirs(ctx, &pdirs);
        }
    }

    ctx.changes_possible = true;

    // pass commands through to git
    util::debug(
        ctx,
        &format!("Running git command {} {}", ctx.git_program, args.join(" ")),
    );
    exit_code(cmd(ctx).args(&args).status())
}

pub fn configure_repo(ctx: &mut Context) {
    util::debug(ctx, "Configuring new repo");

    let worktree = paths::mixed_path(ctx, &ctx.work);
    // change bare to false (there is a working directory)
    let _ = run(ctx, &["config", "core.bare", "false"]);
    // set the worktree for the yadm repo
    let _ = run(ctx, &["config", "core.worktree", &worktree]);
    // by default, do not show untracked files and directories
    let _ = run(ctx, &["config", "status.showUntrackedFiles", "no"]);
    // possibly used later to ensure we're working on the yadm repo
    let _ = run(ctx, &["config", "yadm.managed", "true"]);
}
