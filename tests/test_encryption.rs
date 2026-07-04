// CLI contract tests for encrypt/decrypt.
//
// All ciphering uses the "openssl wrapper" trick instead of real gpg: a
// tiny shell script that forwards to the system `openssl` binary with a
// fixed passphrase, configured via `yadm.cipher openssl` +
// `yadm.openssl-program <wrapper>`. This keeps every test hermetic,
// parallel-safe, network-free, and free of any gpg-agent/pinentry setup.
//
// Cipher/parse/update_exclude internals are unit-tested in src/; this file
// pins the CLI-level contract: exact stdout, exit codes, on-disk effects.

mod common;
use common::*;

const OPENSSL_WRAPPER: &str = "#!/bin/sh\nexec openssl \"$@\" -pass pass:test\n";

/// Configure `yadm.cipher openssl` + `yadm.openssl-program <wrapper>` using
/// the openssl-wrapper trick, so encrypt/decrypt never touch real gpg.
fn setup_openssl_cipher(tb: &TestBed) -> String {
    tb.write_home_mode(".wrap/openssl-wrap", OPENSSL_WRAPPER, 0o755);
    let wrapper_path = tb.home_path(".wrap/openssl-wrap").display().to_string();
    let r = tb.ryadm(&["config", "yadm.cipher", "openssl"]);
    assert!(r.success(), "config cipher failed: {r:?}");
    let r = tb.ryadm(&["config", "yadm.openssl-program", &wrapper_path]);
    assert!(r.success(), "config openssl-program failed: {r:?}");
    wrapper_path
}

/// The archive defaults to `$YADM_DATA/archive`, which lives outside the
/// yadm work tree/repo and is therefore always untracked. `encrypt`'s
/// "offer to add" prompt always fires in that case; since our
/// test harness detaches from any controlling tty, `read -r answer
/// </dev/tty>` fails and no answer is recorded (equivalent to bash's own
/// behavior when /dev/tty is unavailable), but the prompt text itself is
/// still printed to stdout beforehand. This builds the exact trailing block
/// every successful `encrypt` run (against the default, untracked archive
/// path) is expected to print.
fn offer_to_add_block(tb: &TestBed) -> String {
    let archive = tb.archive().display().to_string();
    format!(
        "Wrote new file: {archive}\n\
It appears that {archive} is not tracked by yadm's repository.\n\
Would you like to add it now? (y/n)\n"
    )
}

/// Same, but the wrapper also appends its full argv (one per line) to a log
/// file so tests can assert on the exact flags passed to `openssl enc`.
fn setup_openssl_cipher_logging(tb: &TestBed, log_path: &str) -> String {
    let wrapper = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> {log_path}\nexec openssl \"$@\" -pass pass:test\n"
    );
    tb.write_home_mode(".wrap/openssl-wrap-log", &wrapper, 0o755);
    let wrapper_path = tb.home_path(".wrap/openssl-wrap-log").display().to_string();
    let r = tb.ryadm(&["config", "yadm.cipher", "openssl"]);
    assert!(r.success());
    let r = tb.ryadm(&["config", "yadm.openssl-program", &wrapper_path]);
    assert!(r.success());
    wrapper_path
}

// ---------------------------------------------------------------------
// `encrypt` command
// ---------------------------------------------------------------------

#[test]
fn encrypt_happy_path_lists_files_in_order_and_writes_archive() {
    let tb = TestBed::new("enc-happy");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);

    tb.write_home(".config/yadm/encrypt", "secret1\nsecret2\n");
    tb.write_home("secret1", "secret one\n");
    tb.write_home("secret2", "secret two\n");

    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success(), "encrypt failed: {r:?}");
    assert_eq!(r.stderr, "", "stderr must be empty on success golden path");

    // git ls-files --others returns entries in git's own sort order, which
    // for these two flat filenames is lexicographic: secret1, secret2.
    let expected = format!(
        "Encrypting the following files:\nsecret1\nsecret2\n\n{}",
        offer_to_add_block(&tb)
    );
    assert_eq!(r.stdout, expected);

    assert!(
        tb.archive().is_file(),
        "archive file must exist after encrypt"
    );
    assert!(std::fs::metadata(tb.archive()).unwrap().len() > 0);
}

#[test]
fn encrypt_empty_include_list_still_writes_archive() {
    let tb = TestBed::new("enc-empty");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);

    // Encrypt file exists but only has comments/blank lines -> zero include
    // files. `tar -c` with no file operands writes an (essentially) empty
    // tar stream to its stdout: on this platform's bsdtar it also exits
    // nonzero and complains "no files or directories specified" on
    // stderr, but since encrypt()'s success is judged solely by the
    // cipher's exit code (the pipeline's last command, matching bash pipe
    // semantics) rather than tar's, the overall command still succeeds and
    // writes an archive. tar's own
    // stderr is inherited (unsuppressed), same as bash yadm, so it is not
    // asserted empty here.
    tb.write_home(".config/yadm/encrypt", "# nothing to encrypt\n\n");

    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success(), "encrypt failed: {r:?}");
    let expected = format!(
        "Encrypting the following files:\n\n{}",
        offer_to_add_block(&tb)
    );
    assert_eq!(r.stdout, expected);
    assert!(tb.archive().is_file());
}

#[test]
fn encrypt_warns_about_tracked_files_matching_patterns() {
    let tb = TestBed::new("enc-tracked-warn");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);

    tb.write_home(".config/yadm/encrypt", "secret*\n");
    tb.write_home("secret_untracked", "untracked\n");
    tb.write_home("secret_tracked", "tracked\n");
    let r = tb.ryadm(&["add", "secret_tracked"]);
    assert!(r.success());
    let r = tb.ryadm(&["commit", "-m", "track a secret-matching file"]);
    assert!(r.success());

    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success(), "encrypt failed: {r:?}");
    assert_eq!(r.stderr, "");

    let expected = format!(
        "Encrypting the following files:\nsecret_untracked\n\n\
Warning: The following files are tracked and will NOT be encrypted:\nsecret_tracked\n\n{}",
        offer_to_add_block(&tb)
    );
    assert_eq!(r.stdout, expected);
}

#[test]
fn encrypt_missing_encrypt_file_errors_exit_1() {
    let tb = TestBed::new("enc-missing-file");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);
    // .config/yadm/encrypt intentionally never written.

    let r = tb.ryadm(&["encrypt"]);
    assert_eq!(r.code, 1);
    assert!(
        r.err_contains("does not exist"),
        "stderr was: {:?}",
        r.stderr
    );
    assert!(!tb.archive().exists(), "archive must not be created");
}

#[test]
fn encrypt_unknown_cipher_errors_exit_1() {
    let tb = TestBed::new("enc-bad-cipher");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    tb.write_home(".config/yadm/encrypt", "secret1\n");
    tb.write_home("secret1", "x\n");
    let r = tb.ryadm(&["config", "yadm.cipher", "bogus"]);
    assert!(r.success());

    let r = tb.ryadm(&["encrypt"]);
    assert_eq!(r.code, 1);
    assert_eq!(r.stderr, "ERROR: Unknown cipher 'bogus'\n");
}

#[test]
fn encrypt_overwrites_existing_archive_silently() {
    let tb = TestBed::new("enc-overwrite");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);
    tb.write_home(".config/yadm/encrypt", "secret1\n");
    tb.write_home("secret1", "secret one\n");

    // Pre-seed an existing archive with unrelated content.
    std::fs::create_dir_all(tb.archive().parent().unwrap()).unwrap();
    std::fs::write(tb.archive(), "not a real archive").unwrap();

    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success(), "encrypt failed: {r:?}");
    assert_eq!(r.stderr, "");
    let content = std::fs::read(tb.archive()).unwrap();
    assert_ne!(content, b"not a real archive".to_vec());
}

// ---------------------------------------------------------------------
// exclude_encrypted CLI-observable side effect on info/exclude
// ---------------------------------------------------------------------

#[test]
fn encrypt_updates_info_exclude_managed_block_preserving_unmanaged_prefix() {
    let tb = TestBed::new("enc-exclude-block");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);

    // Pre-seed info/exclude with unmanaged content (no trailing newline
    // before yadm would append its own header).
    std::fs::write(tb.repo().join("info/exclude"), "original-data").unwrap();

    tb.write_home(".config/yadm/encrypt", "test-encrypt-data\n");
    tb.write_home("test-encrypt-data", "shh\n");

    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success(), "encrypt failed: {r:?}");
    assert_eq!(r.stderr, "");

    let exclude_content = std::fs::read_to_string(tb.repo().join("info/exclude")).unwrap();
    assert!(
        exclude_content.contains("original-data"),
        "unmanaged prefix must be preserved: {exclude_content:?}"
    );
    assert!(exclude_content.contains("# yadm-auto-excludes"));
    assert!(exclude_content.contains("# yadm encrypt"));
    assert!(exclude_content.contains("/test-encrypt-data"));
}

#[test]
fn encrypt_with_auto_exclude_false_suppresses_managed_block() {
    let tb = TestBed::new("enc-auto-exclude-off");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    // `git init` itself always creates a template info/exclude (a handful
    // of commented-out example lines) — capture that baseline so the
    // assertion below is about yadm's managed block, not file existence.
    let baseline_exclude = std::fs::read_to_string(tb.repo().join("info/exclude")).unwrap();

    setup_openssl_cipher(&tb);
    let r = tb.ryadm(&["config", "yadm.auto-exclude", "false"]);
    assert!(r.success());

    tb.write_home(".config/yadm/encrypt", "test-encrypt-data\n");
    tb.write_home("test-encrypt-data", "shh\n");

    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success(), "encrypt failed: {r:?}");
    assert_eq!(r.stderr, "");

    // update_exclude's master switch returns before touching anything: the
    // git-provided template is left completely untouched, no yadm managed
    // block and no side-cache file at all.
    let exclude_after = std::fs::read_to_string(tb.repo().join("info/exclude")).unwrap();
    assert_eq!(
        exclude_after, baseline_exclude,
        "info/exclude must be untouched when yadm.auto-exclude=false"
    );
    assert!(!exclude_after.contains("yadm-auto-excludes"));
    assert!(!tb.repo().join("info/exclude.yadm-encrypt").exists());
}

// ---------------------------------------------------------------------
// `decrypt` command
// ---------------------------------------------------------------------

#[test]
fn decrypt_restores_file_contents_and_permissions() {
    let tb = TestBed::new("dec-restore");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);
    // `encrypt` itself sets CHANGES_POSSIBLE, which auto-triggers `perms`
    // (chmod go-rwx) on every file it just archived — genuine yadm
    // behavior (misc.rs perms(): "include any files we encrypt"), but
    // orthogonal to what this test is pinning (tar/cipher round-trip of
    // the mode bits present at encrypt time). Disable the auto-trigger so
    // the source file's mode is left exactly as written.
    let r = tb.ryadm(&["config", "yadm.auto-perms", "false"]);
    assert!(r.success());

    tb.write_home(
        ".config/yadm/encrypt",
        "decrypt1\ndecrypt2\nsubdir/decrypt3\n",
    );
    tb.write_home_mode("decrypt1", "decrypt1", 0o640);
    tb.write_home("decrypt2", "decrypt2");
    tb.write_home("subdir/decrypt3", "subdir/decrypt3");

    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success(), "encrypt failed: {r:?}");
    assert_eq!(
        tb.mode("decrypt1"),
        0o640,
        "auto-perms=false must leave the source file's mode untouched"
    );

    // Remove the plaintext, then confirm decrypt restores it exactly.
    std::fs::remove_file(tb.home_path("decrypt1")).unwrap();
    std::fs::remove_file(tb.home_path("decrypt2")).unwrap();
    std::fs::remove_dir_all(tb.home_path("subdir")).unwrap();

    let r = tb.ryadm(&["decrypt"]);
    assert!(r.success(), "decrypt failed: {r:?}");
    assert!(r.stdout.contains("All files decrypted."));

    assert_eq!(tb.read_home("decrypt1"), "decrypt1");
    assert_eq!(tb.read_home("decrypt2"), "decrypt2");
    assert_eq!(tb.read_home("subdir/decrypt3"), "subdir/decrypt3");
    assert_eq!(tb.mode("decrypt1"), 0o640, "permissions must round-trip");
}

#[test]
fn decrypt_extract_overwrites_existing_files_unconditionally() {
    let tb = TestBed::new("dec-overwrite");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);

    tb.write_home(".config/yadm/encrypt", "decrypt1\n");
    tb.write_home("decrypt1", "decrypt1");
    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success());

    // Overwrite the plaintext with different content before decrypting.
    tb.write_home("decrypt1", "pre-existing file");

    let r = tb.ryadm(&["decrypt"]);
    assert!(r.success(), "decrypt failed: {r:?}");
    assert_eq!(tb.read_home("decrypt1"), "decrypt1");
}

#[test]
fn decrypt_list_mode_lists_without_extracting() {
    let tb = TestBed::new("dec-list");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);

    tb.write_home(".config/yadm/encrypt", "decrypt1\ndecrypt2\n");
    tb.write_home("decrypt1", "decrypt1");
    tb.write_home("decrypt2", "decrypt2");
    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success());

    std::fs::remove_file(tb.home_path("decrypt1")).unwrap();
    std::fs::remove_file(tb.home_path("decrypt2")).unwrap();

    let r = tb.ryadm(&["decrypt", "-l"]);
    assert!(r.success(), "decrypt -l failed: {r:?}");
    // list mode prints no "All files decrypted." trailer.
    assert!(!r.stdout.contains("All files decrypted."));
    assert!(r.stdout.contains("decrypt1"));
    assert!(r.stdout.contains("decrypt2"));

    // No filesystem writes happened.
    assert!(!tb.exists("decrypt1"));
    assert!(!tb.exists("decrypt2"));
}

#[test]
fn decrypt_missing_archive_errors_exit_1() {
    let tb = TestBed::new("dec-missing-archive");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);
    // archive intentionally never created.

    let r = tb.ryadm(&["decrypt"]);
    assert_eq!(r.code, 1);
    assert!(
        r.err_contains("does not exist"),
        "stderr was: {:?}",
        r.stderr
    );
}

#[test]
fn decrypt_unknown_cipher_errors_exit_1() {
    let tb = TestBed::new("dec-bad-cipher");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);
    tb.write_home(".config/yadm/encrypt", "secret1\n");
    tb.write_home("secret1", "x\n");
    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success());

    // Switch to a bogus cipher after a valid archive already exists.
    let r = tb.ryadm(&["config", "yadm.cipher", "bogus"]);
    assert!(r.success());

    let r = tb.ryadm(&["decrypt"]);
    assert_eq!(r.code, 1);
    assert_eq!(r.stderr, "ERROR: Unknown cipher 'bogus'\n");
}

#[test]
fn decrypt_wrong_passphrase_fails_with_extract_error() {
    let tb = TestBed::new("dec-wrong-pass");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);
    tb.write_home(".config/yadm/encrypt", "secret1\n");
    tb.write_home("secret1", "x\n");
    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success());

    // Reconfigure with a wrapper using a different passphrase: the archive
    // was encrypted with "test", decrypting with "wrong" must fail the
    // cipher, which yields yadm's own wrapped error message.
    let wrong_wrapper = "#!/bin/sh\nexec openssl \"$@\" -pass pass:wrong\n";
    tb.write_home_mode(".wrap/openssl-wrong", wrong_wrapper, 0o755);
    let wrapper_path = tb.home_path(".wrap/openssl-wrong").display().to_string();
    let r = tb.ryadm(&["config", "yadm.openssl-program", &wrapper_path]);
    assert!(r.success());

    let r = tb.ryadm(&["decrypt"]);
    assert_eq!(r.code, 1);
    // The cipher's own stderr (openssl's decrypt-failure diagnostics) is
    // inherited/passed through unchanged, same as real yadm does for
    // gpg/openssl — it must not be swallowed or rewritten. yadm's own
    // wrapped message is appended after it.
    assert!(
        r.stderr
            .ends_with("ERROR: Unable to extract encrypted files.\n"),
        "stderr was: {:?}",
        r.stderr
    );
}

// ---------------------------------------------------------------------
// glob/exclude semantics as exercised through the real `encrypt` CLI
// (parse_encrypt's matcher itself is already unit-tested in src/encrypt.rs;
// these confirm the end-to-end CLI file list + info/exclude wiring).
// ---------------------------------------------------------------------

#[test]
fn encrypt_honors_exclude_bang_patterns() {
    let tb = TestBed::new("enc-exclude-glob");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);

    tb.write_home(".config/yadm/encrypt", "exclude*\n!*.ex\n");
    tb.write_home("exclude_file1", "1");
    tb.write_home("exclude_file2.ex", "2");
    tb.write_home("exclude_file3.ex3", "3");

    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success(), "encrypt failed: {r:?}");
    assert_eq!(r.stderr, "");
    assert!(r.stdout.contains("exclude_file1"));
    assert!(r.stdout.contains("exclude_file3.ex3"));
    assert!(
        !r.stdout.contains("exclude_file2.ex"),
        "excluded file leaked into output: {:?}",
        r.stdout
    );
}

#[test]
fn encrypt_ignores_comments_and_blank_lines() {
    let tb = TestBed::new("enc-comments");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);

    tb.write_home(
        ".config/yadm/encrypt",
        "\n# a simple comment\n    # a comment with leading space\nsimple_file\n",
    );
    tb.write_home("simple_file", "x");

    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success(), "encrypt failed: {r:?}");
    assert_eq!(r.stderr, "");
    let expected = format!(
        "Encrypting the following files:\nsimple_file\n\n{}",
        offer_to_add_block(&tb)
    );
    assert_eq!(r.stdout, expected);
}

#[test]
fn encrypt_honors_doublestar_globs() {
    let tb = TestBed::new("enc-doublestar");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);

    tb.write_home(".config/yadm/encrypt", "doublestar/**/file*\n!**/file3\n");
    tb.write_home("doublestar/a/b/file1", "1");
    tb.write_home("doublestar/c/d/file2", "2");
    tb.write_home("doublestar/e/f/file3", "3");
    tb.write_home("doublestar/g/h/nomatch", "x");

    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success(), "encrypt failed: {r:?}");
    assert_eq!(r.stderr, "");
    assert!(r.stdout.contains("doublestar/a/b/file1"));
    assert!(r.stdout.contains("doublestar/c/d/file2"));
    assert!(!r.stdout.contains("doublestar/e/f/file3"));
    assert!(!r.stdout.contains("nomatch"));
}

// ---------------------------------------------------------------------
// OpenSSL options assembly — CLI-observable via wrapper argv logging.
// ---------------------------------------------------------------------

#[test]
fn openssl_modern_default_uses_pbkdf2_sha512() {
    let tb = TestBed::new("enc-ossl-modern");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    let log_path = tb.home_path("openssl.log").display().to_string();
    setup_openssl_cipher_logging(&tb, &log_path);

    tb.write_home(".config/yadm/encrypt", "secret1\n");
    tb.write_home("secret1", "x\n");

    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success(), "encrypt failed: {r:?}");

    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("-pbkdf2"), "log was: {log:?}");
    assert!(log.contains("-iter"));
    assert!(log.contains("100000"));
    assert!(log.contains("-md"));
    assert!(log.contains("sha512"));
    assert!(
        !log.contains("md5"),
        "modern default must not use -md md5: {log:?}"
    );
}

#[test]
fn openssl_old_config_switches_to_md5_no_pbkdf2() {
    let tb = TestBed::new("enc-ossl-old");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    let log_path = tb.home_path("openssl.log").display().to_string();
    setup_openssl_cipher_logging(&tb, &log_path);
    let r = tb.ryadm(&["config", "yadm.openssl-old", "true"]);
    assert!(r.success());

    tb.write_home(".config/yadm/encrypt", "secret1\n");
    tb.write_home("secret1", "x\n");

    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success(), "encrypt failed: {r:?}");

    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("md5"), "log was: {log:?}");
    assert!(
        !log.contains("-pbkdf2"),
        "openssl-old must not use pbkdf2: {log:?}"
    );
    assert!(!log.contains("sha512"));
}

#[test]
fn openssl_old_archive_round_trips_through_decrypt() {
    // Confirms the legacy -md md5 path is not just argv-shape but actually
    // decryptable: encrypt and decrypt both under yadm.openssl-old=true.
    let tb = TestBed::new("enc-ossl-old-roundtrip");
    let r = tb.ryadm(&["init"]);
    assert!(r.success());
    setup_openssl_cipher(&tb);
    let r = tb.ryadm(&["config", "yadm.openssl-old", "true"]);
    assert!(r.success());

    tb.write_home(".config/yadm/encrypt", "secret1\n");
    tb.write_home("secret1", "legacy content\n");

    let r = tb.ryadm(&["encrypt"]);
    assert!(r.success(), "encrypt failed: {r:?}");

    std::fs::remove_file(tb.home_path("secret1")).unwrap();
    let r = tb.ryadm(&["decrypt"]);
    assert!(r.success(), "decrypt failed: {r:?}");
    assert_eq!(tb.read_home("secret1"), "legacy content\n");
}
