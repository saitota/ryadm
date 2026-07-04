//! The clone command.
//! Reference: yadm script lines 765-882. Spec: scratchpad specs repo-cmds.md.

use std::path::Path;

use crate::context::Context;
use crate::git;
use crate::paths;
use crate::privdirs;
use crate::util;

pub fn clone(ctx: &mut Context, args: &[String]) {
    ctx.do_bootstrap = 1;
    let mut clone_args: Vec<String> = Vec::new();
    let mut do_checkout = true;
    let mut submodules: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--bootstrap" => ctx.do_bootstrap = 2,
            "--no-bootstrap" => ctx.do_bootstrap = 3,
            "--checkout" => do_checkout = true,
            "-n" | "--no-checkout" => do_checkout = false,
            "--recursive" | "--recurse-submodules" => submodules.push(":/".to_string()),
            _ if a.starts_with("--recurse-submodules=") => {
                let suffix = a.strip_prefix("--recurse-submodules=").unwrap_or("");
                submodules.push(format!(":/{suffix}"));
            }
            "--bare" | "--mirror" => {}
            _ if a.starts_with("--separate-git-dir=") => {}
            "--separate-git-dir" => {
                // ignore this arg, and also consume (skip) the following one
                i += 1;
            }
            _ => clone_args.push(a.clone()),
        }
        i += 1;
    }

    if ctx.debug {
        privdirs::display_private_perms(ctx, "initial");
    }

    // safety check, don't attempt to clone when the repo is already present
    if Path::new(&ctx.repo).is_dir() && !ctx.force {
        let msg = format!(
            "Git repo already exists. [{}]\\nUse '-f' if you want to force it to be overwritten.",
            ctx.repo
        );
        util::error_out(ctx, &msg);
    }

    // remove existing if forcing the clone to happen anyway
    if Path::new(&ctx.repo).is_dir() {
        util::debug(ctx, "Removing existing repo prior to clone");
        let _ = git::cmd(ctx)
            .args(["-C", &ctx.work, "submodule", "deinit", "-f", "--all"])
            .status();
        let _ = std::fs::remove_dir_all(&ctx.repo);
    }

    let wc = paths::mk_tmp_dir(ctx);
    if !Path::new(&wc).is_dir() {
        util::error_out(ctx, "Unable to create temporary directory");
    }

    // first clone without checkout
    util::debug(ctx, "Doing an initial clone of the repository");
    let mut c = git::cmd(ctx);
    c.current_dir(&wc);
    c.args(["-c", "core.sharedrepository=0600", "clone", "--no-checkout"]);
    c.arg(format!("--separate-git-dir={}", ctx.repo));
    c.args(&clone_args);
    c.arg("repo.git");
    let clone_ok = c.status().map(|s| s.success()).unwrap_or(false);
    if !clone_ok {
        util::debug(ctx, "Removing repo after failed clone");
        let _ = std::fs::remove_dir_all(&ctx.repo);
        let _ = std::fs::remove_dir_all(&wc);
        util::error_out(ctx, "Unable to clone the repository");
    }
    git::configure_repo(ctx);
    let _ = std::fs::remove_dir_all(&wc);

    // then reset the index as the --no-checkout flag makes the index empty
    let _ = git::run(ctx, &["reset", "--quiet", "--", ":/"]);

    if ctx.work == ctx.home {
        util::debug(ctx, "Determining if repo tracks private directories");
        for private_dir in privdirs::private_dirs_all(ctx) {
            let (found_log, _) = git::capture(ctx, &["log", "-n", "1", "--", &private_dir], true);
            if !found_log.is_empty() {
                util::debug(
                    ctx,
                    &format!("Private directory {private_dir} is tracked by repo"),
                );
                privdirs::assert_private_dirs(ctx, &[private_dir]);
            }
        }
    }

    // finally check out (unless instructed not to) all files that don't exist in YADM_WORK
    if do_checkout {
        if ctx.debug {
            privdirs::display_private_perms(ctx, "pre-checkout");
        }

        if !paths::cd_work(ctx, "Clone") {
            return;
        }

        let (deleted, _) = git::capture(ctx, &["ls-files", "--deleted"], false);
        if !deleted.is_empty() {
            for file in deleted.split('\n') {
                let spec = format!(":/{file}");
                let _ = git::run(ctx, &["checkout", "--", &spec]);
            }
        }

        if !submodules.is_empty() {
            let mut c = git::cmd(ctx);
            c.args(["submodule", "update", "--init", "--recursive", "--"]);
            c.args(&submodules);
            let _ = c.status();
        }

        let (modified, _) = git::capture(ctx, &["ls-files", "--modified"], false);
        if !modified.is_empty() {
            // raw string: `\n\` continuations would strip the indentation
            print!(
                r#"**NOTE**
  Local files with content that differs from the ones just
  cloned were found in {work}. They have been left
  unmodified.

  Please review and resolve any differences appropriately.
  If you know what you're doing, and want to overwrite the
  tracked files, consider 'yadm checkout "{work}"'.

"#,
                work = ctx.work
            );
        }

        if ctx.debug {
            privdirs::display_private_perms(ctx, "post-checkout");
        }

        ctx.changes_possible = true;
    }
}
