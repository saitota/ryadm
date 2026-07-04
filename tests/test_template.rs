//! CLI-level compatibility tests for the yadm template engine, as exercised
//! through `ryadm alt`. Engine internals (the awk-based `default` processor's
//! if/else/include/substitution grammar) are unit-tested in
//! `src/template_default.rs` and are intentionally NOT re-tested here — this
//! file only pins the CLI-observable contract: how `##template`/`##seed` alt
//! files resolve, render, get excluded, and interact with the filesystem.

mod common;
use common::*;

/// `ryadm init` plus disabling `yadm.auto-alt`, so that `add`/`commit` in the
/// tests below don't implicitly pre-render templates via the automatic
/// post-command alt pass (yadm runs `alt` automatically after any command
/// that might have changed tracked files) — every render in this file is
/// then driven by an explicit `ryadm alt` call, matching how the ported
/// pytest cases invoke `yadm alt` directly.
fn init_no_auto_alt(tb: &TestBed) {
    let r = tb.ryadm(&["init"]);
    assert!(r.success(), "init failed: {r:?}");
    let r = tb.ryadm(&["config", "yadm.auto-alt", "false"]);
    assert!(r.success(), "config failed: {r:?}");
}

/// `##template` (and its `t`/`yadm` synonyms) renders the source into the
/// target using `local.*` config as the template variables, and the target
/// ends up excluded from git status the same as a symlinked alt would.
#[test]
fn template_renders_target_from_local_config_vars() {
    let tb = TestBed::new("tpl-basic");
    init_no_auto_alt(&tb);
    let r = tb.ryadm(&["config", "local.user", "testuser123"]);
    assert!(r.success(), "config failed: {r:?}");

    tb.write_home("greeting.txt##template", "hello {{yadm.user}}\n");
    let r = tb.ryadm(&["add", "greeting.txt##template"]);
    assert!(r.success(), "add failed: {r:?}");
    let r = tb.ryadm(&["commit", "-m", "add template"]);
    assert!(r.success(), "commit failed: {r:?}");

    let r = tb.ryadm(&["alt"]);
    assert!(r.success(), "alt failed: {r:?}");
    assert_eq!(r.stderr, "");

    assert_eq!(tb.read_home("greeting.txt"), "hello testuser123\n");
}

/// The three template-label synonyms (`t`, `template`, `yadm`) behave
/// identically: all render on every `alt` run (unlike seed).
#[test]
fn template_label_synonyms_all_render() {
    let tb = TestBed::new("tpl-synonyms");
    init_no_auto_alt(&tb);
    let r = tb.ryadm(&["config", "local.user", "syn"]);
    assert!(r.success());

    for (idx, label) in ["t", "template", "yadm"].iter().enumerate() {
        let name = format!("file{idx}.txt##{label}");
        tb.write_home(&name, "value={{yadm.user}}\n");
        let r = tb.ryadm(&["add", &name]);
        assert!(r.success(), "add {name} failed: {r:?}");
    }
    let r = tb.ryadm(&["commit", "-m", "add templates"]);
    assert!(r.success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success(), "alt failed: {r:?}");
    for idx in 0..3 {
        assert_eq!(tb.read_home(&format!("file{idx}.txt")), "value=syn\n");
    }
}

/// `yadm alt` run directly ("loud") prints the exact `Creating <output> from
/// template <input>` line to stdout for a freshly (re-)rendered template.
#[test]
fn template_prints_creating_message_when_loud() {
    let tb = TestBed::new("tpl-loud");
    init_no_auto_alt(&tb);
    tb.write_home("out.txt##template", "static content\n");
    let r = tb.ryadm(&["add", "out.txt##template"]);
    assert!(r.success());
    let r = tb.ryadm(&["commit", "-m", "add"]);
    assert!(r.success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());
    let source = tb.home_path("out.txt##template");
    let target = tb.home_path("out.txt");
    let expected = format!(
        "Creating {} from template {}",
        target.display(),
        source.display()
    );
    assert!(
        r.stdout.contains(&expected),
        "expected stdout to contain {expected:?}, got {:?}",
        r.stdout
    );
}

/// A pre-existing non-template alternate (would-be symlink/copy target) gets
/// superseded once the alt source carries a `##template` condition: the
/// scoring engine records the template processor for that target and
/// `alt_linking` renders instead of symlinking, and the on-disk result is a
/// regular file (not a symlink).
#[test]
fn template_overrides_plain_alt_symlink_behavior() {
    let tb = TestBed::new("tpl-override-symlink");
    init_no_auto_alt(&tb);
    tb.write_home("conf##template", "rendered={{yadm.class}}\n");
    let r = tb.ryadm(&["add", "conf##template"]);
    assert!(r.success());
    let r = tb.ryadm(&["commit", "-m", "add"]);
    assert!(r.success());
    let r = tb.ryadm(&["config", "local.class", "myclass"]);
    assert!(r.success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success(), "alt failed: {r:?}");

    assert!(
        !tb.is_symlink("conf"),
        "template target must not be a symlink"
    );
    assert_eq!(tb.read_home("conf"), "rendered=myclass\n");
}

// ---------------------------------------------------------------------
// Seed (`##s` / `##seed`): render once, then never overwrite existing target.
// ---------------------------------------------------------------------

/// A `##seed` alt renders on the first `alt` run (target absent), then is
/// completely left alone on every subsequent run even if both the seed
/// source and the target are later modified — the target keeps whatever
/// content it had after the first render.
#[test]
fn seed_renders_once_then_never_overwrites() {
    let tb = TestBed::new("tpl-seed-once");
    init_no_auto_alt(&tb);
    tb.write_home("seedme.txt##seed", "seeded content\n");
    let r = tb.ryadm(&["add", "seedme.txt##seed"]);
    assert!(r.success());
    let r = tb.ryadm(&["commit", "-m", "add"]);
    assert!(r.success());

    // First run: target doesn't exist yet -> renders.
    let r1 = tb.ryadm(&["alt"]);
    assert!(r1.success(), "first alt failed: {r1:?}");
    assert!(
        r1.stdout.contains("Creating "),
        "first run should render: {:?}",
        r1.stdout
    );
    assert_eq!(tb.read_home("seedme.txt"), "seeded content\n");

    // User (or a prior process) edits the rendered target directly, and the
    // seed source itself is also changed on disk afterward.
    tb.write_home("seedme.txt", "seeded content EDITED BY USER\n");
    tb.write_home("seedme.txt##seed", "seeded content CHANGED SOURCE\n");

    // Second run: target now exists -> the seed guard skips rendering
    // entirely, producing zero "Creating" lines for this file.
    let r2 = tb.ryadm(&["alt"]);
    assert!(r2.success(), "second alt failed: {r2:?}");
    assert!(
        !r2.stdout.contains("Creating "),
        "second run must not re-render a seeded target: {:?}",
        r2.stdout
    );
    assert_eq!(
        tb.read_home("seedme.txt"),
        "seeded content EDITED BY USER\n",
        "seeded target must be left exactly as the user/prior run left it"
    );
}

/// `##s` is the short synonym for `##seed` and follows the identical
/// once-only rule.
#[test]
fn seed_short_label_behaves_like_seed() {
    let tb = TestBed::new("tpl-seed-short");
    init_no_auto_alt(&tb);
    tb.write_home("s.txt##s", "first\n");
    let r = tb.ryadm(&["add", "s.txt##s"]);
    assert!(r.success());
    let r = tb.ryadm(&["commit", "-m", "add"]);
    assert!(r.success());

    let r1 = tb.ryadm(&["alt"]);
    assert!(r1.success());
    assert_eq!(tb.read_home("s.txt"), "first\n");

    tb.write_home("s.txt##s", "second\n");
    let r2 = tb.ryadm(&["alt"]);
    assert!(r2.success());
    assert_eq!(tb.read_home("s.txt"), "first\n");
}

// ---------------------------------------------------------------------
// Unchanged output is not rewritten (no-op skip, compared via mtime).
// ---------------------------------------------------------------------

/// When the rendered content is byte-identical (modulo the trailing-newline
/// normalization the dispatcher performs) to what's already on disk, the
/// target's mtime must not change across a second `alt` run — proving the
/// "Template output is unchanged" no-op path, not just a content re-check.
#[test]
fn unchanged_template_output_is_not_rewritten() {
    let tb = TestBed::new("tpl-unchanged-mtime");
    init_no_auto_alt(&tb);
    // A template whose rendered output never varies across runs (no
    // per-run-varying substitution such as timestamps).
    tb.write_home("stable.txt##template", "constant line\n");
    let r = tb.ryadm(&["add", "stable.txt##template"]);
    assert!(r.success());
    let r = tb.ryadm(&["commit", "-m", "add"]);
    assert!(r.success());

    let r1 = tb.ryadm(&["alt"]);
    assert!(r1.success());
    let target = tb.home_path("stable.txt");

    // Sleep isn't allowed; instead force the filesystem's mtime backward far
    // enough that any (incorrect) rewrite would be trivially detectable even
    // on filesystems with coarse mtime granularity.
    let far_past = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    let f = std::fs::File::open(&target).unwrap();
    f.set_modified(far_past).unwrap();
    drop(f);

    let r2 = tb.ryadm(&["alt"]);
    assert!(r2.success(), "second alt failed: {r2:?}");
    let mtime2 = std::fs::metadata(&target).unwrap().modified().unwrap();
    assert_eq!(
        mtime2, far_past,
        "unchanged template output must not be rewritten (mtime must survive untouched)"
    );
    assert_eq!(tb.read_home("stable.txt"), "constant line\n");
}

// ---------------------------------------------------------------------
// Template errors: exact "file:line: error: ..." to stderr, target absent.
// ---------------------------------------------------------------------

/// A malformed template (`{% endif %}` with no matching `{% if %}`) makes
/// the awk-based default engine exit non-zero; `template()`'s dispatcher
/// then prints `Error: failed to process template '<input>'` and leaves the
/// target completely absent (no partial file, no empty file). The engine's
/// own `<file>:<line>: error: <text>` diagnostic is also present on stderr.
#[test]
fn template_error_prints_diagnostic_and_leaves_target_absent() {
    let tb = TestBed::new("tpl-error-endif");
    init_no_auto_alt(&tb);
    tb.write_home("bad.txt##template", "{% endif %}\n");
    let r = tb.ryadm(&["add", "bad.txt##template"]);
    assert!(r.success());
    let r = tb.ryadm(&["commit", "-m", "add"]);
    assert!(r.success());

    let r = tb.ryadm(&["alt"]);
    // `yadm alt` itself still exits 0 even though one template failed to
    // render — no error exit propagates from a single failed template source.
    assert!(r.success(), "alt should still exit 0: {r:?}");

    let source = tb.home_path("bad.txt##template");
    assert!(
        r.stderr.contains(&format!(
            "{}:1: error: endif without matching if",
            source.display()
        )),
        "expected exact awk-engine diagnostic in stderr, got: {:?}",
        r.stderr
    );
    assert!(
        r.stderr.contains(&format!(
            "Error: failed to process template '{}'",
            source.display()
        )),
        "expected dispatcher error string in stderr, got: {:?}",
        r.stderr
    );
    assert!(
        !tb.exists("bad.txt"),
        "target must not be created on template error"
    );
}

/// Same as above but for an unopened `{% else %}` — a second distinct
/// error-call-site, still routed through the identical dispatcher error
/// format and still leaving the target absent.
#[test]
fn template_error_else_without_if_leaves_target_absent() {
    let tb = TestBed::new("tpl-error-else");
    init_no_auto_alt(&tb);
    tb.write_home("bad2.txt##template", "{% else %}\n");
    let r = tb.ryadm(&["add", "bad2.txt##template"]);
    assert!(r.success());
    let r = tb.ryadm(&["commit", "-m", "add"]);
    assert!(r.success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());

    let source = tb.home_path("bad2.txt##template");
    assert!(r.stderr.contains(&format!(
        "{}:1: error: else without matching if",
        source.display()
    )));
    assert!(!tb.exists("bad2.txt"));
}

// ---------------------------------------------------------------------
// `{% include %}` relative to the source (top-level input) directory.
// ---------------------------------------------------------------------

/// An `{% include relative/path %}` inside a template resolves relative to
/// the *directory of the top-level template file* (not the target's
/// directory, and not the CWD), even when the template itself lives in a
/// nested directory of the yadm-managed tree.
#[test]
fn include_resolves_relative_to_source_directory() {
    let tb = TestBed::new("tpl-include-reldir");
    init_no_auto_alt(&tb);
    tb.write_home("sub/part.txt", "included text\n");
    tb.write_home(
        "sub/main.txt##template",
        "before\n{% include part.txt %}\nafter\n",
    );
    let r = tb.ryadm(&["add", "sub/part.txt", "sub/main.txt##template"]);
    assert!(r.success(), "add failed: {r:?}");
    let r = tb.ryadm(&["commit", "-m", "add"]);
    assert!(r.success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success(), "alt failed: {r:?}");
    assert_eq!(
        tb.read_home("sub/main.txt"),
        "before\nincluded text\nafter\n"
    );
}

/// Nested-subdirectory include path (`dir/nested`) also resolves relative
/// to the top-level input's directory, proving includes aren't resolved
/// relative to whichever file is currently doing the including.
#[test]
fn include_supports_nested_subdirectory_paths() {
    let tb = TestBed::new("tpl-include-nested-dir");
    init_no_auto_alt(&tb);
    tb.write_home("sub/dir/nested.txt", "nested contents\n");
    tb.write_home(
        "sub/main.txt##template",
        "top\n{% include dir/nested.txt %}\nbottom\n",
    );
    let r = tb.ryadm(&["add", "sub/dir/nested.txt", "sub/main.txt##template"]);
    assert!(r.success());
    let r = tb.ryadm(&["commit", "-m", "add"]);
    assert!(r.success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success(), "alt failed: {r:?}");
    assert_eq!(
        tb.read_home("sub/main.txt"),
        "top\nnested contents\nbottom\n"
    );
}

// ---------------------------------------------------------------------
// Executable bit (and other mode bits) copied from source template
// (copy_perms), overriding whatever mode a pre-existing target had.
// ---------------------------------------------------------------------

/// The rendered target's permission bits end up identical to the *source*
/// template's mode (0o754, including the executable bit), not whatever mode
/// a pre-existing target file happened to have (0o600, non-executable).
#[test]
fn executable_bit_copied_from_source_template() {
    let tb = TestBed::new("tpl-exec-bit");
    init_no_auto_alt(&tb);
    tb.write_home_mode(
        "script.sh##template",
        "#!/bin/sh\necho {{yadm.user}}\n",
        0o754,
    );
    let r = tb.ryadm(&["add", "script.sh##template"]);
    assert!(r.success());
    let r = tb.ryadm(&["commit", "-m", "add"]);
    assert!(r.success());
    let r = tb.ryadm(&["config", "local.user", "execuser"]);
    assert!(r.success());

    // Pre-create the target with a different, non-executable mode so we can
    // prove copy_perms overwrites it rather than leaving it alone.
    tb.write_home_mode("script.sh", "old content, different mode\n", 0o600);

    let r = tb.ryadm(&["alt"]);
    assert!(r.success(), "alt failed: {r:?}");

    assert_eq!(
        tb.mode("script.sh"),
        0o754,
        "executable bit must be copied from the source template"
    );
    assert_eq!(tb.read_home("script.sh"), "#!/bin/sh\necho execuser\n");
}

/// A template source with no executable bit at all (0o644) produces a
/// non-executable target, confirming the mode copy is a faithful bit-for-bit
/// copy, not an unconditional chmod +x.
#[test]
fn non_executable_source_mode_is_also_copied_verbatim() {
    let tb = TestBed::new("tpl-nonexec-mode");
    init_no_auto_alt(&tb);
    tb.write_home_mode("plain.txt##template", "plain content\n", 0o644);
    let r = tb.ryadm(&["add", "plain.txt##template"]);
    assert!(r.success());
    let r = tb.ryadm(&["commit", "-m", "add"]);
    assert!(r.success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success());

    assert_eq!(tb.mode("plain.txt"), 0o644);
}

// ---------------------------------------------------------------------
// Alt condition combined with a template label: a mismatched non-template
// condition suppresses an otherwise-valid template render.
// ---------------------------------------------------------------------

/// `##template,arch.<mismatch>` never renders because the `arch` condition's
/// negative delta drops the whole alt candidate before template resolution
/// can contribute anything, even though the label itself is a valid
/// template/kind combination.
#[test]
fn template_condition_combined_with_mismatched_arch_never_renders() {
    let tb = TestBed::new("tpl-combined-mismatch");
    init_no_auto_alt(&tb);
    let real_arch = host_arch();
    let mismatched = format!("not{real_arch}");
    let name = format!("combo.txt##template,arch.{mismatched}");
    tb.write_home(&name, "should never render\n");
    let r = tb.ryadm(&["add", &name]);
    assert!(r.success());
    let r = tb.ryadm(&["commit", "-m", "add"]);
    assert!(r.success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success(), "alt failed: {r:?}");
    assert!(
        !r.stdout.contains("Creating "),
        "mismatched arch condition must suppress the template render: {:?}",
        r.stdout
    );
    assert!(!tb.exists("combo.txt"));
}

/// Conversely, `##template,arch.<match>` (using this host's real arch) does
/// render, proving the combination succeeds when the non-template condition
/// actually matches.
#[test]
fn template_condition_combined_with_matching_arch_renders() {
    let tb = TestBed::new("tpl-combined-match");
    init_no_auto_alt(&tb);
    let real_arch = host_arch();
    let name = format!("combo2.txt##template,arch.{real_arch}");
    tb.write_home(&name, "renders fine\n");
    let r = tb.ryadm(&["add", &name]);
    assert!(r.success());
    let r = tb.ryadm(&["commit", "-m", "add"]);
    assert!(r.success());

    let r = tb.ryadm(&["alt"]);
    assert!(r.success(), "alt failed: {r:?}");
    assert_eq!(tb.read_home("combo2.txt"), "renders fine\n");
}
