//! Contract tests for repo-level commands: init, clone, upgrade, git
//! passthrough, and the disabled `clean` command. These pin the exact
//! strings/exit codes/filesystem effects (as opposed to
//! tests/compat_yadm.rs, which differentially compares against bash yadm).

mod common;

use common::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// `git --git-dir=<repo> config <key>` (trimmed stdout).
fn repo_config(repo: &Path, key: &str) -> String {
    let out = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(repo)
        .arg("config")
        .arg(key)
        .env("HOME", "/nonexistent")
        .output()
        .expect("git config");
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

fn mode_of(p: &Path) -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).map(|m| m.mode() & 0o7777).unwrap_or(0)
}

/// Build a local bare "remote" under `tb.root` with the given tracked files
/// committed on `master`; returns its absolute path. Any file whose content
/// starts with a shebang (`#!`) is made executable before being tracked, so
/// bootstrap-script fixtures are cloned with the exec bit intact.
fn make_remote(tb: &TestBed, name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let seed = tb.root.join(format!("{name}-seed"));
    let remote = tb.root.join(format!("{name}-remote.git"));
    let mut script = format!(
        "git init -q '{}'\ncd '{}'\n",
        seed.display(),
        seed.display()
    );
    for (path, content) in files {
        if let Some(parent) = Path::new(path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            script.push_str(&format!("mkdir -p '{}'\n", parent.display()));
        }
        script.push_str(&format!("cat > '{path}' <<'RADMEOF'\n{content}RADMEOF\n"));
        if content.starts_with("#!") {
            script.push_str(&format!("chmod 0755 '{path}'\n"));
        }
        script.push_str(&format!("git add '{path}'\n"));
    }
    script.push_str("git commit -q -m seed\n");
    script.push_str(&format!(
        "git clone -q --bare '{}' '{}'\n",
        seed.display(),
        remote.display()
    ));
    let r = tb.sh(&script);
    assert!(r.success(), "make_remote fixture failed: {r:?}");
    remote
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

#[test]
fn init_simple_success_sets_expected_config() {
    let tb = TestBed::new("init-simple");
    let r = tb.radm(&["init"]);
    assert!(r.out_contains("Initialized empty shared Git repository"));
    assert_eq!(r.stderr, "", "fresh init must have empty stderr");
    assert!(r.success());

    let repo = tb.repo();
    assert!(repo.is_dir());
    assert_eq!(mode_of(&repo) & 0o077, 0, "repo dir mode must end in 00");
    assert_eq!(repo_config(&repo, "core.bare"), "false");
    assert_eq!(repo_config(&repo, "status.showUntrackedFiles"), "no");
    assert_eq!(repo_config(&repo, "yadm.managed"), "true");
    assert_eq!(
        repo_config(&repo, "core.worktree"),
        tb.home.to_string_lossy()
    );
}

#[test]
fn init_with_w_sets_alternate_worktree() {
    let tb = TestBed::new("init-w");
    let work = tb.root.join("altwork");
    std::fs::create_dir_all(&work).unwrap();

    let r = tb.radm(&["init", "-w", work.to_str().unwrap()]);
    assert!(r.success());
    assert!(r.out_contains("Initialized empty shared Git repository"));
    assert_eq!(r.stderr, "");

    let repo = tb.repo();
    assert_eq!(repo_config(&repo, "core.worktree"), work.to_string_lossy());
    assert_eq!(repo_config(&repo, "core.bare"), "false");
    assert_eq!(repo_config(&repo, "status.showUntrackedFiles"), "no");
    assert_eq!(repo_config(&repo, "yadm.managed"), "true");
}

#[test]
fn init_existing_repo_without_force_fails_and_preserves_content() {
    let tb = TestBed::new("init-exists-noforce");
    std::fs::create_dir_all(tb.repo()).unwrap();
    let marker = tb.repo().join("old_repo");
    std::fs::write(&marker, "keep me").unwrap();

    let r = tb.radm(&["init"]);
    assert!(!r.success(), "init over existing repo must fail");
    assert_eq!(r.stdout, "");
    assert!(r.err_contains("repo already exists"));
    assert_eq!(r.code, 1);

    // exact two-line stderr shape (yadm's error_out with embedded \n)
    let expected = format!(
        "ERROR: Git repo already exists. [{}]\nUse '-f' if you want to force it to be overwritten.\n",
        tb.repo().display()
    );
    assert_eq!(r.stderr, expected);

    assert!(marker.is_file(), "original repo content must be untouched");
}

#[test]
fn init_force_reinit_destroys_old_repo_and_succeeds() {
    let tb = TestBed::new("init-force");
    std::fs::create_dir_all(tb.repo()).unwrap();
    let marker = tb.repo().join("old_repo");
    std::fs::write(&marker, "destroy me").unwrap();

    let r = tb.radm(&["init", "-f"]);
    assert!(r.success());
    assert!(r.out_contains("Initialized empty shared Git repository"));
    assert!(
        !marker.exists(),
        "old_repo marker must be gone after -f reinit"
    );

    let repo = tb.repo();
    assert_eq!(mode_of(&repo) & 0o077, 0);
    assert_eq!(repo_config(&repo, "core.bare"), "false");
    assert_eq!(repo_config(&repo, "status.showUntrackedFiles"), "no");
    assert_eq!(repo_config(&repo, "yadm.managed"), "true");
    assert_eq!(
        repo_config(&repo, "core.worktree"),
        tb.home.to_string_lossy()
    );
}

#[test]
fn init_w_and_force_together_relative_work_path() {
    let tb = TestBed::new("init-w-force");
    std::fs::create_dir_all(tb.repo()).unwrap();
    let marker = tb.repo().join("old_repo");
    std::fs::write(&marker, "destroy me").unwrap();

    let work_parent = tb.root.join("workparent");
    let work = work_parent.join("work");
    std::fs::create_dir_all(&work).unwrap();

    // `-w work` (relative), run with cwd = work's dirname.
    let r = tb.radm_in(&work_parent, &["init", "-w", "work", "-f"]);
    assert!(r.success());
    assert!(r.out_contains("Initialized empty shared Git repository"));
    assert!(!marker.exists());

    let repo = tb.repo();
    assert_eq!(repo_config(&repo, "core.worktree"), work.to_string_lossy());
}

// ---------------------------------------------------------------------------
// clone
// ---------------------------------------------------------------------------

#[test]
fn clone_bad_remote_cleans_up_and_reports_exact_error() {
    let tb = TestBed::new("clone-bad-remote");
    let bogus = tb.home.join("does-not-exist.git");

    let r = tb.radm(&["clone", bogus.to_str().unwrap()]);
    assert!(!r.success());
    assert_eq!(r.stdout, "");
    assert!(r.err_contains("Unable to clone the repository"));
    assert!(
        !tb.repo().exists(),
        "repo dir must not exist after a failed clone (cleanup guarantee)"
    );
}

#[test]
fn clone_no_repository_specified() {
    let tb = TestBed::new("clone-no-repo");
    let r = tb.radm(&["clone", "-f"]);
    assert!(!r.success());
    assert_eq!(r.stdout, "");
    assert!(r.err_contains("ERROR: Unable to clone the repository"));
    assert!(r.err_contains("repository 'repo.git' does not exist"));
}

#[test]
fn clone_simple_success_sets_head_and_remote() {
    let tb = TestBed::new("clone-simple");
    let remote = make_remote(&tb, "r", &[("t1", "cloned content\n")]);

    let r = tb.radm(&["clone", remote.to_str().unwrap()]);
    assert!(r.success());
    let repo = tb.repo();
    assert_eq!(mode_of(&repo) & 0o077, 0);
    assert_eq!(repo_config(&repo, "core.bare"), "false");
    assert_eq!(repo_config(&repo, "status.showUntrackedFiles"), "no");
    assert_eq!(repo_config(&repo, "yadm.managed"), "true");

    let head = std::fs::read_to_string(repo.join("HEAD")).unwrap();
    assert_eq!(head, "ref: refs/heads/master\n");

    assert_eq!(tb.read_home("t1"), "cloned content\n");

    let remote_v = tb.radm(&["remote", "-v", "show"]);
    assert!(remote_v.success());
    assert_eq!(remote_v.stderr, "");
    assert!(remote_v.out_contains(&format!("origin\t{}", remote.display())));
}

#[test]
fn clone_existing_repo_without_force_fails_preserving_marker() {
    let tb = TestBed::new("clone-exists-noforce");
    let remote = make_remote(&tb, "r", &[("t1", "cloned content\n")]);
    std::fs::create_dir_all(tb.repo()).unwrap();
    let marker = tb.repo().join("old_repo");
    std::fs::write(&marker, "keep me").unwrap();

    let r = tb.radm(&["clone", remote.to_str().unwrap()]);
    assert!(!r.success());
    assert_eq!(r.stdout, "");
    assert!(r.err_contains("Git repo already exists"));
    assert!(marker.is_file());
}

#[test]
fn clone_force_overwrites_existing_repo() {
    let tb = TestBed::new("clone-force");
    let remote = make_remote(&tb, "r", &[("t1", "cloned content\n")]);
    std::fs::create_dir_all(tb.repo()).unwrap();
    let marker = tb.repo().join("old_repo");
    std::fs::write(&marker, "destroy me").unwrap();

    let r = tb.radm(&["clone", "-f", remote.to_str().unwrap()]);
    assert!(r.success());
    assert!(!marker.exists());
    let repo = tb.repo();
    assert_eq!(repo_config(&repo, "core.bare"), "false");
    assert_eq!(repo_config(&repo, "yadm.managed"), "true");
    let head = std::fs::read_to_string(repo.join("HEAD")).unwrap();
    assert_eq!(head, "ref: refs/heads/master\n");
}

#[test]
fn clone_conflicts_preserves_local_file_and_prints_note() {
    let tb = TestBed::new("clone-conflicts");
    let remote = make_remote(&tb, "r", &[("t1", "cloned content\n")]);
    tb.write_home("t1", "conflict\n");

    let r = tb.radm(&["clone", remote.to_str().unwrap()]);
    assert!(r.success());
    assert!(r.out_contains("NOTE"));
    assert!(r.out_contains("Local files with content that differs"));

    // local content preserved, not overwritten
    assert_eq!(tb.read_home("t1"), "conflict\n");

    let status = tb.radm(&["status", "-uno", "--porcelain"]);
    assert!(status.success());
    assert_eq!(status.stderr, "");
    assert!(status.out_contains("t1"));

    let diff = tb.radm(&["diff"]);
    assert!(diff.out_contains("+conflict"));
}

#[test]
fn clone_no_checkout_leaves_worktree_empty() {
    let tb = TestBed::new("clone-no-checkout");
    let remote = make_remote(&tb, "r", &[("t1", "cloned content\n")]);

    let r = tb.radm(&["clone", "-n", remote.to_str().unwrap()]);
    assert!(r.success());
    assert!(!tb.exists("t1"), "-n must skip checkout entirely");

    let r2 = tb.radm(&["clone", "--no-checkout", "-f", remote.to_str().unwrap()]);
    assert!(r2.success());
    assert!(!tb.exists("t1"));
}

#[test]
fn clone_from_subdirectory_of_worktree_is_clean() {
    let tb = TestBed::new("clone-subdir");
    let remote = make_remote(&tb, "r", &[("t1", "cloned content\n")]);
    let subdir = tb.home.join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();

    let r = tb.radm_in(
        &subdir,
        &[
            "clone",
            "-w",
            tb.home.to_str().unwrap(),
            remote.to_str().unwrap(),
        ],
    );
    assert!(r.success());

    let status = tb.radm_in(&subdir, &["status", "-uno", "--porcelain"]);
    assert!(status.success());
    assert_eq!(status.stdout, "", "clean checkout should show no changes");
}

// -- submodules --------------------------------------------------------

fn make_remote_with_submodule(tb: &TestBed) -> std::path::PathBuf {
    let sub = tb.root.join("sub-repo");
    let seed = tb.root.join("subs-seed");
    let remote = tb.root.join("subs-remote.git");
    let script = format!(
        "git init -q '{sub}'\n\
cd '{sub}' && echo subcontent > afile && git add afile && git commit -q -m subcommit\n\
git init -q '{seed}'\n\
cd '{seed}'\n\
mkdir -p d1\n\
echo existing > d1/existing_file\n\
git add d1/existing_file\n\
git commit -q -m 'root commit'\n\
git -c protocol.file.allow=always submodule add '{sub}' a\n\
git -c protocol.file.allow=always submodule add '{sub}' b\n\
git -c protocol.file.allow=always submodule add '{sub}' d1/c\n\
git commit -q -m 'add submodules'\n\
git clone -q --bare '{seed}' '{remote}'\n",
        sub = sub.display(),
        seed = seed.display(),
        remote = remote.display(),
    );
    let r = tb.sh(&script);
    assert!(r.success(), "submodule remote fixture failed: {r:?}");
    remote
}

#[test]
fn clone_submodules_recursive_checks_out_all() {
    let tb = TestBed::new("clone-subm-recursive");
    let remote = make_remote_with_submodule(&tb);

    let r = tb.radm(&["clone", "--recursive", remote.to_str().unwrap()]);
    assert!(r.success(), "recursive clone failed: {r:?}");
    assert!(tb.exists("a/.git"));
    assert!(tb.exists("b/.git"));
    assert!(tb.exists("d1/c/.git"));
}

#[test]
fn clone_submodules_recurse_alias_matches_recursive() {
    let tb = TestBed::new("clone-subm-recurse-alias");
    let remote = make_remote_with_submodule(&tb);

    let r = tb.radm(&["clone", "--recurse-submodules", remote.to_str().unwrap()]);
    assert!(r.success(), "recurse-submodules clone failed: {r:?}");
    assert!(tb.exists("a/.git"));
    assert!(tb.exists("b/.git"));
    assert!(tb.exists("d1/c/.git"));
}

#[test]
fn clone_submodules_specific_paths_only() {
    let tb = TestBed::new("clone-subm-specific");
    let remote = make_remote_with_submodule(&tb);

    let r = tb.radm(&[
        "clone",
        "--recurse-submodules=a",
        "--recurse-submodules=d1/c",
        remote.to_str().unwrap(),
    ]);
    assert!(r.success(), "specific submodule clone failed: {r:?}");
    assert!(tb.exists("a/.git"));
    assert!(tb.exists("d1/c/.git"));
    assert!(
        !tb.exists("b/.git"),
        "b was not requested and must not be checked out as a submodule"
    );
}

// -- bootstrap -----------------------------------------------------------

const BOOTSTRAP_SCRIPT: &str = "#!/bin/sh\necho \"Bootstrap successful\"\nexit 123\n";

fn make_remote_with_bootstrap(tb: &TestBed) -> std::path::PathBuf {
    make_remote(
        tb,
        "bs",
        &[
            ("t1", "cloned content\n"),
            (".config/yadm/bootstrap", BOOTSTRAP_SCRIPT),
        ],
    )
}

#[test]
fn clone_bootstrap_flag_missing_file_no_op() {
    let tb = TestBed::new("clone-bs-force-missing");
    let remote = make_remote(&tb, "r", &[("t1", "cloned content\n")]);

    let r = tb.radm(&["clone", "--bootstrap", remote.to_str().unwrap()]);
    assert!(r.success());
    assert_eq!(r.code, 0);
    assert!(!r.out_contains("Bootstrap successful"));
}

#[test]
fn clone_bootstrap_flag_existing_file_forces_exec() {
    let tb = TestBed::new("clone-bs-force-existing");
    let remote = make_remote_with_bootstrap(&tb);

    let r = tb.radm(&["clone", "--bootstrap", remote.to_str().unwrap()]);
    assert_eq!(r.code, 123);
    assert!(r.out_contains("Bootstrap successful"));
    // clone itself succeeded regardless of the bootstrap exit code
    let repo = tb.repo();
    assert_eq!(repo_config(&repo, "core.bare"), "false");
}

#[test]
fn clone_no_bootstrap_flag_prevents_even_when_present() {
    let tb = TestBed::new("clone-bs-prevent");
    let remote = make_remote_with_bootstrap(&tb);

    let r = tb.radm(&["clone", "--no-bootstrap", remote.to_str().unwrap()]);
    assert!(r.success());
    assert_eq!(r.code, 0);
    assert!(!r.out_contains("Bootstrap successful"));
}

#[test]
fn clone_bootstrap_missing_file_default_prompt_is_noop() {
    let tb = TestBed::new("clone-bs-default-missing");
    let remote = make_remote(&tb, "r", &[("t1", "cloned content\n")]);

    let r = tb.radm(&["clone", remote.to_str().unwrap()]);
    assert!(r.success());
    assert_eq!(r.code, 0);
    assert!(!r.out_contains("Bootstrap successful"));
    // bootstrap_available() fails first: no prompt should even be printed
    assert!(!r.out_contains("Would you like to execute it now"));
}

#[test]
fn clone_bootstrap_default_prompt_no_tty_declines() {
    // No stdin/tty is ever attached by the harness, so the read fails and
    // the prompt is declined -- bootstrap must not run.
    let tb = TestBed::new("clone-bs-default-notty");
    let remote = make_remote_with_bootstrap(&tb);

    let r = tb.radm(&["clone", remote.to_str().unwrap()]);
    assert!(r.success());
    assert_eq!(r.code, 0);
    assert!(r.out_contains("Found"));
    assert!(r.out_contains("It appears that a bootstrap program exists."));
    assert!(r.out_contains("Would you like to execute it now? (y/n)"));
    assert!(!r.out_contains("Bootstrap successful"));
}

// -- private dir permission handling --------------------------------------

fn make_remote_with_private_dir(tb: &TestBed, dirname: &str) -> std::path::PathBuf {
    make_remote(
        tb,
        &format!("priv-{}", dirname.trim_start_matches('.')),
        &[
            ("t1", "cloned content\n"),
            (&format!("{dirname}/tracked_secret"), "secret\n"),
        ],
    )
}

#[test]
fn clone_perms_ssh_preexisting_insecure_left_until_final_sweep() {
    let tb = TestBed::new("clone-perms-ssh-inwork");
    let remote = make_remote_with_private_dir(&tb, ".ssh");
    tb.write_home(".ssh/placeholder", "x\n");
    std::fs::set_permissions(tb.home.join(".ssh"), std::fs::Permissions::from_mode(0o777)).unwrap();

    let r = tb.radm(&[
        "clone",
        "-d",
        "-w",
        tb.home.to_str().unwrap(),
        remote.to_str().unwrap(),
    ]);
    assert!(r.success(), "{r:?}");
    assert!(r.out_contains("initial private dir perms"));
    assert!(r.out_contains("pre-checkout private dir perms"));
    assert!(r.out_contains("post-checkout private dir perms"));

    // final invariant: no matter the transient state, perms end up "00"
    assert_eq!(tb.mode(".ssh") & 0o077, 0);
}

#[test]
fn clone_perms_ssh_absent_created_secure_from_the_start() {
    let tb = TestBed::new("clone-perms-ssh-notinwork");
    let remote = make_remote_with_private_dir(&tb, ".ssh");

    let r = tb.radm(&[
        "clone",
        "-d",
        "-w",
        tb.home.to_str().unwrap(),
        remote.to_str().unwrap(),
    ]);
    assert!(r.success(), "{r:?}");
    assert!(!r.out_contains("initial private dir perms"));
    assert!(r.out_contains("pre-checkout private dir perms"));
    assert!(r.out_contains("post-checkout private dir perms"));

    assert_eq!(tb.mode(".ssh") & 0o077, 0);
}

#[test]
fn clone_perms_gnupg_preexisting_insecure_left_until_final_sweep() {
    let tb = TestBed::new("clone-perms-gnupg-inwork");
    let remote = make_remote_with_private_dir(&tb, ".gnupg");
    tb.write_home(".gnupg/placeholder", "x\n");
    std::fs::set_permissions(
        tb.home.join(".gnupg"),
        std::fs::Permissions::from_mode(0o777),
    )
    .unwrap();

    let r = tb.radm(&[
        "clone",
        "-d",
        "-w",
        tb.home.to_str().unwrap(),
        remote.to_str().unwrap(),
    ]);
    assert!(r.success(), "{r:?}");
    assert!(r.out_contains("initial private dir perms"));
    assert!(r.out_contains("pre-checkout private dir perms"));
    assert!(r.out_contains("post-checkout private dir perms"));
    assert_eq!(tb.mode(".gnupg") & 0o077, 0);
}

#[test]
fn clone_perms_gnupg_absent_created_secure_from_the_start() {
    let tb = TestBed::new("clone-perms-gnupg-notinwork");
    let remote = make_remote_with_private_dir(&tb, ".gnupg");

    let r = tb.radm(&[
        "clone",
        "-d",
        "-w",
        tb.home.to_str().unwrap(),
        remote.to_str().unwrap(),
    ]);
    assert!(r.success(), "{r:?}");
    assert!(!r.out_contains("initial private dir perms"));
    assert!(r.out_contains("pre-checkout private dir perms"));
    assert!(r.out_contains("post-checkout private dir perms"));
    assert_eq!(tb.mode(".gnupg") & 0o077, 0);
}

// -- alternate branch cloning ----------------------------------------------

fn make_remote_with_branches(tb: &TestBed, head_branch: &str) -> std::path::PathBuf {
    let seed = tb.root.join("branch-seed");
    let remote = tb.root.join("branch-remote.git");
    let script = format!(
        "git init -q '{seed}'\n\
cd '{seed}'\n\
echo t1 > t1 && git add t1 && git commit -q -m 'Initial commit'\n\
git checkout -qb valid\n\
git commit -q --allow-empty -m 'This branch is valid'\n\
git checkout -q master\n\
git clone -q --bare '{seed}' '{remote}'\n\
git --git-dir='{remote}' symbolic-ref HEAD refs/heads/{head_branch}\n",
        seed = seed.display(),
        remote = remote.display(),
        head_branch = head_branch,
    );
    let r = tb.sh(&script);
    assert!(r.success(), "branch remote fixture failed: {r:?}");
    remote
}

#[test]
fn clone_alternate_branch_master_default() {
    let tb = TestBed::new("clone-branch-master");
    let remote = make_remote_with_branches(&tb, "master");

    let r = tb.radm(&["clone", remote.to_str().unwrap()]);
    assert!(r.success());
    let repo = tb.repo();
    let head = std::fs::read_to_string(repo.join("HEAD")).unwrap();
    assert_eq!(head, "ref: refs/heads/master\n");

    let show = tb.radm(&["show"]);
    assert!(show.out_contains("Initial commit"));

    let remote_v = tb.radm(&["remote", "-v", "show"]);
    assert!(remote_v.out_contains(&format!("origin\t{}", remote.display())));
}

#[test]
fn clone_alternate_branch_follows_remote_default_head() {
    let tb = TestBed::new("clone-branch-default");
    let remote = make_remote_with_branches(&tb, "valid");

    // no -b flag: yadm/radm must not hardcode "master", it follows git's own
    // default-branch detection (remote HEAD points at "valid").
    let r = tb.radm(&["clone", remote.to_str().unwrap()]);
    assert!(r.success());
    let repo = tb.repo();
    let head = std::fs::read_to_string(repo.join("HEAD")).unwrap();
    assert_eq!(head, "ref: refs/heads/valid\n");

    let show = tb.radm(&["show"]);
    assert!(show.out_contains("This branch is valid"));
}

#[test]
fn clone_alternate_branch_explicit_valid() {
    let tb = TestBed::new("clone-branch-explicit-valid");
    let remote = make_remote_with_branches(&tb, "master");

    let r = tb.radm(&["clone", "-b", "valid", remote.to_str().unwrap()]);
    assert!(r.success());
    let repo = tb.repo();
    let head = std::fs::read_to_string(repo.join("HEAD")).unwrap();
    assert_eq!(head, "ref: refs/heads/valid\n");

    let show = tb.radm(&["show"]);
    assert!(show.out_contains("This branch is valid"));

    let remote_v = tb.radm(&["remote", "-v", "show"]);
    assert!(remote_v.out_contains(&format!("origin\t{}", remote.display())));
}

#[test]
fn clone_alternate_branch_invalid_fails_with_both_messages() {
    let tb = TestBed::new("clone-branch-invalid");
    let remote = make_remote_with_branches(&tb, "master");

    let r = tb.radm(&["clone", "-b", "invalid", remote.to_str().unwrap()]);
    assert!(!r.success());
    assert!(r.err_contains("ERROR: Unable to clone the repository"));
    assert!(r.err_contains("Remote branch invalid not found in upstream"));
}

// ---------------------------------------------------------------------------
// upgrade
// ---------------------------------------------------------------------------

#[test]
fn upgrade_no_legacy_paths_reports_not_necessary() {
    let tb = TestBed::new("upgrade-no-paths");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    let r = tb.radm(&["upgrade"]);
    assert!(r.success());
    assert_eq!(r.code, 0);
    assert!(r.out_contains("No legacy paths found. Upgrade is not necessary"));
    assert_eq!(r.stderr, "");
}

#[test]
fn upgrade_override_repo_guard_error() {
    let tb = TestBed::new("upgrade-override-guard");
    let r = tb.radm(&[
        "--yadm-repo",
        tb.home.join("other-repo.git").to_str().unwrap(),
        "upgrade",
    ]);
    assert!(!r.success());
    assert_eq!(r.code, 1);
    assert!(r.err_contains(
        "ERROR: Unable to upgrade. Paths have been overridden with command line options"
    ));
}

#[test]
fn upgrade_existing_target_repo_collision_error() {
    let tb = TestBed::new("upgrade-existing-repo");
    // simulate a v2-layout legacy repo AND a pre-existing target repo
    std::fs::create_dir_all(tb.yadm_dir().join("repo.git")).unwrap();
    std::fs::create_dir_all(tb.repo()).unwrap();

    let r = tb.radm(&["upgrade"]);
    assert!(!r.success());
    assert_eq!(r.code, 1);
    assert!(r.err_contains("Unable to upgrade"));
    assert!(r.err_contains(&format!(
        "'{}' already exists. Refusing to overwrite it.",
        tb.repo().display()
    )));
}

#[test]
fn upgrade_moves_legacy_repo_and_v1_paths() {
    let tb = TestBed::new("upgrade-migrate");
    tb.init_repo_with(&[("t1", "data\n")]);

    // simulate the on-disk layout yadm 1.x/2.x would have left behind:
    // legacy repo at the v2 location ($YADM_DIR/repo.git), plus v1-only
    // paths under ~/.yadm.
    let legacy_repo = tb.yadm_dir().join("repo.git");
    std::fs::rename(tb.repo(), &legacy_repo).unwrap();

    let legacy_dir = tb.home.join(".yadm");
    std::fs::create_dir_all(legacy_dir.join("hooks")).unwrap();
    std::fs::write(legacy_dir.join("config"), "config content").unwrap();
    std::fs::write(legacy_dir.join("encrypt"), "encrypt content").unwrap();
    std::fs::write(legacy_dir.join("bootstrap"), "bootstrap content").unwrap();
    std::fs::write(legacy_dir.join("hooks/pre_cmd"), "hook content").unwrap();

    let r = tb.radm(&["upgrade"]);
    assert!(r.success());
    assert_eq!(r.code, 0);
    assert!(r.out_contains(&format!(
        "Moving {} to {}",
        legacy_repo.display(),
        tb.repo().display()
    )));
    assert!(r.out_contains(&format!(
        "Moving {} to {}",
        legacy_dir.join("config").display(),
        tb.yadm_dir().join("config").display()
    )));
    assert!(r.out_contains(&format!(
        "Moving {} to {}",
        legacy_dir.join("hooks/pre_cmd").display(),
        tb.yadm_dir().join("hooks/pre_cmd").display()
    )));

    assert!(
        tb.repo().is_dir(),
        "repo must have moved to the new location"
    );
    assert!(!legacy_repo.exists());
    assert_eq!(tb.read_home(".config/yadm/config"), "config content");
    assert_eq!(tb.read_home(".config/yadm/encrypt"), "encrypt content");
    assert_eq!(tb.read_home(".config/yadm/bootstrap"), "bootstrap content");
    assert_eq!(tb.read_home(".config/yadm/hooks/pre_cmd"), "hook content");

    // repo relocated but content intact
    let status = tb.radm(&["status"]);
    assert!(status.success());
    let show = tb.radm(&["show", "HEAD:t1"]);
    assert!(show.out_contains("data"));
}

#[test]
fn upgrade_never_runs_post_hook_on_success() {
    // yadm's upgrade() does a hard `exit 0`, bypassing exit_with_hook -- the
    // post hook must not fire for a successful upgrade.
    let tb = TestBed::new("upgrade-no-post-hook");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    let post_hook = "#!/bin/sh\necho POST_HOOK_RAN\n";
    tb.write_home_mode(".config/yadm/hooks/post_upgrade", post_hook, 0o755);

    let r = tb.radm(&["upgrade"]);
    assert!(r.success());
    assert!(!r.out_contains("POST_HOOK_RAN"));
}

// ---------------------------------------------------------------------------
// 4. git passthrough
// ---------------------------------------------------------------------------

#[test]
fn git_passthrough_unknown_subcommand() {
    let tb = TestBed::new("git-bogus");
    tb.init_repo_with(&[("t1", "data\n")]);

    let r = tb.radm(&["bogus"]);
    assert!(!r.success());
    assert_eq!(r.stdout, "");
    assert!(r.err_contains("is not a git command"));
}

#[test]
fn git_passthrough_bad_pathspec_exit_code_128() {
    let tb = TestBed::new("git-bad-pathspec");
    tb.init_repo_with(&[("t1", "data\n")]);

    let r = tb.radm(&["add", "-v", "does_not_exist"]);
    assert_eq!(r.code, 128);
    assert_eq!(r.stdout, "");
    assert!(r.err_contains("pathspec 'does_not_exist' did not match any files"));
}

#[test]
fn git_passthrough_add_new_file_succeeds() {
    let tb = TestBed::new("git-add-ok");
    tb.init_repo_with(&[("t1", "data\n")]);
    tb.write_home("newfile", "content\n");

    let r = tb.radm(&["add", "-v", "newfile"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert!(r.out_contains("add 'newfile'"));
}

#[test]
fn git_passthrough_status_shows_new_file() {
    let tb = TestBed::new("git-status");
    tb.init_repo_with(&[("t1", "data\n")]);
    tb.write_home("newfile", "content\n");
    let add = tb.radm(&["add", "newfile"]);
    assert!(add.success());

    let r = tb.radm(&["status"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert!(r.out_contains("new file:"));
    assert!(r.out_contains("newfile"));
}

#[test]
fn git_passthrough_commit_and_log() {
    let tb = TestBed::new("git-commit-log");
    tb.init_repo_with(&[("t1", "data\n")]);
    tb.write_home("newfile", "content\n");
    let add = tb.radm(&["add", "newfile"]);
    assert!(add.success());

    let commit = tb.radm(&["commit", "-m", "Add newfile"]);
    assert!(commit.success());
    assert_eq!(commit.stderr, "");
    assert!(commit.out_contains("1 file changed"));
    assert!(commit.out_contains("1 insertion"));

    let log = tb.radm(&["log", "--oneline"]);
    assert!(log.success());
    assert_eq!(log.stderr, "");
    assert!(log.out_contains("Add newfile"));
}

#[test]
fn gitconfig_translates_to_config() {
    let tb = TestBed::new("gitconfig-translate");
    tb.init_repo_with(&[]);
    let r = tb.radm(&["gitconfig", "user.name"]);
    assert!(r.success());
    assert_eq!(r.stdout.trim_end(), "Test User");
}

// ---------------------------------------------------------------------------
// 5. clean (disabled)
// ---------------------------------------------------------------------------

#[test]
fn clean_is_disabled_with_exact_message() {
    let tb = TestBed::new("clean-disabled");
    tb.init_repo_with(&[("t1", "data\n")]);

    let r = tb.radm(&["clean"]);
    assert!(!r.success());
    assert_eq!(r.code, 1);
    assert_eq!(r.stdout, "");
    assert_eq!(
        r.stderr,
        "ERROR: \"git clean\" has been disabled for safety. You could end up removing all unmanaged files.\n"
    );
}

#[test]
fn clean_is_disabled_regardless_of_arguments() {
    let tb = TestBed::new("clean-disabled-args");
    tb.init_repo_with(&[("t1", "data\n")]);

    let r = tb.radm(&["clean", "-fdx"]);
    assert!(!r.success());
    assert_eq!(r.code, 1);
    assert_eq!(r.stdout, "");
    assert!(r.err_contains("disabled"));

    // no filesystem side effects: tracked file untouched, no extra files removed
    assert_eq!(tb.read_home("t1"), "data\n");
}
