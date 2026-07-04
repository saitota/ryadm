//! Hook invocation (pre/post) and the exit-with-post-hook path.

use std::path::Path;
use std::process::Command;

use crate::context::Context;
use crate::git;
use crate::util;

/// Bash helper functions yadm `export -f`s to hooks. ryadm exports them in the
/// modern bash env encoding so bash hooks keep working.
///
/// These are hand-ported duplicates of the Rust implementations in `paths.rs`
/// (`builtin_dirname` / `relative_path` / `unix_path` / `mixed_path`). When
/// changing yadm's path semantics, update both this table and `paths.rs` so
/// hooks and the internal path resolver do not diverge.
const EXPORTED_FUNCTIONS: &[(&str, &str)] = &[
    (
        "builtin_dirname",
        r#"{ local path="$1"; while [ "${path: -1}" = "/" ]; do path="${path%/}"; done; local dir_name="${path%/*}"; while [ "${dir_name: -1}" = "/" ]; do dir_name="${dir_name%/}"; done; if [ "$path" = "$dir_name" ]; then dir_name="."; elif [ -z "$dir_name" ]; then dir_name="/"; fi; echo "$dir_name"; }"#,
    ),
    (
        "relative_path",
        r#"{ local base="$1"; if [ "${base:0:1}" != "/" ]; then base="$PWD/$base"; fi; local full="$2"; if [ "${full:0:1}" != "/" ]; then full="$PWD/$full"; fi; local common_part="$base"; local result=""; while [ "$common_part" != "$full" ]; do if [ "$common_part" = "/" ]; then result="${result:+$result/}"; break; elif [ "${full#"$common_part"/}" != "$full" ]; then common_part="$common_part/"; result="${result:+$result/}"; break; fi; common_part=$(builtin_dirname "$common_part"); result="..${result:+/$result}"; done; echo "$result${full#"$common_part"}"; }"#,
    ),
    (
        "unix_path",
        r#"{ if [ "$USE_CYGPATH" = "1" ]; then cygpath -u "$1"; else echo "$1"; fi; }"#,
    ),
    (
        "mixed_path",
        r#"{ if [ "$USE_CYGPATH" = "1" ]; then cygpath -m "$1"; else echo "$1"; fi; }"#,
    ),
];

pub fn invoke_hook(ctx: &Context, mode: &str, exit_status: Option<i32>) {
    let hook_command = format!("{}/{}_{}", ctx.hooks_dir, mode, ctx.hook_command);

    let runnable = util::is_executable(Path::new(&hook_command))
        || (ctx.operating_system.starts_with("MINGW") && Path::new(&hook_command).is_file());
    if !runnable {
        return;
    }

    util::debug(ctx, &format!("Invoking hook: {hook_command}"));

    // expose some internal data to all hooks; filenames including a newline
    // character (\n) are NOT supported
    let encrypt_include_files = match &ctx.encrypt_include_files {
        Some(v) => v.join("\n"),
        None => "unparsed".to_string(),
    };

    let mut cmd = Command::new(&hook_command);
    cmd.env("YADM_HOOK_COMMAND", &ctx.hook_command)
        .env("YADM_HOOK_DIR", &ctx.dir)
        .env("YADM_HOOK_DATA", &ctx.data)
        .env(
            "YADM_HOOK_EXIT",
            exit_status.map(|c| c.to_string()).unwrap_or_default(),
        )
        .env("YADM_HOOK_FULL_COMMAND", &ctx.full_command)
        .env("YADM_HOOK_REPO", &ctx.repo)
        .env("YADM_HOOK_WORK", &ctx.work)
        .env("YADM_ENCRYPT_INCLUDE_FILES", encrypt_include_files);
    for (name, body) in EXPORTED_FUNCTIONS {
        cmd.env(format!("BASH_FUNC_{name}%%"), format!("() {body}"));
    }

    let hook_status = git::exit_code(cmd.status());

    // failing "pre" hooks will prevent commands from being run
    if mode == "pre" && hook_status != 0 {
        println!("Hook {hook_command} was not successful");
        println!("{} will not be run", ctx.hook_command);
        std::process::exit(hook_status);
    }
}

pub fn exit_with_hook(ctx: &Context, exit_status: i32) -> ! {
    invoke_hook(ctx, "post", Some(exit_status));
    std::process::exit(exit_status);
}
