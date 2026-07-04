//! enter, git-crypt and transcrypt (sub-shell with GIT variables set).

use std::process::Command;

use crate::context::Context;
use crate::git;
use crate::hooks;
use crate::util;

pub fn enter(ctx: &mut Context, args: &[String]) {
    let command = args.join(" ");
    require_shell(ctx);
    git::require_repo(ctx);

    let shell = std::env::var("SHELL").unwrap_or_default();
    let mut shell_opts: Vec<String> = Vec::new();
    let mut shell_path = "";
    if shell.ends_with("bash") {
        shell_opts.push("--norc".to_string());
        shell_path = "\\w";
    } else if shell.ends_with("csh") || shell.ends_with("zsh") {
        shell_opts.push("-f".to_string());
        if shell.ends_with("zsh") && std::env::var("TERM").as_deref() == Ok("dumb") {
            // Disable ZLE for tramp
            shell_opts.push("--no-zle".to_string());
        }
        shell_path = "%~";
    }

    std::env::set_var("GIT_WORK_TREE", &ctx.work);

    if command.is_empty() {
        println!("Entering yadm repo");
    }

    let yadm_prompt = format!("yadm shell ({}) {} > ", ctx.repo, shell_path);
    let mut c = Command::new(&shell);
    c.env("PROMPT", &yadm_prompt).env("PS1", &yadm_prompt);
    c.args(&shell_opts);
    if !command.is_empty() {
        c.arg("-c").arg(&command);
    }
    let return_code = git::exit_code(c.status());

    if command.is_empty() {
        println!("Leaving yadm repo");
    } else {
        hooks::exit_with_hook(ctx, return_code);
    }
}

pub fn git_crypt(ctx: &mut Context, args: &[String]) {
    require_git_crypt(ctx);
    let command = format!("{} {}", ctx.git_crypt_program, args.join(" "));
    enter(ctx, &[command]);
}

pub fn transcrypt(ctx: &mut Context, args: &[String]) {
    require_transcrypt(ctx);
    let command = format!("{} {}", ctx.transcrypt_program, args.join(" "));
    enter(ctx, &[command]);
}

fn require_shell(ctx: &Context) {
    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.is_empty() || !util::is_executable_file(std::path::Path::new(&shell)) {
        util::error_out(ctx, "$SHELL does not refer to an executable.");
    }
}

fn require_git_crypt(ctx: &Context) {
    if !util::command_exists(&ctx.git_crypt_program) {
        let msg = format!(
            "This functionality requires git-crypt to be installed, but the command '{}' cannot be located.",
            ctx.git_crypt_program
        );
        util::error_out(ctx, &msg);
    }
}

fn require_transcrypt(ctx: &Context) {
    if !util::command_exists(&ctx.transcrypt_program) {
        let msg = format!(
            "This functionality requires transcrypt to be installed, but the command '{}' cannot be located.",
            ctx.transcrypt_program
        );
        util::error_out(ctx, &msg);
    }
}
