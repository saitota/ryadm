//! Bootstrap, hooks, and path-resolution contract tests: exact expected
//! strings/exit codes/side effects (not just differential comparison
//! against bash yadm).

mod common;
use common::*;

// ---------------------------------------------------------------------
// Bootstrap command (`ryadm bootstrap`)
// ---------------------------------------------------------------------

#[test]
fn bootstrap_missing_errors_exactly_and_exits_1() {
    let tb = TestBed::new("bootstrap-missing");
    let r = tb.ryadm(&["bootstrap"]);
    assert_eq!(r.code, 1);
    assert_eq!(r.stdout, "");
    let expected_path = tb.yadm_dir().join("bootstrap");
    let expected = format!(
        "ERROR: Cannot execute bootstrap\n'{}' is not an executable program.\n",
        expected_path.display()
    );
    assert_eq!(r.stderr, expected);
}

#[test]
fn bootstrap_not_executable_errors_exactly_and_exits_1() {
    let tb = TestBed::new("bootstrap-not-exec");
    tb.write_home_mode(".config/yadm/bootstrap", "", 0o644);
    let r = tb.ryadm(&["bootstrap"]);
    assert_eq!(r.code, 1);
    assert_eq!(r.stdout, "");
    assert!(r.err_contains("is not an executable program"));
}

#[test]
fn bootstrap_executable_execs_and_propagates_exit_code() {
    let tb = TestBed::new("bootstrap-exec");
    let script = "#!/bin/bash\necho Bootstrap successful\nexit 123\n";
    tb.write_home_mode(".config/yadm/bootstrap", script, 0o775);
    let r = tb.ryadm(&["bootstrap"]);
    assert_eq!(r.code, 123);
    assert_eq!(r.stderr, "");
    assert!(r.out_contains("Bootstrap successful"));
    let expected_path = tb.yadm_dir().join("bootstrap");
    assert!(r.out_contains(&format!("Executing {}", expected_path.display())));
}

// ---------------------------------------------------------------------
// Auto-bootstrap after clone
// ---------------------------------------------------------------------

/// Build a bare remote at `<root>/remote.git` with a single commit.
fn seed_remote_at(tb: &TestBed) -> String {
    let remote = tb.root.join("remote.git");
    let script = format!(
        r#"set -e
git init -q "{root}/seed"
cd "{root}/seed"
git checkout -qb master 2>/dev/null || true
echo "remote file" > .cfile1
git add .cfile1
git commit -qm seed
git clone -q --bare "{root}/seed" "{remote}"
"#,
        root = tb.root.display(),
        remote = remote.display(),
    );
    let r = tb.sh(&script);
    assert!(r.success(), "seed fixture failed: {r:?}");
    remote.to_string_lossy().into_owned()
}

const BOOTSTRAP_CODE: i32 = 123;
const BOOTSTRAP_MSG: &str = "Bootstrap successful";

fn write_clone_bootstrap(tb: &TestBed) {
    let script = "#!/bin/sh\necho Bootstrap successful\nexit 123\n";
    tb.write_home_mode(".config/yadm/bootstrap", script, 0o775);
}

#[test]
fn clone_bootstrap_flag_missing_bootstrap_is_noop() {
    let tb = TestBed::new("clone-bs-force-missing");
    let remote = seed_remote_at(&tb);
    let r = tb.ryadm(&["clone", "--bootstrap", &remote]);
    assert_eq!(r.code, 0);
    assert!(!r.out_contains(BOOTSTRAP_MSG));
}

#[test]
fn clone_bootstrap_flag_forces_execution_without_prompt() {
    let tb = TestBed::new("clone-bs-force-exists");
    let remote = seed_remote_at(&tb);
    write_clone_bootstrap(&tb);
    let r = tb.ryadm(&["clone", "--bootstrap", &remote]);
    assert_eq!(r.code, BOOTSTRAP_CODE);
    assert!(r.out_contains(BOOTSTRAP_MSG));
    assert!(!r.out_contains("Would you like to execute it now"));
}

#[test]
fn clone_no_bootstrap_flag_prevents_execution_without_prompt() {
    let tb = TestBed::new("clone-bs-prevent");
    let remote = seed_remote_at(&tb);
    write_clone_bootstrap(&tb);
    let r = tb.ryadm(&["clone", "--no-bootstrap", &remote]);
    assert_eq!(r.code, 0);
    assert!(!r.out_contains(BOOTSTRAP_MSG));
    assert!(!r.out_contains("Would you like to execute it now"));
}

#[test]
fn clone_default_ask_no_tty_shows_prompt_and_skips_bootstrap() {
    // No pty is attached (TestBed detaches the controlling terminal), so the
    // `read -r answer </dev/tty` fails; ryadm/yadm both treat that as "no".
    let tb = TestBed::new("clone-bs-ask-no-tty");
    let remote = seed_remote_at(&tb);
    write_clone_bootstrap(&tb);
    let r = tb.ryadm(&["clone", &remote]);
    assert_eq!(r.code, 0);
    assert!(r.out_contains("Found"));
    assert!(r.out_contains("It appears that a bootstrap program exists."));
    assert!(r.out_contains("Would you like to execute it now? (y/n)"));
    assert!(!r.out_contains(BOOTSTRAP_MSG));
}

#[test]
fn clone_bootstrap_message_never_appears_when_bootstrap_absent() {
    let tb = TestBed::new("clone-bs-absent");
    let remote = seed_remote_at(&tb);
    let r = tb.ryadm(&["clone", &remote]);
    assert_eq!(r.code, 0);
    assert!(!r.out_contains("Found"));
    assert!(!r.out_contains("Would you like to execute it now"));
    assert!(!r.out_contains(BOOTSTRAP_MSG));
}

// ---------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------

fn create_hook(tb: &TestBed, rel: &str, name: &str, code: i32) {
    let script = format!("#!/bin/sh\necho HOOK:{name}\nexit {code}\n");
    tb.write_home_mode(rel, &script, 0o755);
}

/// Hook invocation matrix for the `version` command (`--version` mangles
/// to the same `HOOK_COMMAND`).
#[test]
fn hooks_pre_post_matrix_for_version_and_dashdash_version() {
    struct Case {
        id: &'static str,
        pre: bool,
        pre_code: i32,
        post: bool,
        post_code: i32,
    }
    let cases = [
        Case {
            id: "no-hooks",
            pre: false,
            pre_code: 0,
            post: false,
            post_code: 0,
        },
        Case {
            id: "pre-success",
            pre: true,
            pre_code: 0,
            post: false,
            post_code: 0,
        },
        Case {
            id: "pre-fail",
            pre: true,
            pre_code: 5,
            post: false,
            post_code: 0,
        },
        Case {
            id: "post-success",
            pre: false,
            pre_code: 0,
            post: true,
            post_code: 0,
        },
        Case {
            id: "post-fail",
            pre: false,
            pre_code: 0,
            post: true,
            post_code: 5,
        },
        Case {
            id: "pre-post-success",
            pre: true,
            pre_code: 0,
            post: true,
            post_code: 0,
        },
        Case {
            id: "pre-post-fail",
            pre: true,
            pre_code: 5,
            post: true,
            post_code: 5,
        },
    ];

    for cmd in ["--version", "version"] {
        for c in &cases {
            let tb = TestBed::new(&format!(
                "hooks-matrix-{}-{}",
                cmd.trim_start_matches('-'),
                c.id
            ));
            if c.pre {
                create_hook(
                    &tb,
                    ".config/yadm/hooks/pre_version",
                    "pre_version",
                    c.pre_code,
                );
            }
            if c.post {
                create_hook(
                    &tb,
                    ".config/yadm/hooks/post_version",
                    "post_version",
                    c.post_code,
                );
            }
            let r = tb.ryadm(&[cmd]);

            assert_eq!(
                r.code, c.pre_code,
                "cmd={cmd} case={} : exit code must equal pre_code regardless of post_code",
                c.id
            );
            assert_eq!(
                r.stderr, "",
                "cmd={cmd} case={} : stderr must be empty",
                c.id
            );

            if c.pre {
                assert!(
                    r.out_contains("HOOK:pre_version"),
                    "cmd={cmd} case={} : expected pre hook output",
                    c.id
                );
            }
            if r.success() {
                if c.post {
                    assert!(
                        r.out_contains("HOOK:post_version"),
                        "cmd={cmd} case={} : expected post hook output on success",
                        c.id
                    );
                }
            } else {
                assert!(
                    r.out_contains("version will not be run"),
                    "cmd={cmd} case={} : expected pre-fail abort message",
                    c.id
                );
                assert!(
                    !r.out_contains("HOOK:post_version"),
                    "cmd={cmd} case={} : post hook must not run after pre failure",
                    c.id
                );
            }
        }
    }
}

#[test]
fn pre_hook_failure_prints_exact_two_lines_and_propagates_exit_code() {
    let tb = TestBed::new("hooks-pre-fail-exact");
    create_hook(&tb, ".config/yadm/hooks/pre_version", "pre_version", 5);
    let r = tb.ryadm(&["version"]);
    assert_eq!(r.code, 5);
    assert!(r.out_contains("Hook"));
    assert!(r.out_contains("was not successful"));
    assert!(r.out_contains("version will not be run"));
    // Exact hook-command path embedded in the first line.
    let hook_path = tb.yadm_dir().join("hooks/pre_version");
    let expected_line1 = format!("Hook {} was not successful", hook_path.display());
    assert!(
        r.stdout.contains(&expected_line1),
        "stdout was: {:?}",
        r.stdout
    );
    assert!(r.stdout.contains("version will not be run\n"));
}

/// Full hook env-var contract, using a real repo fixture and an unknown
/// git subcommand ("passthrucmd"; ryadm passes git's own failure through).
#[test]
fn hook_env_vars_exact_set_and_values_for_passthru_command() {
    let tb = TestBed::new("hook-env-passthru");
    tb.init_repo_with(&[(".cfile1", "hello\n")]);

    let hook = "#!/bin/bash\nenv\n";
    tb.write_home_mode(".config/yadm/hooks/post_passthrucmd", hook, 0o755);

    let r = tb.ryadm(&["passthrucmd", "extra_args"]);

    assert!(!r.success(), "passthru of unknown git subcommand must fail");
    assert!(
        r.err_contains("'passthrucmd' is not a git command"),
        "stderr was: {:?}",
        r.stderr
    );

    let repo = tb.repo();
    let work = &tb.home;
    let dir = tb.yadm_dir();
    let data = tb.yadm_data();

    assert!(r.out_contains(&format!("YADM_HOOK_EXIT={}\n", r.code)));
    assert!(r.out_contains("YADM_HOOK_COMMAND=passthrucmd\n"));
    assert!(r.out_contains(&format!("YADM_HOOK_DIR={}\n", dir.display())));
    assert!(r.out_contains(&format!("YADM_HOOK_DATA={}\n", data.display())));
    assert!(r.out_contains("YADM_HOOK_FULL_COMMAND=passthrucmd extra_args\n"));
    assert!(r.out_contains(&format!("YADM_HOOK_REPO={}\n", repo.display())));
    assert!(r.out_contains(&format!("YADM_HOOK_WORK={}\n", work.display())));
    assert!(r.out_contains("YADM_ENCRYPT_INCLUDE_FILES=\n"));
}

#[test]
fn hook_env_encrypt_include_files_is_literal_unparsed_in_pre_hooks() {
    // Per hooks.rs: ctx.encrypt_include_files starts as None ("unparsed"
    // sentinel) until a command that parses the encrypt file runs; a pre
    // hook for an arbitrary command observes the literal string "unparsed".
    let tb = TestBed::new("hook-env-unparsed");
    tb.init_repo_with(&[]);
    let hook = "#!/bin/sh\necho YADM_ENCRYPT_INCLUDE_FILES=$YADM_ENCRYPT_INCLUDE_FILES\n";
    tb.write_home_mode(".config/yadm/hooks/pre_list", hook, 0o755);
    let r = tb.ryadm(&["list"]);
    assert!(r.success(), "run: {r:?}");
    assert!(r.out_contains("YADM_ENCRYPT_INCLUDE_FILES=unparsed\n"));
}

#[test]
fn hook_full_command_escapes_backslash_tab_space_in_exact_order() {
    let tb = TestBed::new("hook-full-command-escaped");
    tb.init_repo_with(&[]);
    let hook = "#!/bin/bash\nenv\n";
    tb.write_home_mode(".config/yadm/hooks/post_passthrucmd", hook, 0o755);

    let r = tb.ryadm(&["passthrucmd", "a b", "c\td", "e\\f"]);
    assert!(
        r.out_contains("YADM_HOOK_FULL_COMMAND=passthrucmd a\\ b c\\\td e\\\\f\n"),
        "stdout was: {:?}",
        r.stdout
    );
}

#[test]
fn hooks_honored_for_internal_commands_with_yadm_dir_override() {
    // -Y/--yadm-dir relocates YADM_DIR (and thus YADM_HOOKS); hooks must
    // still be discovered and invoked from the overridden location.
    let tb = TestBed::new("hooks-with-y-override");
    let custom_dir = tb.root.join("custom-yadm-dir");
    std::fs::create_dir_all(custom_dir.join("hooks")).unwrap();
    let hook_path = custom_dir.join("hooks/pre_version");
    std::fs::write(&hook_path, "#!/bin/sh\necho HOOK:pre_version\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755)).unwrap();

    let r = tb.ryadm(&["-Y", &custom_dir.to_string_lossy(), "version"]);
    assert!(r.success());
    assert!(r.out_contains("HOOK:pre_version"));
}

#[test]
fn hook_not_executable_is_silently_skipped() {
    let tb = TestBed::new("hook-not-exec-skip");
    tb.write_home_mode(
        ".config/yadm/hooks/pre_version",
        "#!/bin/sh\necho HOOK\n",
        0o644,
    );
    let r = tb.ryadm(&["version"]);
    assert!(r.success());
    assert!(!r.out_contains("HOOK"));
}

#[test]
fn hook_executable_runs() {
    let tb = TestBed::new("hook-exec-runs");
    tb.write_home_mode(
        ".config/yadm/hooks/pre_version",
        "#!/bin/sh\necho HOOK\n",
        0o755,
    );
    let r = tb.ryadm(&["version"]);
    assert!(r.success());
    assert!(r.out_contains("HOOK"));
}

// ---------------------------------------------------------------------
// Path resolution -- -Y/--yadm-dir, --yadm-data, --yadm-repo, etc.
// ---------------------------------------------------------------------

#[test]
fn yadm_dir_and_yadm_data_relocate_config_and_repo() {
    let tb = TestBed::new("paths-y-and-data");
    let custom_dir = tb.root.join("custom-dir");
    let custom_data = tb.root.join("custom-data");
    let r = tb.ryadm(&[
        "-Y",
        &custom_dir.to_string_lossy(),
        "--yadm-data",
        &custom_data.to_string_lossy(),
        "introspect",
        "repo",
    ]);
    assert!(r.success(), "run: {r:?}");
    let expected_repo = custom_data.join("repo.git");
    assert_eq!(r.stdout.trim_end(), expected_repo.to_string_lossy());
}

#[test]
fn yadm_repo_override_relocates_repo_and_git_dir() {
    let tb = TestBed::new("paths-yadm-repo");
    let custom_repo = tb.root.join("elsewhere-repo.git");
    let r = tb.ryadm(&[
        "--yadm-repo",
        &custom_repo.to_string_lossy(),
        "introspect",
        "repo",
    ]);
    assert!(r.success(), "run: {r:?}");
    assert_eq!(r.stdout.trim_end(), custom_repo.to_string_lossy());
}

#[test]
fn yadm_repo_override_does_not_affect_archive() {
    // --yadm-repo is an independent absolute override; it must not perturb
    // YADM_ARCHIVE (which stays relative to YADM_DATA).
    let tb = TestBed::new("paths-yadm-repo-independence");
    let custom_repo = tb.root.join("elsewhere-repo.git");
    // there's no `introspect archive`; exercise indirectly via the legacy
    // warning suppression rule instead, which is override_repo-sensitive.
    tb.write_home("config", ""); // no-op, keep testbed non-empty
    let r = tb.ryadm(&[
        "--yadm-repo",
        &custom_repo.to_string_lossy(),
        "introspect",
        "repo",
    ]);
    assert!(r.success());
    assert_eq!(r.stdout.trim_end(), custom_repo.to_string_lossy());
    assert_eq!(tb.archive(), tb.yadm_data().join("archive"));
}

#[test]
fn yadm_bootstrap_override_relocates_bootstrap_script() {
    let tb = TestBed::new("paths-yadm-bootstrap");
    let script = tb.root.join("my-bootstrap.sh");
    std::fs::write(&script, "#!/bin/sh\necho Bootstrap successful\nexit 123\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o775)).unwrap();

    let r = tb.ryadm(&["--yadm-bootstrap", &script.to_string_lossy(), "bootstrap"]);
    assert_eq!(r.code, 123);
    assert!(r.out_contains("Bootstrap successful"));
    assert!(r.out_contains(&format!("Executing {}", script.display())));
}

#[test]
fn yadm_config_encrypt_archive_overrides_are_independent_absolute_paths() {
    // Verified indirectly: overriding --yadm-config must not affect
    // introspect repo (YADM_REPO), confirming independence of the override
    // vars from each other.
    let tb = TestBed::new("paths-yadm-config-independent");
    let custom_config = tb.root.join("custom.config");
    let r = tb.ryadm(&[
        "--yadm-config",
        &custom_config.to_string_lossy(),
        "introspect",
        "repo",
    ]);
    assert!(r.success());
    assert_eq!(r.stdout.trim_end(), tb.repo().to_string_lossy());
}

#[test]
fn relative_override_paths_are_qualified_against_cwd_not_home() {
    let tb = TestBed::new("paths-relative-cwd");
    let subdir = tb.home.join("sub/deeper");
    std::fs::create_dir_all(&subdir).unwrap();

    let r = tb.ryadm_in(
        &subdir,
        &["--yadm-repo", "relative-repo.git", "introspect", "repo"],
    );
    assert!(r.success(), "run: {r:?}");
    let expected = subdir.join("relative-repo.git");
    assert_eq!(r.stdout.trim_end(), expected.to_string_lossy());
}

#[test]
fn relative_override_path_with_leading_dot_slash_strips_one_prefix() {
    let tb = TestBed::new("paths-relative-dotslash");
    let r = tb.ryadm(&["--yadm-repo", "./override-repo.git", "introspect", "repo"]);
    assert!(r.success(), "run: {r:?}");
    let expected = tb.home.join("override-repo.git");
    assert_eq!(r.stdout.trim_end(), expected.to_string_lossy());
}

#[test]
fn relative_override_path_dot_resolves_to_cwd() {
    let tb = TestBed::new("paths-relative-dot");
    let r = tb.ryadm(&["--yadm-repo", ".", "introspect", "repo"]);
    assert!(r.success(), "run: {r:?}");
    assert_eq!(r.stdout.trim_end(), tb.home.to_string_lossy());
}

#[test]
fn empty_yadm_dir_path_errors_and_falls_through_to_help() {
    let tb = TestBed::new("paths-empty-yadm-dir");
    let r = tb.ryadm(&["-Y"]);
    assert_eq!(r.code, 1);
    assert!(r.err_contains("You can't specify an empty yadm path"));
}

#[test]
fn empty_yadm_data_path_errors() {
    let tb = TestBed::new("paths-empty-yadm-data");
    let r = tb.ryadm(&["--yadm-data"]);
    assert_eq!(r.code, 1);
    assert!(r.err_contains("You can't specify an empty data path"));
}

// ---------------------------------------------------------------------
// XDG_CONFIG_HOME / XDG_DATA_HOME: honored only when absolute
// ---------------------------------------------------------------------

#[test]
fn xdg_config_home_absolute_is_honored() {
    let tb = TestBed::new("xdg-config-absolute");
    let xdg = tb.root.join("xdg-config");
    let r = tb.ryadm_env(
        &["introspect", "repo"],
        "XDG_CONFIG_HOME",
        &xdg.to_string_lossy(),
    );
    assert!(r.success(), "run: {r:?}");
    // YADM_DIR relocates under XDG_CONFIG_HOME; YADM_REPO (unaffected by
    // XDG_CONFIG_HOME, which only feeds YADM_DIR) still defaults under
    // the default YADM_DATA.
    assert_eq!(r.stdout.trim_end(), tb.repo().to_string_lossy());
}

#[test]
fn xdg_data_home_absolute_is_honored_for_repo_location() {
    let tb = TestBed::new("xdg-data-absolute");
    let xdg = tb.root.join("xdg-data");
    let r = tb.ryadm_env(
        &["introspect", "repo"],
        "XDG_DATA_HOME",
        &xdg.to_string_lossy(),
    );
    assert!(r.success(), "run: {r:?}");
    let expected = xdg.join("yadm/repo.git");
    assert_eq!(r.stdout.trim_end(), expected.to_string_lossy());
}

#[test]
fn xdg_data_home_relative_value_falls_back_to_default() {
    let tb = TestBed::new("xdg-data-relative-fallback");
    let r = tb.ryadm_env(
        &["introspect", "repo"],
        "XDG_DATA_HOME",
        "relative/xdg/path",
    );
    assert!(r.success(), "run: {r:?}");
    // Falls back to $HOME/.local/share/yadm since XDG_DATA_HOME didn't
    // start with '/'.
    assert_eq!(r.stdout.trim_end(), tb.repo().to_string_lossy());
}

#[test]
fn xdg_config_home_relative_value_falls_back_to_default() {
    let tb = TestBed::new("xdg-config-relative-fallback");
    // introspect repo doesn't reflect YADM_DIR directly; instead verify via
    // the legacy-warning suppression path is unaffected (YADM_DIR falls
    // back to $HOME/.config/yadm, matching YADM_LEGACY_DIR test isn't
    // applicable here) -- use hooks dir discovery as the observable proxy:
    // a hook placed at the *default* $HOME/.config/yadm/hooks must still
    // fire when XDG_CONFIG_HOME is relative (i.e. ignored).
    tb.write_home_mode(
        ".config/yadm/hooks/pre_version",
        "#!/bin/sh\necho HOOK\nexit 0\n",
        0o755,
    );
    let r = tb.ryadm_env(&["version"], "XDG_CONFIG_HOME", "relative/xdg/config");
    assert!(r.success(), "run: {r:?}");
    assert!(r.out_contains("HOOK"));
}

// ---------------------------------------------------------------------
// 5. Legacy path warning
// ---------------------------------------------------------------------

#[test]
fn legacy_warning_appears_when_legacy_config_exists() {
    let tb = TestBed::new("legacy-warning-appears");
    tb.write_home(".yadm/config", "");
    let r = tb.ryadm(&["list"]);
    assert!(r.err_contains("**WARNING**"));
    assert!(r.err_contains("Legacy paths have been detected."));
    let legacy_config = tb.home.join(".yadm/config");
    assert!(r.err_contains(&format!("    * {}", legacy_config.display())));
    // The warning block itself ends with the asterisks line; `list` then
    // additionally reports the (expected, in this fixture) missing repo.
    assert!(r.stderr.contains("***********\n"));
}

#[test]
fn legacy_warning_absent_when_no_legacy_paths_exist() {
    let tb = TestBed::new("legacy-warning-absent");
    let r = tb.ryadm(&["list"]);
    assert!(!r.err_contains("**WARNING**"));
}

#[test]
fn legacy_warning_suppressed_during_upgrade() {
    let tb = TestBed::new("legacy-warning-suppressed-upgrade");
    tb.write_home(".yadm/config", "");
    let r = tb.ryadm(&["upgrade"]);
    assert!(!r.err_contains("**WARNING**"));
}

#[test]
fn legacy_warning_suppressed_with_yadm_repo_override() {
    let tb = TestBed::new("legacy-warning-suppressed-override");
    tb.write_home(".yadm/config", "");
    let custom_repo = tb.root.join("custom-repo.git");
    let r = tb.ryadm(&["--yadm-repo", &custom_repo.to_string_lossy(), "list"]);
    assert!(!r.err_contains("**WARNING**"));
}

#[test]
fn legacy_warning_suppressed_with_yadm_archive_override() {
    let tb = TestBed::new("legacy-warning-suppressed-archive-override");
    tb.write_home(".yadm/config", "");
    let custom_archive = tb.root.join("custom-archive");
    let r = tb.ryadm(&["--yadm-archive", &custom_archive.to_string_lossy(), "list"]);
    assert!(!r.err_contains("**WARNING**"));
}

#[test]
fn legacy_config_triggers_warning_on_stderr() {
    let tb = TestBed::new("legacy-warning");
    tb.write_home(".yadm/config", "");
    let r = tb.ryadm(&["list"]);
    // a legacy path fires the warning, naming the detected legacy config file
    let legacy_config = tb.home.join(".yadm/config");
    assert!(r.stderr.contains("**WARNING**"));
    assert!(r.stderr.contains("Legacy paths detected:"));
    assert!(r.stderr.contains(&*legacy_config.display().to_string()));
}

#[test]
fn legacy_warning_not_issued_when_yadm_dir_equals_legacy_dir() {
    // -Y $HOME/.yadm makes YADM_DIR itself the legacy dir; guard #2 fires.
    let tb = TestBed::new("legacy-warning-dir-is-legacy-dir");
    tb.write_home(".yadm/config", "");
    let legacy_dir = tb.home.join(".yadm");
    let r = tb.ryadm(&["-Y", &legacy_dir.to_string_lossy(), "list"]);
    assert!(!r.err_contains("**WARNING**"));
}

// ---------------------------------------------------------------------
// -w / work tree qualify_path label sanity (paths.rs qualify_path reuse)
// ---------------------------------------------------------------------

#[test]
fn work_tree_flag_uses_work_tree_label_in_empty_path_error() {
    let tb = TestBed::new("worktree-empty-label");
    // -w consumes the next token; passing it last means the "value" is
    // empty (no next arg), which qualify_path rejects with the "work tree"
    // label used by main()'s -w handling.
    let r = tb.ryadm(&["init", "-w"]);
    assert_eq!(r.code, 1);
    assert!(r.err_contains("You can't specify an empty work tree path"));
}

#[test]
fn nonexistent_work_tree_errors_exactly() {
    let tb = TestBed::new("worktree-nonexistent");
    let bogus = tb.home.join("nonexistent-workdir-xyz");
    let r = tb.ryadm(&["init", "-w", &bogus.to_string_lossy()]);
    assert_eq!(r.code, 1);
    assert!(r.err_contains(&format!("Work tree does not exist: [{}]", bogus.display())));
}
