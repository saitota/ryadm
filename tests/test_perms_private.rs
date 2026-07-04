//! Permissions & private-directory contract tests.
//!
//! Pins the exact, byte-level contract of `yadm perms` / `auto_perms()` /
//! `private_dirs()` / `assert_private_dirs()` / `auto_private_dirs` with
//! explicit expected strings, exit codes, and filesystem effects (rather than
//! only differential comparison against the bash reference).

mod common;
use common::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// mode ends in "00" -- fully secured (group/other bits cleared by go-rwx).
fn secured(mode: u32) -> bool {
    mode & 0o077 == 0
}

/// Tracked private files under .ssh/.gnupg plus a couple of top-level
/// files, enough to exercise perms()/private-dirs semantics.
fn seed_private_files(tb: &TestBed) {
    tb.write_home_mode(".ssh/p1", "p1\n", 0o644);
    tb.write_home_mode(".ssh/.p2", "p2\n", 0o644);
    tb.write_home_mode(".gnupg/p3", "p3\n", 0o644);
    tb.write_home_mode(".gnupg/.p4", "p4\n", 0o644);
}

// ---------------------------------------------------------------------------
// `perms()` -- yadm perms CLI: exact stdout/stderr/exit contract
// ---------------------------------------------------------------------------

#[test]
fn perms_cli_produces_no_output_and_exits_zero() {
    let tb = TestBed::new("perms-cli-silent");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    seed_private_files(&tb);

    let r = tb.radm(&["perms"]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");
}

#[test]
fn perms_secures_ssh_and_gnupg_files_go_rwx() {
    let tb = TestBed::new("perms-secures-ssh-gnupg");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    seed_private_files(&tb);
    // Precondition: none of the private paths start pre-secured.
    for p in [
        ".ssh",
        ".gnupg",
        ".ssh/p1",
        ".ssh/.p2",
        ".gnupg/p3",
        ".gnupg/.p4",
    ] {
        assert!(!secured(tb.mode(p)), "{p} unexpectedly pre-secured");
    }

    let r = tb.radm(&["perms"]);
    assert!(r.success());

    for p in [
        ".ssh",
        ".gnupg",
        ".ssh/p1",
        ".ssh/.p2",
        ".gnupg/p3",
        ".gnupg/.p4",
    ] {
        assert!(
            secured(tb.mode(p)),
            "{p} not secured after perms: {:o}",
            tb.mode(p)
        );
    }
}

#[test]
fn perms_exact_mode_transitions_go_rwx_keeps_user_bits() {
    // go-rwx is a *relative* chmod: user bits untouched, group/other cleared.
    let tb = TestBed::new("perms-exact-modes");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    tb.write_home_mode(".ssh/a", "a\n", 0o755);
    tb.write_home_mode(".ssh/b", "b\n", 0o644);
    tb.write_home_mode(".ssh/c", "c\n", 0o777);

    let r = tb.radm(&["perms"]);
    assert!(r.success());

    assert_eq!(tb.mode(".ssh/a"), 0o700);
    assert_eq!(tb.mode(".ssh/b"), 0o600);
    assert_eq!(tb.mode(".ssh/c"), 0o700);
}

#[test]
fn perms_does_not_touch_worktree_root_mode() {
    let tb = TestBed::new("perms-worktree-root-untouched");
    let r = tb.radm(&["init"]);
    assert!(r.success());
    seed_private_files(&tb);

    std::fs::set_permissions(
        &tb.home,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();

    let r = tb.radm(&["perms"]);
    assert!(r.success());

    use std::os::unix::fs::MetadataExt;
    let mode = std::fs::metadata(&tb.home).unwrap().mode() & 0o7777;
    assert_eq!(
        mode, 0o755,
        "worktree root mode must be untouched by perms()"
    );
}

#[test]
fn perms_secures_archive_file_when_present() {
    let tb = TestBed::new("perms-archive");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    // YADM_ARCHIVE default location: $YADM_DATA/yadm/archive -> here
    // tb.archive() is $YADM_DATA/archive (radm's ctx.archive base), confirm
    // by writing directly there via write_home-style absolute path.
    let archive = tb.archive();
    std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
    std::fs::write(&archive, "").unwrap();
    std::fs::set_permissions(
        &archive,
        std::os::unix::fs::PermissionsExt::from_mode(0o666),
    )
    .unwrap();

    let r = tb.radm(&["perms"]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");

    use std::os::unix::fs::MetadataExt;
    let mode = std::fs::metadata(&archive).unwrap().mode() & 0o7777;
    assert_eq!(mode, 0o600, "archive file must be secured to go-rwx");
}

#[test]
fn perms_secures_encrypt_listed_files_but_not_excluded_ones() {
    // the "!" prefix excludes efile1 from ENCRYPT_INCLUDE_FILES, so only
    // efile2 gets secured by perms().
    let tb = TestBed::new("perms-encrypt-listed");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    tb.write_home_mode("efile1", "efile1", 0o644);
    tb.write_home_mode("efile2", "efile2", 0o644);
    tb.write_home("config/yadm/encrypt", "unused"); // sanity: wrong path, no effect
    tb.write_home(".config/yadm/encrypt", "efile1\nefile2\n!efile1\n");

    assert!(!secured(tb.mode("efile1")));
    assert!(!secured(tb.mode("efile2")));

    let r = tb.radm(&["perms"]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");

    // efile1 excluded via "!efile1": must remain unsecured.
    assert_eq!(
        tb.mode("efile1"),
        0o644,
        "excluded encrypt file must stay untouched"
    );
    // efile2 included: must be secured.
    assert!(secured(tb.mode("efile2")), "efile2 should be secured");
}

// ---------------------------------------------------------------------------
// yadm.ssh-perms / yadm.gpg-perms fine-grained control
// ---------------------------------------------------------------------------

#[test]
fn ssh_perms_false_leaves_ssh_untouched_but_still_secures_gnupg() {
    let tb = TestBed::new("perms-ssh-false");
    let r = tb.radm(&["init"]);
    assert!(r.success());
    seed_private_files(&tb);

    let r = tb.radm(&["config", "yadm.ssh-perms", "false"]);
    assert!(r.success());

    let r = tb.radm(&["perms"]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");

    // .ssh untouched
    assert!(!secured(tb.mode(".ssh")));
    assert!(!secured(tb.mode(".ssh/p1")));
    assert!(!secured(tb.mode(".ssh/.p2")));
    // .gnupg still secured
    assert!(secured(tb.mode(".gnupg")));
    assert!(secured(tb.mode(".gnupg/p3")));
    assert!(secured(tb.mode(".gnupg/.p4")));
}

#[test]
fn gpg_perms_false_leaves_gnupg_untouched_but_still_secures_ssh() {
    let tb = TestBed::new("perms-gpg-false");
    let r = tb.radm(&["init"]);
    assert!(r.success());
    seed_private_files(&tb);

    let r = tb.radm(&["config", "yadm.gpg-perms", "false"]);
    assert!(r.success());

    let r = tb.radm(&["perms"]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");

    assert!(secured(tb.mode(".ssh")));
    assert!(secured(tb.mode(".ssh/p1")));
    assert!(secured(tb.mode(".ssh/.p2")));

    assert!(!secured(tb.mode(".gnupg")));
    assert!(!secured(tb.mode(".gnupg/p3")));
    assert!(!secured(tb.mode(".gnupg/.p4")));
}

#[test]
fn both_ssh_and_gpg_perms_false_leaves_both_untouched() {
    let tb = TestBed::new("perms-both-false");
    let r = tb.radm(&["init"]);
    assert!(r.success());
    seed_private_files(&tb);

    assert!(tb.radm(&["config", "yadm.ssh-perms", "false"]).success());
    assert!(tb.radm(&["config", "yadm.gpg-perms", "false"]).success());

    let r = tb.radm(&["perms"]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");

    for p in [
        ".ssh",
        ".ssh/p1",
        ".ssh/.p2",
        ".gnupg",
        ".gnupg/p3",
        ".gnupg/.p4",
    ] {
        assert!(!secured(tb.mode(p)), "{p} should remain unsecured");
    }
    // worktree root mode unaffected regardless
    use std::os::unix::fs::MetadataExt;
    let mode = std::fs::metadata(&tb.home).unwrap().mode() & 0o7777;
    assert_eq!(mode, 0o755);
}

#[test]
fn ssh_perms_true_and_gpg_perms_true_both_secure() {
    let tb = TestBed::new("perms-both-true");
    let r = tb.radm(&["init"]);
    assert!(r.success());
    seed_private_files(&tb);

    assert!(tb.radm(&["config", "yadm.ssh-perms", "true"]).success());
    assert!(tb.radm(&["config", "yadm.gpg-perms", "true"]).success());

    let r = tb.radm(&["perms"]);
    assert!(r.success());

    for p in [
        ".ssh",
        ".ssh/p1",
        ".ssh/.p2",
        ".gnupg",
        ".gnupg/p3",
        ".gnupg/.p4",
    ] {
        assert!(secured(tb.mode(p)), "{p} should be secured");
    }
}

// ---------------------------------------------------------------------------
// GNUPGHOME env moves the gnupg glob target
// ---------------------------------------------------------------------------

#[test]
fn gnupghome_env_relocates_gnupg_perms_target() {
    let tb = TestBed::new("perms-gnupghome-env");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    // Files under the default .gnupg must NOT be touched...
    tb.write_home_mode(".gnupg/p3", "p3\n", 0o644);
    // ...while files under the GNUPGHOME-relocated dir must be secured.
    tb.write_home_mode("alt/gnupghome/p3", "p3\n", 0o644);
    tb.write_home_mode("alt/gnupghome/.p4", "p4\n", 0o644);

    let gnupghome_abs = tb.home_path("alt/gnupghome");
    let r = tb.radm_env(&["perms"], "GNUPGHOME", &gnupghome_abs.to_string_lossy());
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");

    // Default .gnupg untouched since GNUPGHOME overrides the glob base.
    assert!(!secured(tb.mode(".gnupg/p3")));
    // Relocated dir secured.
    assert!(secured(tb.mode("alt/gnupghome/p3")));
    assert!(secured(tb.mode("alt/gnupghome/.p4")));
    assert!(secured(tb.mode("alt/gnupghome")));
}

#[test]
fn gnupghome_env_outside_worktree_still_secures_absolute_target() {
    // GNUPGHOME pointing entirely outside YADM_WORK: relative_path falls
    // back to an absolute/`../`-laden path, but perms() must still resolve
    // and chmod the real target.
    let tb = TestBed::new("perms-gnupghome-outside");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    let outside_dir = tb.root.join("outside-gnupg");
    std::fs::create_dir_all(&outside_dir).unwrap();
    let secret = outside_dir.join("secret");
    std::fs::write(&secret, "s\n").unwrap();
    std::fs::set_permissions(&secret, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
    std::fs::set_permissions(
        &outside_dir,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();

    let r = tb.radm_env(&["perms"], "GNUPGHOME", &outside_dir.to_string_lossy());
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");

    use std::os::unix::fs::MetadataExt;
    let mode = std::fs::metadata(&secret).unwrap().mode() & 0o7777;
    assert_eq!(
        mode, 0o600,
        "file under externally-relocated GNUPGHOME must be secured"
    );
    let dir_mode = std::fs::metadata(&outside_dir).unwrap().mode() & 0o7777;
    assert_eq!(
        dir_mode, 0o700,
        "externally-relocated GNUPGHOME dir itself must be secured"
    );
}

// ---------------------------------------------------------------------------
// auto_perms(): runs after git passthrough commands, gated by yadm.auto-perms
// ---------------------------------------------------------------------------

#[test]
fn auto_perms_runs_after_status_and_secures_private_paths() {
    let tb = TestBed::new("auto-perms-status");
    let r = tb.radm(&["init"]);
    assert!(r.success());
    seed_private_files(&tb);

    for p in [
        ".ssh",
        ".gnupg",
        ".ssh/p1",
        ".ssh/.p2",
        ".gnupg/p3",
        ".gnupg/.p4",
    ] {
        assert!(!secured(tb.mode(p)));
    }

    let r = tb.radm(&["status"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");

    for p in [
        ".ssh",
        ".gnupg",
        ".ssh/p1",
        ".ssh/.p2",
        ".gnupg/p3",
        ".gnupg/.p4",
    ] {
        assert!(
            secured(tb.mode(p)),
            "{p} should be secured by auto_perms via status"
        );
    }
}

#[test]
fn auto_perms_disabled_via_config_leaves_private_paths_untouched() {
    let tb = TestBed::new("auto-perms-disabled");
    let r = tb.radm(&["init"]);
    assert!(r.success());
    seed_private_files(&tb);

    let r = tb.radm(&["config", "yadm.auto-perms", "false"]);
    assert!(r.success());

    for p in [
        ".ssh",
        ".gnupg",
        ".ssh/p1",
        ".ssh/.p2",
        ".gnupg/p3",
        ".gnupg/.p4",
    ] {
        assert!(!secured(tb.mode(p)));
    }

    let r = tb.radm(&["status"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");

    for p in [
        ".ssh",
        ".gnupg",
        ".ssh/p1",
        ".ssh/.p2",
        ".gnupg/p3",
        ".gnupg/.p4",
    ] {
        assert!(
            !secured(tb.mode(p)),
            "{p} must remain unsecured when auto-perms=false"
        );
    }
}

#[test]
fn auto_perms_true_explicit_behaves_like_default() {
    let tb = TestBed::new("auto-perms-true");
    let r = tb.radm(&["init"]);
    assert!(r.success());
    seed_private_files(&tb);

    let r = tb.radm(&["config", "yadm.auto-perms", "true"]);
    assert!(r.success());

    let r = tb.radm(&["status"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");

    for p in [
        ".ssh",
        ".gnupg",
        ".ssh/p1",
        ".ssh/.p2",
        ".gnupg/p3",
        ".gnupg/.p4",
    ] {
        assert!(secured(tb.mode(p)));
    }
}

// ---------------------------------------------------------------------------
// auto_private_dirs: git passthrough commands create .ssh/.gnupg at 0700
// ---------------------------------------------------------------------------

#[test]
fn auto_private_dirs_created_0700_by_git_passthrough_when_missing() {
    let tb = TestBed::new("auto-pdirs-create");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    assert!(!tb.exists(".ssh"));
    assert!(!tb.exists(".gnupg"));

    let r = tb.radm(&["status"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert!(r.stdout.contains("On branch master"));

    assert!(tb.exists(".ssh"));
    assert!(tb.exists(".gnupg"));
    assert_eq!(tb.mode(".ssh"), 0o700);
    assert_eq!(tb.mode(".gnupg"), 0o700);
}

#[test]
fn auto_private_dirs_disabled_via_config_leaves_dirs_absent() {
    let tb = TestBed::new("auto-pdirs-disabled");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    let r = tb.radm(&["config", "yadm.auto-private-dirs", "false"]);
    assert!(r.success());

    assert!(!tb.exists(".ssh"));
    assert!(!tb.exists(".gnupg"));

    let r = tb.radm(&["status"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert!(r.stdout.contains("On branch master"));

    assert!(
        !tb.exists(".ssh"),
        ".ssh must not be created when auto-private-dirs=false"
    );
    assert!(
        !tb.exists(".gnupg"),
        ".gnupg must not be created when auto-private-dirs=false"
    );
}

#[test]
fn auto_private_dirs_not_created_when_worktree_differs_from_home() {
    // init -w <altwork>: YADM_WORK != HOME, so the auto-private-dirs gate in
    // git_command() never fires, regardless of the auto-private-dirs config.
    let tb = TestBed::new("auto-pdirs-alt-worktree");
    let altwork = tb.root.join("altwork");
    std::fs::create_dir_all(&altwork).unwrap();

    let r = tb.radm(&["init", "-w", &altwork.to_string_lossy()]);
    assert!(r.success());

    let r = tb.radm(&["status"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");

    // Neither HOME nor the alt worktree gets .ssh/.gnupg created.
    assert!(!tb.exists(".ssh"));
    assert!(!tb.exists(".gnupg"));
    assert!(!altwork.join(".ssh").exists());
    assert!(!altwork.join(".gnupg").exists());
}

#[test]
fn auto_private_dirs_leaves_existing_dirs_and_their_modes_alone() {
    // If the directories already exist, assert_private_dirs is a no-op:
    // no mkdir call, existing permissions untouched.
    let tb = TestBed::new("auto-pdirs-existing-untouched");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    std::fs::create_dir_all(tb.home_path(".ssh")).unwrap();
    std::fs::create_dir_all(tb.home_path(".gnupg")).unwrap();
    std::fs::set_permissions(
        tb.home_path(".ssh"),
        std::os::unix::fs::PermissionsExt::from_mode(0o777),
    )
    .unwrap();
    std::fs::set_permissions(
        tb.home_path(".gnupg"),
        std::os::unix::fs::PermissionsExt::from_mode(0o777),
    )
    .unwrap();

    // Disable auto-perms so the go-rwx chmod doesn't also run and mask the
    // "existing dirs untouched by assert_private_dirs" assertion.
    let r = tb.radm(&["config", "yadm.auto-perms", "false"]);
    assert!(r.success());

    let r = tb.radm(&["status"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert!(r.stdout.contains("On branch master"));

    assert_eq!(
        tb.mode(".ssh"),
        0o777,
        ".ssh mode must be untouched when already present"
    );
    assert_eq!(
        tb.mode(".gnupg"),
        0o777,
        ".gnupg mode must be untouched when already present"
    );
}

#[test]
fn auto_private_dirs_creates_nested_gnupghome_via_env() {
    // GNUPGHOME set to a nested path: assert_private_dirs must create all
    // intermediate parents too, with the leaf at exactly 0700.
    let tb = TestBed::new("auto-pdirs-nested-gnupghome");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    let gnupghome_abs = tb.home_path("alt/nested/gnupghome");
    assert!(!gnupghome_abs.exists());

    let r = tb.radm_env(&["status"], "GNUPGHOME", &gnupghome_abs.to_string_lossy());
    assert!(r.success());
    assert_eq!(r.stderr, "");

    assert!(gnupghome_abs.is_dir());
    use std::os::unix::fs::MetadataExt;
    let mode = std::fs::metadata(&gnupghome_abs).unwrap().mode() & 0o7777;
    assert_eq!(mode, 0o700);
    // .ssh still created too (independent private dir).
    assert_eq!(tb.mode(".ssh"), 0o700);
}

// ---------------------------------------------------------------------------
// private_dirs() -- unit-style coverage of the underlying Rust function
//    (gnupg_dir / private_dirs_all).
// ---------------------------------------------------------------------------

#[test]
fn private_dirs_default_without_gnupghome_uses_dot_gnupg() {
    // No GNUPGHOME set: perms()/auto-private-dirs both resolve the gnupg
    // slot to the literal ".gnupg" -- verified indirectly via CLI effects.
    let tb = TestBed::new("privdirs-default-gnupg");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    let r = tb.radm(&["status"]);
    assert!(r.success());

    assert!(tb.exists(".gnupg"));
    assert!(!tb.exists("alt/gnupghome"));
}

#[test]
fn private_dirs_with_gnupghome_relative_subpath_of_work() {
    // GNUPGHOME under YADM_WORK: private_dirs' "gnupg" slot becomes the
    // subpath relative to YADM_WORK (e.g. "alt/gnupghome"), not ".gnupg".
    let tb = TestBed::new("privdirs-gnupghome-subpath");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    let gnupghome_abs = tb.home_path("alt/gnupghome");
    let r = tb.radm_env(&["status"], "GNUPGHOME", &gnupghome_abs.to_string_lossy());
    assert!(r.success());

    assert!(
        gnupghome_abs.is_dir(),
        "relocated gnupg dir must be created"
    );
    assert!(
        !tb.exists(".gnupg"),
        "default .gnupg must not be created when GNUPGHOME overrides the slot"
    );
}

// ---------------------------------------------------------------------------
// get_mode / copy_perms -- exercised via template rendering, which is the
//    one real call site of copy_perms in radm (yadm:355 template()).
// ---------------------------------------------------------------------------

#[test]
fn copy_perms_preserves_source_mode_on_rendered_template_output() {
    // template() copies mode from the original file to its processed
    // output; a bare "##template" marker selects the built-in "default"
    // (awk-based) engine, requiring no external interpreter, so this stays
    // hermetic. The "##" alt-marker mechanism only considers *tracked*
    // files (git ls-files), so the template source must be added and
    // committed first.
    let tb = TestBed::new("copy-perms-template");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    tb.write_home_mode(".bashrc##template", "#compat-marker\n", 0o640);
    assert!(tb.radm(&["add", ".bashrc##template"]).success());
    assert!(tb.radm(&["commit", "-m", "add template"]).success());

    let r = tb.radm(&["alt"]);
    assert!(r.success(), "alt failed: {r:?}");
    assert_eq!(r.stderr, "");

    assert!(
        tb.exists(".bashrc"),
        "alt should render the ##template marker"
    );
    assert!(
        !tb.is_symlink(".bashrc"),
        "templated output must be a regular rendered file, not a symlink"
    );
    assert_eq!(
        tb.mode(".bashrc"),
        0o640,
        "copy_perms must carry the source template's mode to the rendered output"
    );
}

#[test]
fn copy_perms_reapplies_source_mode_after_output_mode_drifts() {
    // Render once, then mutate the *source* template's mode and re-render:
    // copy_perms must re-copy the (new) source mode onto the output each
    // time template() runs, regardless of the output's prior mode.
    let tb = TestBed::new("copy-perms-template-redrift");
    let r = tb.radm(&["init"]);
    assert!(r.success());

    tb.write_home_mode(".bashrc##template", "#v1\n", 0o644);
    assert!(tb.radm(&["add", ".bashrc##template"]).success());
    assert!(tb.radm(&["commit", "-m", "add template"]).success());

    let r = tb.radm(&["alt"]);
    assert!(r.success());
    assert_eq!(tb.mode(".bashrc"), 0o644);

    // Drift the output's mode away, then change source content+mode and
    // re-render -- copy_perms should reassert the source's current mode.
    std::fs::set_permissions(
        tb.home_path(".bashrc"),
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )
    .unwrap();
    tb.write_home_mode(".bashrc##template", "#v2\n", 0o755);

    let r = tb.radm(&["alt"]);
    assert!(r.success());
    assert_eq!(
        tb.mode(".bashrc"),
        0o755,
        "re-render must copy the (changed) source mode onto the output"
    );
}
