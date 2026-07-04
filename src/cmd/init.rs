//! The init command.

use std::path::Path;

use crate::context::Context;
use crate::git;
use crate::paths;
use crate::util;

pub fn init(ctx: &mut Context, args: &[String]) {
    // safety check, don't attempt to init when the repo is already present
    if Path::new(&ctx.repo).is_dir() && !ctx.force {
        let msg = format!(
            "Git repo already exists. [{}]\\nUse '-f' if you want to force it to be overwritten.",
            ctx.repo
        );
        util::error_out(ctx, &msg);
    }

    // remove existing if forcing the init to happen anyway
    if Path::new(&ctx.repo).is_dir() {
        util::debug(ctx, "Removing existing repo prior to init");
        let _ = git::cmd(ctx)
            .args(["-C", &ctx.work, "submodule", "deinit", "-f", "--all"])
            .status();
        let _ = std::fs::remove_dir_all(&ctx.repo);
    }

    // init a new bare repo
    util::debug(ctx, "Init new repo");
    let repo = paths::mixed_path(ctx, &ctx.repo);
    let mut c = git::cmd(ctx);
    c.args(["init", "--shared=0600", "--bare", &repo]);
    c.args(args);
    let _ = c.status();
    git::configure_repo(ctx);

    ctx.changes_possible = true;
}
