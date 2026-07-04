//! The upgrade command (legacy path migration).
//! Reference: yadm script lines 1368-1473.
//! Note: yadm's upgrade ends with a bare `exit 0` — it never runs the post
//! hook or the automatic events.

use std::path::Path;

use crate::context::{Context, LEGACY_ARCHIVE};
use crate::git;
use crate::paths;
use crate::util;

pub fn upgrade(ctx: &mut Context) -> ! {
    let mut actions_performed = false;
    let mut repo_updates = false;
    let mut submodules: Vec<String> = Vec::new();

    if !ctx.override_repo.is_empty() || !ctx.override_archive.is_empty() || ctx.data == ctx.dir {
        util::error_out(
            ctx,
            "Unable to upgrade. Paths have been overridden with command line options",
        );
    }

    // choose a legacy repo, the version 2 location will be favored
    let mut legacy_repo = String::new();
    let legacy_dir_repo = format!("{}/repo.git", ctx.legacy_dir);
    if Path::new(&legacy_dir_repo).is_dir() {
        legacy_repo = legacy_dir_repo;
    }
    let dir_repo = format!("{}/repo.git", ctx.dir);
    if Path::new(&dir_repo).is_dir() {
        legacy_repo = dir_repo;
    }

    // handle legacy repo
    if !legacy_repo.is_empty() && Path::new(&legacy_repo).is_dir() {
        if Path::new(&ctx.repo).exists() {
            util::error_out(
                ctx,
                &format!(
                    "Unable to upgrade. '{}' already exists. Refusing to overwrite it.",
                    ctx.repo
                ),
            );
        } else {
            actions_performed = true;
            println!("Moving {} to {}", legacy_repo, ctx.repo);

            std::env::set_var("GIT_DIR", &legacy_repo);

            // Must absorb git dirs, otherwise deinit below will fail for
            // modules that have been cloned first and then added as a
            // submodule.
            let _ = git::run(ctx, &["submodule", "absorbgitdirs"]);

            let (submodule_status, _) =
                git::capture(ctx, &["-C", &ctx.work, "submodule", "status"], false);
            if !submodule_status.is_empty() {
                for line in submodule_status.split('\n') {
                    let mut parts = line.trim_start().splitn(3, char::is_whitespace);
                    let sha = parts.next().unwrap_or("");
                    let submodule = parts.next().unwrap_or("");
                    if submodule.is_empty() {
                        continue;
                    }
                    if sha.starts_with('-') {
                        continue;
                    }
                    let mut c = git::cmd(ctx);
                    c.args(["-C", &ctx.work, "submodule", "deinit"]);
                    if ctx.force {
                        c.arg("-f");
                    }
                    c.args(["--", submodule]);
                    let ok = c.status().map(|s| s.success()).unwrap_or(false);
                    if !ok {
                        for other in &submodules {
                            let mut c2 = git::cmd(ctx);
                            c2.args([
                                "-C",
                                &ctx.work,
                                "submodule",
                                "update",
                                "--init",
                                "--recursive",
                                "--",
                                other,
                            ]);
                            let _ = c2.status();
                        }
                        util::error_out(
                            ctx,
                            &format!("Unable to upgrade. Could not deinit submodule {submodule}"),
                        );
                    }
                    submodules.push(submodule.to_string());
                }
            }

            paths::assert_parent(&ctx.repo);
            let _ = std::fs::rename(&legacy_repo, &ctx.repo);
        }
    }
    std::env::set_var("GIT_DIR", &ctx.repo);

    // choose a legacy archive, the version 2 location will be favored
    let mut legacy_archive = String::new();
    let legacy_dir_archive = format!("{}/{}", ctx.legacy_dir, LEGACY_ARCHIVE);
    if Path::new(&legacy_dir_archive).exists() {
        legacy_archive = legacy_dir_archive;
    }
    let dir_archive = format!("{}/{}", ctx.dir, LEGACY_ARCHIVE);
    if Path::new(&dir_archive).exists() {
        legacy_archive = dir_archive;
    }

    // handle legacy archive
    if !legacy_archive.is_empty() && Path::new(&legacy_archive).exists() {
        actions_performed = true;
        println!("Moving {} to {}", legacy_archive, ctx.archive);
        paths::assert_parent(&ctx.archive);
        if is_tracked(ctx, &legacy_archive) {
            if git_mv(ctx, &legacy_archive, &ctx.archive) {
                repo_updates = true;
            }
        } else {
            let _ = std::fs::rename(&legacy_archive, &ctx.archive);
        }
    }

    // handle any remaining version 1 paths
    let mut legacy_paths: Vec<String> = vec![
        format!("{}/config", ctx.legacy_dir),
        format!("{}/encrypt", ctx.legacy_dir),
        format!("{}/bootstrap", ctx.legacy_dir),
    ];
    let hooks_dir = format!("{}/hooks", ctx.legacy_dir);
    for prefix in ["pre_", "post_"] {
        legacy_paths.extend(glob_prefix(&hooks_dir, prefix));
    }

    for legacy_path in &legacy_paths {
        if Path::new(legacy_path).exists() {
            let prefix = format!("{}/", ctx.legacy_dir);
            let rel = legacy_path.strip_prefix(&prefix).unwrap_or(legacy_path);
            let new_filename = format!("{}/{}", ctx.dir, rel);

            actions_performed = true;
            println!("Moving {legacy_path} to {new_filename}");
            paths::assert_parent(&new_filename);
            if is_tracked(ctx, legacy_path) {
                if git_mv(ctx, legacy_path, &new_filename) {
                    repo_updates = true;
                }
            } else {
                let _ = std::fs::rename(legacy_path, &new_filename);
            }
        }
    }

    // handle submodules, which need to be reinitialized
    for submodule in &submodules {
        let mut c = git::cmd(ctx);
        c.args([
            "-C",
            &ctx.work,
            "submodule",
            "update",
            "--init",
            "--recursive",
            "--",
            submodule,
        ]);
        let _ = c.status();
    }

    if !actions_performed {
        println!("No legacy paths found. Upgrade is not necessary");
    }

    if repo_updates {
        println!(
            "Some files tracked by yadm have been renamed. These changes should probably be commited now."
        );
    }

    std::process::exit(0)
}

/// `git ls-files --error-unmatch <path>` with both streams silenced.
fn is_tracked(ctx: &Context, path: &str) -> bool {
    let (_, ok) = git::capture(ctx, &["ls-files", "--error-unmatch", path], true);
    ok
}

/// `git mv <from> <to>`, returns true on success.
fn git_mv(ctx: &Context, from: &str, to: &str) -> bool {
    let mut c = git::cmd(ctx);
    c.args(["mv", from, to]);
    c.status().map(|s| s.success()).unwrap_or(false)
}

/// Expand a `dir/prefix*` glob: sorted matches (bash's own glob sort order).
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // serialize: Context::new reads process-global env
    static LOCK: Mutex<()> = Mutex::new(());

    fn tmp_home() -> String {
        let _guard = LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "radm-upgrade-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn glob_prefix_sorted_and_filters_bare_prefix() {
        let home = tmp_home();
        let hooks = format!("{home}/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        std::fs::write(format!("{hooks}/pre_status"), "status").unwrap();
        std::fs::write(format!("{hooks}/pre_commit"), "commit").unwrap();
        std::fs::write(format!("{hooks}/pre_"), "bare").unwrap(); // no suffix: excluded
        std::fs::write(format!("{hooks}/post_commit"), "commit").unwrap();

        let mut pre = glob_prefix(&hooks, "pre_");
        pre.sort();
        assert_eq!(
            pre,
            vec![format!("{hooks}/pre_commit"), format!("{hooks}/pre_status"),]
        );

        let post = glob_prefix(&hooks, "post_");
        assert_eq!(post, vec![format!("{hooks}/post_commit")]);

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn glob_prefix_missing_dir_is_empty() {
        let home = tmp_home();
        let hooks = format!("{home}/hooks-does-not-exist");
        assert!(glob_prefix(&hooks, "pre_").is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }
}
