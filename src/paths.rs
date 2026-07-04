//! Path resolution: global argument overrides, XDG directory resolution,
//! legacy path warning, and the string-based path helpers yadm defines.

use std::path::Path;

use crate::context::{Context, LEGACY_ARCHIVE};
use crate::os;
use crate::util;

pub fn process_global_args(ctx: &mut Context, argv: &[String]) -> Vec<String> {
    let mut main_args: Vec<String> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let take_value = |i: usize| -> String { argv.get(i + 1).cloned().unwrap_or_default() };
        match argv[i].as_str() {
            "-Y" | "--yadm-dir" => {
                let v = qualify_path(ctx, &take_value(i), "yadm");
                ctx.dir = v;
                i += 1;
            }
            "--yadm-data" => {
                let v = qualify_path(ctx, &take_value(i), "data");
                ctx.data = v;
                i += 1;
            }
            "--yadm-repo" => {
                let v = qualify_path(ctx, &take_value(i), "repo");
                ctx.override_repo = v;
                i += 1;
            }
            "--yadm-config" => {
                let v = qualify_path(ctx, &take_value(i), "config");
                ctx.override_config = v;
                i += 1;
            }
            "--yadm-encrypt" => {
                let v = qualify_path(ctx, &take_value(i), "encrypt");
                ctx.override_encrypt = v;
                i += 1;
            }
            "--yadm-archive" => {
                let v = qualify_path(ctx, &take_value(i), "archive");
                ctx.override_archive = v;
                i += 1;
            }
            "--yadm-bootstrap" => {
                let v = qualify_path(ctx, &take_value(i), "bootstrap");
                ctx.override_bootstrap = v;
                i += 1;
            }
            _ => main_args.push(argv[i].clone()),
        }
        i += 1;
    }
    main_args
}

pub fn qualify_path(ctx: &Context, path: &str, what: &str) -> String {
    if path.is_empty() {
        // yadm only calls qualify_path inside $(...) substitutions, so
        // error_out prints but exits just the subshell — the variable is
        // assigned an empty string and execution continues.
        eprintln!("ERROR: You can't specify an empty {what} path");
        return String::new();
    }
    if path == "." {
        ctx.pwd.clone()
    } else if !path.starts_with('/') {
        format!("{}/{}", ctx.pwd, path.strip_prefix("./").unwrap_or(path))
    } else {
        path.to_string()
    }
}

pub fn set_yadm_dirs(ctx: &mut Context, main_args: &[String]) {
    if ctx.data.is_empty() {
        let mut base = std::env::var("XDG_DATA_HOME").unwrap_or_default();
        if !base.starts_with('/') {
            base = format!("{}/.local/share", ctx.home);
        }
        ctx.data = format!("{base}/yadm");
    }
    if ctx.dir.is_empty() {
        let mut base = std::env::var("XDG_CONFIG_HOME").unwrap_or_default();
        if !base.starts_with('/') {
            base = format!("{}/.config", ctx.home);
        }
        ctx.dir = format!("{base}/yadm");
    }
    issue_legacy_path_warning(ctx, main_args);
}

pub fn issue_legacy_path_warning(ctx: &mut Context, main_args: &[String]) {
    // no warnings during upgrade (substring match, like yadm)
    if main_args.join(" ").contains("upgrade") {
        return;
    }
    // no warnings if YADM_DIR is resolved as the legacy path
    if ctx.dir == ctx.legacy_dir {
        return;
    }
    // no warnings if overrides have been provided
    if !ctx.override_repo.is_empty() || !ctx.override_archive.is_empty() || ctx.data == ctx.dir {
        return;
    }

    // ordered by importance; ctx.repo etc. are still the relative defaults here
    let mut candidates: Vec<String> = vec![
        format!("{}/{}", ctx.dir, ctx.repo),
        format!("{}/{}", ctx.dir, LEGACY_ARCHIVE),
        format!("{}/{}", ctx.legacy_dir, ctx.repo),
        format!("{}/{}", ctx.legacy_dir, ctx.bootstrap_file),
        format!("{}/{}", ctx.legacy_dir, ctx.config_file),
        format!("{}/{}", ctx.legacy_dir, ctx.encrypt_file),
    ];
    let hooks_base = format!("{}/{}", ctx.legacy_dir, ctx.hooks_dir);
    for prefix in ["pre_", "post_"] {
        candidates.extend(glob_prefix(&hooks_base, prefix));
    }
    candidates.push(format!("{}/{}", ctx.legacy_dir, LEGACY_ARCHIVE));

    let legacy_found: Vec<String> = candidates
        .into_iter()
        .filter(|p| Path::new(p).exists())
        .collect();
    if legacy_found.is_empty() {
        return;
    }

    let mut path_list = String::new();
    for legacy_path in &legacy_found {
        path_list.push_str(&format!("    * {legacy_path}\n"));
    }

    // raw string: `\n\` continuations would strip the leading indentation
    eprint!(
        r#"
**WARNING**
  Legacy paths have been detected.

  With version 3.0.0, yadm uses the XDG Base Directory Specification
  to find its configurations and data. Read more about these changes here:

    https://yadm.io/docs/upgrade_from_2
    https://yadm.io/docs/upgrade_from_1

  In your environment, the data directory has been resolved to:

    {data}

  To remove this warning do one of the following:
    * Run "yadm upgrade" to move the yadm data to the new paths. (RECOMMENDED)
    * Manually move yadm data to new default paths and reinit any submodules.
    * Specify your preferred paths with --yadm-data and --yadm-archive each execution.

  Legacy paths detected:
{path_list}
***********

"#,
        data = ctx.data,
        path_list = path_list
    );
    ctx.legacy_warning_issued = true;
}

/// Expand a `dir/prefix*` glob: sorted matches, or nothing when none match
/// (the unexpanded literal pattern never passes the -e test in yadm).
fn glob_prefix(dir: &str, prefix: &str) -> Vec<String> {
    let mut matches: Vec<String> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| name.starts_with(prefix) && name.len() > prefix.len())
            .map(|name| format!("{dir}/{name}"))
            .collect(),
        Err(_) => Vec::new(),
    };
    matches.sort();
    matches
}

pub fn configure_paths(ctx: &mut Context) {
    // change paths to be relative to YADM_DIR
    ctx.config_file = format!("{}/{}", ctx.dir, ctx.config_file);
    ctx.encrypt_file = format!("{}/{}", ctx.dir, ctx.encrypt_file);
    ctx.bootstrap_file = format!("{}/{}", ctx.dir, ctx.bootstrap_file);
    ctx.hooks_dir = format!("{}/{}", ctx.dir, ctx.hooks_dir);
    ctx.alt_dir = format!("{}/{}", ctx.dir, ctx.alt_dir);

    // change paths to be relative to YADM_DATA
    ctx.repo = format!("{}/{}", ctx.data, ctx.repo);
    ctx.archive = format!("{}/{}", ctx.data, ctx.archive);

    // independent overrides for paths
    if !ctx.override_repo.is_empty() {
        ctx.repo = ctx.override_repo.clone();
    }
    if !ctx.override_config.is_empty() {
        ctx.config_file = ctx.override_config.clone();
    }
    if !ctx.override_encrypt.is_empty() {
        ctx.encrypt_file = ctx.override_encrypt.clone();
    }
    if !ctx.override_archive.is_empty() {
        ctx.archive = ctx.override_archive.clone();
    }
    if !ctx.override_bootstrap.is_empty() {
        ctx.bootstrap_file = ctx.override_bootstrap.clone();
    }

    // use the yadm repo for all git operations (children inherit this)
    let git_dir = mixed_path(ctx, &ctx.repo);
    std::env::set_var("GIT_DIR", &git_dir);

    // obtain YADM_WORK from repo if it exists
    if Path::new(&git_dir).is_dir() {
        let out = os::capture(&ctx.git_program, &["config", "core.worktree"]);
        let work = unix_path(ctx, &out);
        if !work.is_empty() {
            ctx.work = work;
        }
    }

    // YADM_BASE is used for manipulating the base worktree path for much of
    // the alternate file processing
    ctx.base = if ctx.work == "/" {
        String::new()
    } else {
        ctx.work.clone()
    };
}

/// yadm's builtin_dirname — including its quirks (e.g. dirname of "/" is ".").
pub fn builtin_dirname(path: &str) -> String {
    let mut p = path.to_string();
    while p.ends_with('/') {
        p.pop();
    }
    let mut d = match p.rfind('/') {
        Some(i) => p[..i].to_string(),
        None => p.clone(),
    };
    while d.ends_with('/') {
        d.pop();
    }
    if p == d {
        ".".to_string()
    } else if d.is_empty() {
        "/".to_string()
    } else {
        d
    }
}

/// yadm's relative_path: a path to `full`, relative to `base`.
pub fn relative_path(base_in: &str, full_in: &str) -> String {
    let pwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let base = if !base_in.starts_with('/') {
        format!("{pwd}/{base_in}")
    } else {
        base_in.to_string()
    };
    let full = if !full_in.starts_with('/') {
        format!("{pwd}/{full_in}")
    } else {
        full_in.to_string()
    };

    let mut common_part = base;
    let mut result = String::new();
    loop {
        if common_part == full {
            break;
        }
        if common_part == "/" {
            // No common part found. Append / if result is set to make the
            // final result correct.
            if !result.is_empty() {
                result.push('/');
            }
            break;
        } else if full.starts_with(&format!("{common_part}/")) {
            common_part.push('/');
            if !result.is_empty() {
                result.push('/');
            }
            break;
        }
        common_part = builtin_dirname(&common_part);
        result = if result.is_empty() {
            "..".to_string()
        } else {
            format!("../{result}")
        };
    }

    let suffix = full.strip_prefix(&common_part).unwrap_or(&full);
    format!("{result}{suffix}")
}

/// yadm's assert_parent: ensure the parent directory of `path` exists.
pub fn assert_parent(path: &str) {
    let basedir = match path.rfind('/') {
        Some(i) => &path[..i],
        None => path,
    };
    if !basedir.is_empty() && !Path::new(basedir).exists() {
        let _ = std::fs::create_dir_all(basedir);
    }
}

/// yadm's mk_tmp_dir: create $YADM_DATA/tmp.<pid>.<random> and return it.
pub fn mk_tmp_dir(ctx: &Context) -> String {
    let rand = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
        % 32768;
    let tempdir = format!("{}/tmp.{}.{}", ctx.data, std::process::id(), rand);
    assert_parent(&format!("{tempdir}/"));
    tempdir
}

/// yadm's cd_work: chdir to the work tree (affects the whole process, as in bash).
pub fn cd_work(ctx: &Context, what: &str) -> bool {
    if std::env::set_current_dir(&ctx.work).is_err() {
        util::debug(
            ctx,
            &format!("{} not processed, unable to cd to {}", what, ctx.work),
        );
        return false;
    }
    true
}

/// Path used by bash/yadm itself (cygpath -u on Cygwin; identity elsewhere).
pub fn unix_path(ctx: &Context, path: &str) -> String {
    if ctx.use_cygpath {
        os::capture("cygpath", &["-u", path])
    } else {
        path.to_string()
    }
}

/// Path handed to Git (cygpath -m on Cygwin; identity elsewhere).
pub fn mixed_path(ctx: &Context, path: &str) -> String {
    if ctx.use_cygpath {
        os::capture("cygpath", &["-m", path])
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirname_matches_yadm_quirks() {
        assert_eq!(builtin_dirname("/a/b/c"), "/a/b");
        assert_eq!(builtin_dirname("/a/b/c/"), "/a/b");
        assert_eq!(builtin_dirname("/a"), "/");
        assert_eq!(builtin_dirname("a"), ".");
        assert_eq!(builtin_dirname("a/b"), "a");
        assert_eq!(builtin_dirname("a//b"), "a");
        // yadm's builtin_dirname of "/" collapses to "."
        assert_eq!(builtin_dirname("/"), ".");
        assert_eq!(builtin_dirname(""), ".");
    }
}
