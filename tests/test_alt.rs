//! CLI integration tests for alternate-file processing. The scoring
//! algorithms are unit-tested in src/alt.rs; this file drives the compiled
//! `ryadm` binary end-to-end and asserts on stdout/stderr text, symlink
//! targets, file contents, modes, and exclude-file/status side effects.

mod common;
use common::*;

// ---------------------------------------------------------------------
// Condition scoring precedence, end-to-end via `yadm alt`
// ---------------------------------------------------------------------

/// Two candidates for the same target: a bare `##default` (score 1000) and a
/// `##class.X` match (score 1016) when local.class is set to X. The
/// higher-weight condition must win on disk (symlink source + content).
#[test]
fn precedence_higher_weight_condition_wins_on_disk() {
    let tb = TestBed::new("alt-precedence");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());
    assert!(tb
        .ryadm(&["config", "--add", "local.class", "winclass"])
        .success());

    tb.write_home("f1##default", "default-content\n");
    tb.write_home("f1##class.winclass", "class-content\n");
    assert!(tb
        .ryadm(&["add", "f1##default", "f1##class.winclass"])
        .success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(tb.is_symlink("f1"));
    assert_eq!(tb.link_target("f1").unwrap(), "f1##class.winclass");
    assert_eq!(tb.read_home("f1"), "class-content\n");
}

/// A combined-conditions alt (class + os, score 1000+16 + 1000+2 = 2018)
/// outscores a single-condition alt (class only, 1016) for the same target.
#[test]
fn precedence_combined_conditions_beat_single_condition() {
    let tb = TestBed::new("alt-combo-precedence");
    let os = host_os();
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());
    assert!(tb
        .ryadm(&["config", "--add", "local.class", "winclass"])
        .success());

    tb.write_home("f1##class.winclass", "single-content\n");
    let combo_name = format!("f1##class.winclass,os.{os}");
    tb.write_home(&combo_name, "combo-content\n");
    assert!(tb
        .ryadm(&["add", "f1##class.winclass", &combo_name])
        .success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert_eq!(tb.link_target("f1").unwrap(), combo_name);
    assert_eq!(tb.read_home("f1"), "combo-content\n");
}

// ---------------------------------------------------------------------
// Negation
// ---------------------------------------------------------------------

#[test]
fn negated_condition_that_does_not_match_still_links() {
    let tb = TestBed::new("alt-negate-nomatch");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());

    tb.write_home("f1##~os.BogusOS", "neg-content\n");
    assert!(tb.ryadm(&["add", "f1##~os.BogusOS"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(tb.is_symlink("f1"));
    assert_eq!(tb.link_target("f1").unwrap(), "f1##~os.BogusOS");
    assert_eq!(tb.read_home("f1"), "neg-content\n");
}

#[test]
fn negated_condition_that_matches_aborts_and_never_links() {
    let tb = TestBed::new("alt-negate-match");
    let os = host_os();
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());

    let name = format!("f2##~os.{os}");
    tb.write_home(&name, "neg2-content\n");
    assert!(tb.ryadm(&["add", &name]).success());
    assert!(tb.ryadm(&["commit", "-m", "t2"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(!tb.exists("f2"));
}

// ---------------------------------------------------------------------
// Invalid alt WARNING (stderr only, empty stdout)
// ---------------------------------------------------------------------

#[test]
fn invalid_alt_warns_on_stderr_only() {
    let tb = TestBed::new("alt-invalid-warning");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());

    tb.write_home("f1##invalid", "content\n");
    assert!(tb.ryadm(&["add", "f1##invalid"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert_eq!(r.stdout, "", "add-driven/loud alt: stdout unaffected here");
    // the invalid alternate is reported on stderr, not stdout
    let src = tb.home_path("f1##invalid");
    assert!(r.stderr.contains(&*src.to_string_lossy()));
}

#[test]
fn valid_alts_produce_no_warning() {
    let tb = TestBed::new("alt-no-warning");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());

    tb.write_home("f1##default", "content\n");
    assert!(tb.ryadm(&["add", "f1##default"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
}

// ---------------------------------------------------------------------
// ##default
// ---------------------------------------------------------------------

#[test]
fn bare_default_condition_always_links() {
    let tb = TestBed::new("alt-bare-default");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());

    tb.write_home("f1##default", "default content\n");
    assert!(tb.ryadm(&["add", "f1##default"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(r.out_contains("Linking"));
    assert!(tb.is_symlink("f1"));
    assert_eq!(tb.read_home("f1"), "default content\n");
}

// ---------------------------------------------------------------------
// Combined conditions (multiple conditions on one alt, all must match)
// ---------------------------------------------------------------------

#[test]
fn combined_conditions_all_must_match_to_link() {
    let tb = TestBed::new("alt-combined-conditions");
    let os = host_os();
    let host = host_hostname_short();
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());

    // matches: os matches AND hostname matches
    let good = format!("t9##os.{os},hostname.{host}");
    tb.write_home(&good, "combo good\n");
    // fails: os matches but hostname is bogus -> whole alt invalid, no link
    let bad = format!("t9b##os.{os},hostname.BogusHost");
    tb.write_home(&bad, "combo bad\n");

    assert!(tb.ryadm(&["add", &good, &bad]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(tb.is_symlink("t9"));
    assert_eq!(tb.link_target("t9").unwrap(), good);
    assert!(!tb.exists("t9b"));
}

// ---------------------------------------------------------------------
// Dir alts (##cond/inner)
// ---------------------------------------------------------------------

#[test]
fn directory_alt_links_contained_file_relative() {
    let tb = TestBed::new("alt-dir-inner");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());

    tb.write_home("mydir##default/inner", "inner content\n");
    assert!(tb.ryadm(&["add", "mydir##default/inner"]).success());
    assert!(tb.ryadm(&["commit", "-m", "dir"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(tb.is_symlink("mydir/inner"));
    assert_eq!(
        tb.link_target("mydir/inner").unwrap(),
        "../mydir##default/inner"
    );
    assert_eq!(tb.read_home("mydir/inner"), "inner content\n");
}

/// A pre-2.0 legacy symlink sitting directly at the directory-alt's target
/// path (the whole dir is a symlink to the alt-suffixed dir) must be removed
/// and replaced by correct per-file symlinks inside the (now real) directory.
#[test]
fn legacy_dir_symlink_is_replaced_by_per_file_symlinks() {
    let tb = TestBed::new("alt-legacy-dir-link");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());

    tb.write_home("legacydir##default/inner", "legacy content\n");
    assert!(tb.ryadm(&["add", "legacydir##default/inner"]).success());
    assert!(tb.ryadm(&["commit", "-m", "legacy"]).success());

    // Simulate a pre-2.0 whole-directory symlink at the target path.
    std::os::unix::fs::symlink(
        tb.home_path("legacydir##default"),
        tb.home_path("legacydir"),
    )
    .unwrap();
    assert!(tb.is_symlink("legacydir"));

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(
        !tb.is_symlink("legacydir"),
        "legacy dir symlink must be removed"
    );
    assert!(tb.home_path("legacydir").is_dir());
    assert!(tb.is_symlink("legacydir/inner"));
    assert_eq!(tb.read_home("legacydir/inner"), "legacy content\n");
}

// ---------------------------------------------------------------------
// alt-copy: file copied, not symlinked
// ---------------------------------------------------------------------

#[test]
fn alt_copy_true_copies_regular_file_not_symlink() {
    let tb = TestBed::new("alt-copy-file");
    let os = host_os();
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.alt-copy", "true"]).success());

    let name = format!("c1##os.{os}");
    tb.write_home(&name, "test_alt_copy content\n");
    assert!(tb.ryadm(&["add", &name]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert!(r.out_contains("Copying"));
    assert!(!tb.is_symlink("c1"));
    assert_eq!(tb.read_home("c1"), "test_alt_copy content\n");
}

#[test]
fn alt_copy_unset_links_instead_of_copying() {
    let tb = TestBed::new("alt-copy-unset");
    let os = host_os();
    assert!(tb.ryadm(&["init"]).success());

    let name = format!("c1##os.{os}");
    tb.write_home(&name, "test_alt_copy content\n");
    assert!(tb.ryadm(&["add", &name]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert!(r.out_contains("Linking"));
    assert!(tb.is_symlink("c1"));
    assert_eq!(tb.read_home("c1"), "test_alt_copy content\n");
}

#[test]
fn alt_copy_false_links_instead_of_copying() {
    let tb = TestBed::new("alt-copy-false");
    let os = host_os();
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.alt-copy", "false"]).success());

    let name = format!("c1##os.{os}");
    tb.write_home(&name, "test_alt_copy content\n");
    assert!(tb.ryadm(&["add", &name]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert!(r.out_contains("Linking"));
    assert!(tb.is_symlink("c1"));
}

#[test]
fn alt_copy_true_overwrites_pre_existing_symlink() {
    let tb = TestBed::new("alt-copy-preexisting-symlink");
    let os = host_os();
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.alt-copy", "true"]).success());

    let name = format!("c1##os.{os}");
    tb.write_home(&name, "test_alt_copy content\n");
    assert!(tb.ryadm(&["add", &name]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    // pre-existing symlink pointing elsewhere (auto-alt from `add`/`commit`
    // above may already have linked c1, so remove whatever is there first)
    let _ = std::fs::remove_file(tb.home_path("c1"));
    std::os::unix::fs::symlink("/nonexistent", tb.home_path("c1")).unwrap();
    assert!(tb.is_symlink("c1"));

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert!(r.out_contains("Copying"));
    assert!(!tb.is_symlink("c1"));
    assert_eq!(tb.read_home("c1"), "test_alt_copy content\n");
}

#[test]
fn alt_copy_true_overwrites_pre_existing_regular_file() {
    let tb = TestBed::new("alt-copy-preexisting-file");
    let os = host_os();
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.alt-copy", "true"]).success());

    let name = format!("c1##os.{os}");
    tb.write_home(&name, "test_alt_copy content\n");
    assert!(tb.ryadm(&["add", &name]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    tb.write_home("c1", "wrong content\n");

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert!(r.out_contains("Copying"));
    assert!(!tb.is_symlink("c1"));
    assert_eq!(tb.read_home("c1"), "test_alt_copy content\n");
}

/// alt-copy applied to a directory alt: contained file must be a copy, not a
/// symlink.
#[test]
fn alt_copy_true_copies_directory_alt_contained_file() {
    let tb = TestBed::new("alt-copy-dir");
    let os = host_os();
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.alt-copy", "true"]).success());

    let name = format!("dir1##os.{os}/inner");
    tb.write_home(&name, "inner content\n");
    assert!(tb.ryadm(&["add", &name]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(!tb.is_symlink("dir1/inner"));
    assert_eq!(tb.read_home("dir1/inner"), "inner content\n");
}

// ---------------------------------------------------------------------
// Files inside .config/yadm/alt/ mapping to $HOME
// ---------------------------------------------------------------------

#[test]
fn alt_dir_source_maps_to_home_relative_target() {
    let tb = TestBed::new("alt-source-dir-mapping");
    assert!(tb.ryadm(&["init"]).success());

    tb.write_home(".config/yadm/alt/f1##default", "alt content\n");
    let r = tb.ryadm(&["add", ".config/yadm/alt/f1##default"]);
    assert!(r.success());

    assert!(tb.is_symlink("f1"));
    assert_eq!(
        tb.link_target("f1").unwrap(),
        ".config/yadm/alt/f1##default"
    );
    assert_eq!(tb.read_home("f1"), "alt content\n");
}

/// A nested alt source under `.config/yadm/alt/` must have all intermediate
/// target directories created before symlinking/templating.
#[test]
fn ensure_alt_path_creates_intermediate_dirs_symlink_style() {
    let tb = TestBed::new("alt-ensure-path-symlink");
    assert!(tb.ryadm(&["init"]).success());

    tb.write_home(".config/yadm/alt/a/b/c/file##default", "test-data\n");
    let r = tb.ryadm(&["add", ".config/yadm/alt/a/b/c/file##default"]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");
    assert_eq!(tb.read_home("a/b/c/file").trim(), "test-data");
}

#[test]
fn ensure_alt_path_creates_intermediate_dirs_template_style() {
    let tb = TestBed::new("alt-ensure-path-template");
    assert!(tb.ryadm(&["init"]).success());

    tb.write_home(".config/yadm/alt/a/b/c/file##template", "test-data\n");
    let r = tb.ryadm(&["add", ".config/yadm/alt/a/b/c/file##template"]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");
    assert_eq!(tb.read_home("a/b/c/file").trim(), "test-data");
}

// ---------------------------------------------------------------------
// Untracked ##files not linked
// ---------------------------------------------------------------------

#[test]
fn untracked_alt_file_not_covered_by_encrypt_is_never_linked() {
    let tb = TestBed::new("alt-untracked-skip");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());

    // Not added/committed, no encrypt file: must never be processed.
    tb.write_home("f3##default", "untracked\n");
    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(!tb.exists("f3"));
}

// ---------------------------------------------------------------------
// Encrypt-included / exclude-negated untracked alt sources
// ---------------------------------------------------------------------

#[test]
fn encrypt_included_untracked_alt_file_is_linked() {
    let tb = TestBed::new("alt-encrypt-included");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());

    tb.write_home(".config/yadm/encrypt", "f2##default\n");
    tb.write_home("f2##default", "content-untracked\n");

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(tb.is_symlink("f2"));
    assert_eq!(tb.read_home("f2"), "content-untracked\n");
}

#[test]
fn encrypt_include_negated_by_exclude_pattern_is_not_linked() {
    let tb = TestBed::new("alt-encrypt-excluded");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());

    tb.write_home(".config/yadm/encrypt", "f2##default\n!f2##default\n");
    tb.write_home("f2##default", "content-untracked\n");

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(!tb.exists("f2"));
}

// ---------------------------------------------------------------------
// Exclude file: repo.git/info/exclude managed block content
// ---------------------------------------------------------------------

#[test]
fn alt_updates_exclude_yadm_alt_managed_block() {
    let tb = TestBed::new("alt-exclude-managed-block");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());

    tb.write_home("f1##default", "content\n");
    assert!(tb.ryadm(&["add", "f1##default"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());

    let part = std::fs::read_to_string(tb.repo().join("info/exclude.yadm-alt")).unwrap();
    assert_eq!(part, "/f1\n");

    let exclude = std::fs::read_to_string(tb.repo().join("info/exclude")).unwrap();
    assert!(exclude.contains("# yadm-auto-excludes"));
    assert!(exclude.contains("# yadm alt\n/f1"));
}

#[test]
fn alt_exclude_status_flag_default_and_true_are_ignored() {
    for autoexclude in [None, Some("true")] {
        let name = format!("alt-exclude-status-{autoexclude:?}");
        let tb = TestBed::new(&name);
        assert!(tb.ryadm(&["init"]).success());
        assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());
        if let Some(v) = autoexclude {
            assert!(tb.ryadm(&["config", "yadm.auto-exclude", v]).success());
        }

        tb.write_home("f1##default", "content\n");
        assert!(tb.ryadm(&["add", "f1##default"]).success());
        assert!(tb.ryadm(&["commit", "-m", "t"]).success());
        assert!(tb.ryadm(&["alt"]).success());

        let r = tb.ryadm(&["status", "-z", "-uall", "--ignored"]);
        assert!(r.success());
        let lines: Vec<&str> = r.stdout.split('\0').collect();
        assert!(
            lines.contains(&"!! f1"),
            "expected '!! f1' (ignored) in status output for {autoexclude:?}, got: {:?}",
            lines
        );
    }
}

#[test]
fn alt_exclude_status_flag_false_is_untracked_not_ignored() {
    let tb = TestBed::new("alt-exclude-status-false");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());
    assert!(tb
        .ryadm(&["config", "yadm.auto-exclude", "false"])
        .success());

    tb.write_home("f1##default", "content\n");
    assert!(tb.ryadm(&["add", "f1##default"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());
    assert!(tb.ryadm(&["alt"]).success());

    assert!(!tb.repo().join("info/exclude.yadm-alt").exists());

    let r = tb.ryadm(&["status", "-z", "-uall", "--ignored"]);
    assert!(r.success());
    let lines: Vec<&str> = r.stdout.split('\0').collect();
    assert!(
        lines.contains(&"?? f1"),
        "expected '?? f1' (untracked) in status output, got: {:?}",
        lines
    );
}

// ---------------------------------------------------------------------
// Auto-alt on git commands vs disabled via yadm.auto-alt false
// ---------------------------------------------------------------------

#[test]
fn auto_alt_runs_quietly_on_status_when_unset() {
    let tb = TestBed::new("auto-alt-unset");
    assert!(tb.ryadm(&["init"]).success());

    tb.write_home("f1##default", "content\n");
    assert!(tb.ryadm(&["add", "f1##default"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    // `status` is a git passthrough command; auto-alt should run but quietly
    // (loud is gated on YADM_COMMAND == "alt").
    let r = tb.ryadm(&["status"]);
    assert!(r.success());
    assert!(!r.out_contains("Linking"));
    assert!(!r.out_contains("Creating"));
    assert!(!r.out_contains("Copying"));
    assert_eq!(r.stderr, "");

    assert!(tb.is_symlink("f1"));
    assert_eq!(tb.read_home("f1"), "content\n");
}

#[test]
fn auto_alt_runs_quietly_on_status_when_explicitly_true() {
    let tb = TestBed::new("auto-alt-true");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "true"]).success());

    tb.write_home("f1##default", "content\n");
    assert!(tb.ryadm(&["add", "f1##default"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["status"]);
    assert!(r.success());
    assert!(!r.out_contains("Linking"));

    assert!(tb.is_symlink("f1"));
}

#[test]
fn auto_alt_disabled_leaves_targets_unlinked() {
    let tb = TestBed::new("auto-alt-false");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());

    tb.write_home("f1##default", "content\n");
    assert!(tb.ryadm(&["add", "f1##default"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["status"]);
    assert!(r.success());
    assert!(!tb.exists("f1"), "auto-alt=false must not link on status");

    // Explicit `yadm alt` still works regardless of auto-alt setting.
    let r2 = tb.ryadm(&["alt"]);
    assert!(r2.success());
    assert!(tb.is_symlink("f1"));
}

// ---------------------------------------------------------------------
// local.class / local.os / local.hostname / local.user / local.arch
// overrides affecting selection
// ---------------------------------------------------------------------

#[test]
fn local_os_override_affects_selection() {
    let tb = TestBed::new("alt-local-os-override");
    let real_os = host_os();
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());
    assert!(tb.ryadm(&["config", "local.os", "FakeOS"]).success());

    let fake = "f1##os.FakeOS";
    let real = format!("f1##os.{real_os}");
    tb.write_home(fake, "content-fake\n");
    tb.write_home(&real, "content-real\n");
    assert!(tb.ryadm(&["add", fake, &real]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert_eq!(tb.link_target("f1").unwrap(), fake);
    assert_eq!(tb.read_home("f1"), "content-fake\n");
}

#[test]
fn local_arch_override_affects_selection() {
    let tb = TestBed::new("alt-local-arch-override");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());
    assert!(tb.ryadm(&["config", "local.arch", "FakeArch"]).success());

    tb.write_home("f2##arch.FakeArch", "content-arch\n");
    assert!(tb.ryadm(&["add", "f2##arch.FakeArch"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(tb.is_symlink("f2"));
    assert_eq!(tb.link_target("f2").unwrap(), "f2##arch.FakeArch");
}

#[test]
fn local_hostname_override_affects_selection() {
    let tb = TestBed::new("alt-local-hostname-override");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());
    assert!(tb
        .ryadm(&["config", "local.hostname", "fakehost"])
        .success());

    tb.write_home("f1##hostname.fakehost", "content\n");
    assert!(tb.ryadm(&["add", "f1##hostname.fakehost"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(tb.is_symlink("f1"));
    assert_eq!(tb.link_target("f1").unwrap(), "f1##hostname.fakehost");
}

#[test]
fn local_user_override_affects_selection() {
    let tb = TestBed::new("alt-local-user-override");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());
    assert!(tb.ryadm(&["config", "local.user", "fakeuser"]).success());

    tb.write_home("f2##user.fakeuser", "content\n");
    assert!(tb.ryadm(&["add", "f2##user.fakeuser"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(tb.is_symlink("f2"));
    assert_eq!(tb.link_target("f2").unwrap(), "f2##user.fakeuser");
}

// ---------------------------------------------------------------------
// Class case-insensitivity
// ---------------------------------------------------------------------

#[test]
fn class_matching_is_case_insensitive() {
    let tb = TestBed::new("alt-class-case-insensitive");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());
    // Configured class has different case than the alt's condition value.
    assert!(tb
        .ryadm(&["config", "--add", "local.class", "TestClass"])
        .success());

    tb.write_home("f1##class.testclass", "content\n");
    assert!(tb.ryadm(&["add", "f1##class.testclass"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(tb.is_symlink("f1"));
    assert_eq!(tb.link_target("f1").unwrap(), "f1##class.testclass");
}

// ---------------------------------------------------------------------
// Multiple local.class values (config --add local.class), in_list membership
// ---------------------------------------------------------------------

#[test]
fn multiple_local_class_values_all_participate_in_matching() {
    let tb = TestBed::new("alt-multi-class");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());
    assert!(tb
        .ryadm(&["config", "--add", "local.class", "before"])
        .success());
    assert!(tb
        .ryadm(&["config", "--add", "local.class", "testclass"])
        .success());
    assert!(tb
        .ryadm(&["config", "--add", "local.class", "after"])
        .success());

    // "testclass" is the *middle* value (not the last), yet in_list checks
    // the whole array, so it must still match.
    tb.write_home("f1##class.testclass", "content\n");
    assert!(tb.ryadm(&["add", "f1##class.testclass"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(tb.is_symlink("f1"));

    let get_all = tb.ryadm(&["config", "--get-all", "local.class"]);
    assert_eq!(get_all.stdout, "before\ntestclass\nafter\n");
}

/// Stale-link removal: when `local.class` changes such that a previously
/// matching alt no longer matches anything, re-running `alt` must remove
/// the previously created symlink.
#[test]
fn stale_link_removed_when_class_no_longer_matches() {
    let tb = TestBed::new("alt-stale-link-removal");
    assert!(tb.ryadm(&["init"]).success());
    assert!(tb.ryadm(&["config", "yadm.auto-alt", "false"]).success());
    assert!(tb
        .ryadm(&["config", "--add", "local.class", "staleclass"])
        .success());

    tb.write_home("stale##class.staleclass", "content\n");
    assert!(tb.ryadm(&["add", "stale##class.staleclass"]).success());
    assert!(tb.ryadm(&["commit", "-m", "t"]).success());

    assert!(tb.ryadm(&["alt"]).success());
    assert!(tb.is_symlink("stale"));

    assert!(tb
        .ryadm(&["config", "--unset-all", "local.class"])
        .success());
    assert!(tb
        .ryadm(&["config", "--add", "local.class", "somethingelse"])
        .success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    assert!(!tb.exists("stale"), "stale symlink must be removed");
}
