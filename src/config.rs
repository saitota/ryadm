//! yadm's config() — both the internal `$(config ...)` helper and the
//! `config` command — plus the introspection string constants.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::context::Context;
use crate::git;
use crate::paths;
use crate::util;

pub const INTROSPECT_COMMANDS: &str = "alt\n\
bootstrap\n\
clean\n\
clone\n\
config\n\
decrypt\n\
encrypt\n\
enter\n\
git-crypt\n\
gitconfig\n\
help\n\
init\n\
introspect\n\
list\n\
perms\n\
transcrypt\n\
upgrade\n\
version";

pub const INTROSPECT_CONFIGS: &str = "local.arch\n\
local.class\n\
local.distro\n\
local.distro-family\n\
local.hostname\n\
local.os\n\
local.user\n\
yadm.alt-copy\n\
yadm.auto-alt\n\
yadm.auto-exclude\n\
yadm.auto-perms\n\
yadm.auto-private-dirs\n\
yadm.cipher\n\
yadm.git-program\n\
yadm.gpg-perms\n\
yadm.gpg-program\n\
yadm.gpg-recipient\n\
yadm.openssl-ciphername\n\
yadm.openssl-old\n\
yadm.openssl-program\n\
yadm.ssh-perms";

pub const INTROSPECT_SWITCHES: &str = "--yadm-archive\n\
--yadm-bootstrap\n\
--yadm-config\n\
--yadm-data\n\
--yadm-dir\n\
--yadm-encrypt\n\
--yadm-repo\n\
-Y";

pub fn is_local_option(s: &str) -> bool {
    matches!(
        s,
        "local.class"
            | "local.arch"
            | "local.os"
            | "local.hostname"
            | "local.user"
            | "local.distro"
            | "local.distro-family"
    )
}

/// Internal `$(config ...)` — runs in a "subshell": captures stdout, never
/// exits the process, and never mutates the context.
pub fn config_output(ctx: &Context, args: &[&str]) -> String {
    let use_repo_config = args.iter().any(|a| is_local_option(a));
    let output = if use_repo_config {
        // require_repo inside a subshell only prints; it can't exit the parent
        if !Path::new(&ctx.repo).is_dir() {
            eprintln!("ERROR: Git repo does not exist. did you forget to run 'init' or 'clone'?");
            return String::new();
        }
        Command::new(&ctx.git_program)
            .arg("config")
            .args(args)
            .stderr(Stdio::inherit())
            .output()
    } else {
        paths::assert_parent(&ctx.config_file);
        Command::new(&ctx.git_program)
            .arg("config")
            .arg(format!(
                "--file={}",
                paths::mixed_path(ctx, &ctx.config_file)
            ))
            .args(args)
            .stderr(Stdio::inherit())
            .output()
    };
    match output {
        Ok(o) => util::trim_trailing_newlines(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => String::new(),
    }
}

/// The `config` command (output goes straight to the terminal).
pub fn config_cmd(ctx: &mut Context, args: &[String]) {
    if args.is_empty() {
        // with no parameters, provide some helpful documentation
        println!("yadm supports the following configurations:");
        println!();
        for supported_config in INTROSPECT_CONFIGS.lines() {
            println!("  {supported_config}");
        }
        println!();
        // yadm's echo() sets IFS=' ' before the heredoc read here, so read
        // keeps the trailing newline and the output ends with a blank line.
        println!(
            "Please read the CONFIGURATION section in the man\n\
page for more details about configurations, and\n\
how to adjust them.\n"
        );
        return;
    }

    let use_repo_config = args.iter().any(|a| is_local_option(a));
    if use_repo_config {
        git::require_repo(ctx);
        // operate on the yadm repo's configuration file
        // this is always local to the machine
        let _ = Command::new(&ctx.git_program)
            .arg("config")
            .args(args)
            .status();
        ctx.changes_possible = true;
    } else {
        // make sure parent folder of config file exists
        paths::assert_parent(&ctx.config_file);
        // operate on the yadm configuration file
        let _ = Command::new(&ctx.git_program)
            .arg("config")
            .arg(format!(
                "--file={}",
                paths::mixed_path(ctx, &ctx.config_file)
            ))
            .args(args)
            .status();
    }
}
