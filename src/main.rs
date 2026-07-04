//! ryadm — Rust (Yet) Another Dotfiles Manager.
//!
//! A byte-compatible reimplementation of yadm 3.5.0. Control flow mirrors
//! the original script: global args → OS detection → path resolution →
//! command dispatch → automatic events → exit through the post hook.

mod alt;
mod cmd;
mod config;
mod context;
mod encrypt;
mod exclude;
mod git;
mod hooks;
mod os;
mod paths;
mod privdirs;
mod template;
mod template_default;
mod util;

use std::path::Path;

use context::Context;

const INTERNAL_COMMANDS: &[&str] = &[
    "alt",
    "bootstrap",
    "clean",
    "clone",
    "config",
    "decrypt",
    "encrypt",
    "enter",
    "git-crypt",
    "help",
    "--help",
    "init",
    "introspect",
    "list",
    "perms",
    "transcrypt",
    "upgrade",
    "version",
    "--version",
];

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut ctx = Context::new();
    let main_args = paths::process_global_args(&mut ctx, &argv);
    os::set_operating_system(&mut ctx);
    os::set_awk(&mut ctx);
    paths::set_yadm_dirs(&mut ctx, &main_args);
    paths::configure_paths(&mut ctx);
    run_main(&mut ctx, &main_args);
}

fn run_main(ctx: &mut Context, args: &[String]) -> ! {
    git::require_git(ctx);

    // capture full command, for passing to hooks: space delimited with
    // backslashes, tabs and spaces escaped
    let full: Vec<String> = args
        .iter()
        .map(|p| {
            p.replace('\\', "\\\\")
                .replace('\t', "\\\t")
                .replace(' ', "\\ ")
        })
        .collect();
    ctx.full_command = full.join(" ");

    // create the YADM_DIR & YADM_DATA if they don't exist yet
    if !Path::new(&ctx.dir).is_dir() {
        let _ = std::fs::create_dir_all(&ctx.dir);
    }
    if !Path::new(&ctx.data).is_dir() {
        let _ = std::fs::create_dir_all(&ctx.data);
    }

    let mut retval = 0;
    if args.is_empty() {
        // no arguments will result in help()
        cmd::misc::help(ctx);
    } else if INTERNAL_COMMANDS.contains(&args[0].as_str()) {
        // for internal commands, process all of the arguments
        let mut name = args[0].replace('-', "_");
        if let Some(stripped) = name.strip_prefix("__") {
            name = stripped.to_string();
        }
        ctx.yadm_command = name.clone();

        let rest = &args[1..];
        let mut yadm_args: Vec<String> = Vec::new();
        // enter and git-crypt do not process any of the parameters
        if name == "enter" || name == "git_crypt" {
            yadm_args = rest.to_vec();
        } else {
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "-a" => ctx.list_all = true,
                    "-d" => ctx.debug = true,
                    "-f" => ctx.force = true,
                    "-l" => {
                        ctx.do_list = true;
                        if name == "clone" || name == "config" {
                            yadm_args.push("-l".to_string());
                        }
                    }
                    "-w" => {
                        let value = rest.get(i + 1).cloned().unwrap_or_default();
                        let qualified = paths::qualify_path(ctx, &value, "work tree");
                        ctx.work = qualified;
                        i += 1;
                    }
                    _ => yadm_args.push(rest[i].clone()),
                }
                i += 1;
            }
        }

        if !Path::new(&ctx.work).is_dir() {
            let msg = format!("Work tree does not exist: [{}]", ctx.work);
            util::error_out(ctx, &msg);
        }
        ctx.hook_command = name.clone();
        hooks::invoke_hook(ctx, "pre", None);
        dispatch(ctx, &name, &yadm_args);
    } else {
        // any other commands are simply passed through to git
        ctx.hook_command = args[0].clone();
        hooks::invoke_hook(ctx, "pre", None);
        retval = git::git_command(ctx, args);
    }

    // process automatic events
    auto_alt(ctx);
    auto_perms(ctx);
    auto_bootstrap(ctx);

    hooks::exit_with_hook(ctx, retval)
}

fn dispatch(ctx: &mut Context, name: &str, args: &[String]) {
    match name {
        "alt" => alt::alt(ctx),
        "bootstrap" => cmd::misc::bootstrap(ctx),
        "clean" => cmd::misc::clean(ctx),
        "clone" => cmd::clone::clone(ctx, args),
        "config" => config::config_cmd(ctx, args),
        "decrypt" => encrypt::decrypt(ctx),
        "encrypt" => encrypt::encrypt(ctx),
        "enter" => cmd::enter::enter(ctx, args),
        "git_crypt" => cmd::enter::git_crypt(ctx, args),
        "help" => cmd::misc::help(ctx),
        "init" => cmd::init::init(ctx, args),
        "introspect" => cmd::misc::introspect(ctx, args),
        "list" => cmd::misc::list(ctx),
        "perms" => cmd::misc::perms(ctx),
        "transcrypt" => cmd::enter::transcrypt(ctx, args),
        "upgrade" => cmd::upgrade::upgrade(ctx),
        "version" => cmd::misc::version(ctx),
        _ => unreachable!("unknown internal command {name}"),
    }
}

fn auto_alt(ctx: &mut Context) {
    // process alternates if there are possible changes
    if ctx.changes_possible {
        let auto_alt = config::config_output(ctx, &["--bool", "yadm.auto-alt"]);
        if auto_alt != "false" && Path::new(&ctx.repo).is_dir() {
            alt::alt(ctx);
        }
    }
}

fn auto_perms(ctx: &mut Context) {
    // process permissions if there are possible changes
    if ctx.changes_possible {
        let auto_perms = config::config_output(ctx, &["--bool", "yadm.auto-perms"]);
        if auto_perms != "false" && Path::new(&ctx.repo).is_dir() {
            cmd::misc::perms(ctx);
        }
    }
}

fn auto_bootstrap(ctx: &mut Context) {
    if !cmd::misc::bootstrap_available(ctx) {
        return;
    }
    match ctx.do_bootstrap {
        0 | 3 => (),
        2 => cmd::misc::bootstrap(ctx),
        1 => {
            println!("Found {}", ctx.bootstrap_file);
            println!("It appears that a bootstrap program exists.");
            println!("Would you like to execute it now? (y/n)");
            let answer = util::read_tty_line().unwrap_or_default();
            if answer == "y" || answer == "Y" {
                cmd::misc::bootstrap(ctx);
            }
        }
        _ => (),
    }
}
