//! encrypt/decrypt commands, encrypt-file parsing and encrypted excludes.
//! Reference: yadm script lines 928-1084 (ciphers, encrypt, decrypt),
//! 1555-1572 (exclude_encrypted), 1970-2017 (parse_encrypt).
//! Spec: scratchpad specs encryption.md.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::config;
use crate::context::Context;
use crate::exclude;
use crate::git;
use crate::paths;
use crate::util;

/// yadm's classification regex `^[[:blank:]]*(#|$)`: a line is a
/// comment/blank line when, after stripping leading spaces/tabs, it is
/// either empty or starts with '#'.
fn is_comment_or_blank(line: &str) -> bool {
    let stripped = line.trim_start_matches([' ', '\t']);
    stripped.is_empty() || stripped.starts_with('#')
}

/// yadm's `${filename%/}` — strip exactly one trailing '/'.
fn strip_one_trailing_slash(s: &str) -> String {
    s.strip_suffix('/').unwrap_or(s).to_string()
}

// ---------------------------------------------------------------------
// require_* prerequisites (yadm:2179-2223)
// ---------------------------------------------------------------------

/// yadm's require_archive (yadm:2179-2181).
pub fn require_archive(ctx: &Context) {
    if !Path::new(&ctx.archive).is_file() {
        util::error_out(
            ctx,
            &format!(
                "{} does not exist. did you forget to create it?",
                ctx.archive
            ),
        );
    }
}

/// yadm's require_encrypt (yadm:2182-2184).
pub fn require_encrypt(ctx: &Context) {
    if !Path::new(&ctx.encrypt_file).is_file() {
        util::error_out(
            ctx,
            &format!(
                "{} does not exist. did you forget to create it?",
                ctx.encrypt_file
            ),
        );
    }
}

/// yadm's require_gpg (yadm:2198-2210): may override ctx.gpg_program from
/// yadm.gpg-program config.
pub fn require_gpg(ctx: &mut Context) {
    let alt_gpg = config::config_output(ctx, &["yadm.gpg-program"]);
    let mut more_info = String::new();
    if !alt_gpg.is_empty() {
        ctx.gpg_program = alt_gpg;
        more_info =
            "\\nThis command has been set via the yadm.gpg-program configuration.".to_string();
    }
    if !util::command_exists(&ctx.gpg_program) {
        util::error_out(
            ctx,
            &format!(
                "This functionality requires GPG to be installed, but the command '{}' cannot be located.{}",
                ctx.gpg_program, more_info
            ),
        );
    }
}

/// yadm's require_openssl (yadm:2211-2223): may override ctx.openssl_program
/// from yadm.openssl-program config.
pub fn require_openssl(ctx: &mut Context) {
    let alt_openssl = config::config_output(ctx, &["yadm.openssl-program"]);
    let mut more_info = String::new();
    if !alt_openssl.is_empty() {
        ctx.openssl_program = alt_openssl;
        more_info =
            "\\nThis command has been set via the yadm.openssl-program configuration.".to_string();
    }
    if !util::command_exists(&ctx.openssl_program) {
        util::error_out(
            ctx,
            &format!(
                "This functionality requires OpenSSL to be installed, but the command '{}' cannot be located.{}",
                ctx.openssl_program, more_info
            ),
        );
    }
}

// ---------------------------------------------------------------------
// Cipher option assembly (yadm:928-966)
// ---------------------------------------------------------------------

/// yadm's _get_cipher (yadm:960-966): config yadm.cipher, default "gpg".
fn get_cipher(ctx: &Context) -> String {
    let cipher = config::config_output(ctx, &["yadm.cipher"]);
    if cipher.is_empty() {
        "gpg".to_string()
    } else {
        cipher
    }
}

/// yadm's _set_gpg_options (yadm:928-940).
fn set_gpg_options(ctx: &Context) -> Vec<String> {
    let gpg_key = config::config_output(ctx, &["yadm.gpg-recipient"]);
    if gpg_key == "ASK" {
        vec!["--no-default-recipient".to_string(), "-e".to_string()]
    } else if !gpg_key.is_empty() {
        let mut opts = vec!["-e".to_string()];
        // bash: `for key in $gpg_key` word-splits on IFS whitespace, and each
        // "-r $key" is a SINGLE array element containing an embedded space.
        for key in gpg_key.split_whitespace() {
            opts.push(format!("-r {key}"));
        }
        opts
    } else {
        vec!["-c".to_string()]
    }
}

/// yadm's _get_openssl_ciphername (yadm:942-948).
fn get_openssl_ciphername(ctx: &Context) -> String {
    let name = config::config_output(ctx, &["yadm.openssl-ciphername"]);
    if name.is_empty() {
        "aes-256-cbc".to_string()
    } else {
        name
    }
}

/// yadm's _set_openssl_options (yadm:950-958).
fn set_openssl_options(ctx: &Context) -> Vec<String> {
    let cipher_name = get_openssl_ciphername(ctx);
    let mut opts = vec![format!("-{cipher_name}"), "-salt".to_string()];
    let openssl_old = config::config_output(ctx, &["--bool", "yadm.openssl-old"]);
    if openssl_old == "true" {
        opts.push("-md".to_string());
        opts.push("md5".to_string());
    } else {
        opts.push("-pbkdf2".to_string());
        opts.push("-iter".to_string());
        opts.push("100000".to_string());
        opts.push("-md".to_string());
        opts.push("sha512".to_string());
    }
    opts
}

// ---------------------------------------------------------------------
// exclude_encrypted (yadm:1555-1572)
// ---------------------------------------------------------------------

/// yadm's exclude_encrypted: mirror the encrypt file into the managed
/// info/exclude block.
pub fn exclude_encrypted(ctx: &Context) {
    let mut entries: Vec<String> = Vec::new();

    // `-r` (readable) check, distinct from parse_encrypt's `-f` (regular
    // file) check: an unreadable-but-existing file yields an empty list
    // here, same as a missing file.
    if is_readable(&ctx.encrypt_file) {
        if let Some(lines) = util::read_lines(&ctx.encrypt_file) {
            for pattern in lines {
                if let Some(rest) = pattern.strip_prefix('!') {
                    entries.push(format!("!/{rest}"));
                } else if !is_comment_or_blank(&pattern) {
                    entries.push(format!("/{pattern}"));
                }
            }
        }
    }

    // update_exclude is called unconditionally, even with an empty list.
    exclude::update_exclude(ctx, "encrypt", &entries);
}

fn is_readable(path: &str) -> bool {
    std::fs::File::open(path).is_ok()
}

// ---------------------------------------------------------------------
// parse_encrypt (yadm:1970-2017)
// ---------------------------------------------------------------------

/// yadm's parse_encrypt: populate ctx.encrypt_include_files (once) and,
/// for the encrypt command, ctx.no_encrypt_tracked_files.
pub fn parse_encrypt(ctx: &mut Context) {
    // Memoization: yadm's "unparsed" sentinel maps to encrypt_include_files
    // being None. Once parsed (even to an empty vec), never reprocess.
    if ctx.encrypt_include_files.is_some() {
        return;
    }

    ctx.encrypt_include_files = Some(Vec::new());

    if !Path::new(&ctx.encrypt_file).is_file() {
        return;
    }

    if !paths::cd_work(ctx, "Parsing encrypt") {
        return;
    }

    let lines = match util::read_lines(&ctx.encrypt_file) {
        Some(l) => l,
        None => return,
    };

    let mut exclude: Vec<String> = Vec::new();
    let mut include: Vec<String> = Vec::new();

    for pattern in lines {
        if let Some(rest) = pattern.strip_prefix('!') {
            exclude.push(format!("--exclude=/{rest}"));
        } else if !is_comment_or_blank(&pattern) {
            include.push(pattern);
        }
    }

    if include.is_empty() {
        return;
    }

    let mut args: Vec<&str> = vec!["--glob-pathspecs", "ls-files", "--others"];
    for e in &exclude {
        args.push(e.as_str());
    }
    args.push("--");
    for i in &include {
        args.push(i.as_str());
    }
    let (out, _) = git::capture(ctx, &args, true);
    let mut include_files: Vec<String> = Vec::new();
    for filename in out.split('\n') {
        if !filename.is_empty() {
            include_files.push(strip_one_trailing_slash(filename));
        }
    }
    ctx.encrypt_include_files = Some(include_files);

    if ctx.yadm_command != "encrypt" {
        return;
    }

    let mut args2: Vec<&str> = vec!["--glob-pathspecs", "ls-files"];
    for e in &exclude {
        args2.push(e.as_str());
    }
    args2.push("--");
    for i in &include {
        args2.push(i.as_str());
    }
    // stderr is NOT suppressed here (asymmetric with the --others query).
    let (out2, _) = git::capture(ctx, &args2, false);
    for filename in out2.split('\n') {
        if !filename.is_empty() {
            ctx.no_encrypt_tracked_files
                .push(strip_one_trailing_slash(filename));
        }
    }
}

// ---------------------------------------------------------------------
// Cipher dispatch used by encrypt()/decrypt() (yadm:968-1019)
// ---------------------------------------------------------------------

/// Build the argv (excluding the program name) for encrypting to `archive`.
/// Mutates ctx via require_gpg/require_openssl (program overrides).
fn encrypt_to_argv(ctx: &mut Context, archive: &str) -> (String, Vec<String>) {
    let cipher = get_cipher(ctx);
    match cipher.as_str() {
        "gpg" => {
            require_gpg(ctx);
            let opts = set_gpg_options(ctx);
            let mut argv = vec!["--yes".to_string()];
            argv.extend(opts);
            argv.push("--output".to_string());
            argv.push(archive.to_string());
            (ctx.gpg_program.clone(), argv)
        }
        "openssl" => {
            require_openssl(ctx);
            let opts = set_openssl_options(ctx);
            let mut argv = vec!["enc".to_string(), "-e".to_string()];
            argv.extend(opts);
            argv.push("-out".to_string());
            argv.push(archive.to_string());
            (ctx.openssl_program.clone(), argv)
        }
        other => {
            util::error_out(ctx, &format!("Unknown cipher '{other}'"));
        }
    }
}

/// Build the argv (excluding the program name) for decrypting `archive`.
fn decrypt_from_argv(ctx: &mut Context, archive: &str) -> (String, Vec<String>) {
    let cipher = get_cipher(ctx);
    match cipher.as_str() {
        "gpg" => {
            require_gpg(ctx);
            (
                ctx.gpg_program.clone(),
                vec!["-d".to_string(), archive.to_string()],
            )
        }
        "openssl" => {
            require_openssl(ctx);
            let opts = set_openssl_options(ctx);
            let mut argv = vec!["enc".to_string(), "-d".to_string()];
            argv.extend(opts);
            argv.push("-in".to_string());
            argv.push(archive.to_string());
            (ctx.openssl_program.clone(), argv)
        }
        other => {
            util::error_out(ctx, &format!("Unknown cipher '{other}'"));
        }
    }
}

// ---------------------------------------------------------------------
// encrypt (yadm:1044-1084)
// ---------------------------------------------------------------------

pub fn encrypt(ctx: &mut Context) {
    require_encrypt(ctx);
    exclude_encrypted(ctx);
    parse_encrypt(ctx);

    if !paths::cd_work(ctx, "Encryption") {
        return;
    }

    let include_files = ctx.encrypt_include_files.clone().unwrap_or_default();

    println!("Encrypting the following files:");
    for f in &include_files {
        println!("{f}");
    }
    println!();

    if !ctx.no_encrypt_tracked_files.is_empty() {
        println!("Warning: The following files are tracked and will NOT be encrypted:");
        for f in &ctx.no_encrypt_tracked_files {
            println!("{f}");
        }
        println!();
    }

    // tar -f - -c <files...> | _encrypt_to "$YADM_ARCHIVE"
    let archive = ctx.archive.clone();
    let (cipher_program, cipher_args) = encrypt_to_argv(ctx, &archive);

    let mut tar_args: Vec<String> = vec!["-f".to_string(), "-".to_string(), "-c".to_string()];
    tar_args.extend(include_files.iter().cloned());

    let mut tar_child = Command::new("tar")
        .args(&tar_args)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| util::error_out(ctx, &format!("Unable to spawn tar: {e}")));
    let tar_stdout = tar_child.stdout.take().expect("tar stdout piped");

    // The cipher's own stdout/stderr are inherited (gpg may prompt via tty).
    let cipher_status = Command::new(&cipher_program)
        .args(&cipher_args)
        .stdin(Stdio::from(tar_stdout))
        .status();

    // Reap tar; bash pipe status reflects only the last command (_encrypt_to)
    // so tar's own exit code is intentionally not consulted for success.
    let _ = tar_child.wait();

    let success = matches!(cipher_status, Ok(s) if s.success());

    if success {
        println!("Wrote new file: {archive}");
    } else {
        util::error_out(ctx, &format!("Unable to write {archive}"));
    }

    // offer to add YADM_ARCHIVE if untracked
    let mixed_archive = paths::mixed_path(ctx, &archive);
    let (archive_status, _) = git::capture(
        ctx,
        &["status", "--porcelain", "-uall", &mixed_archive],
        true,
    );
    if archive_status.starts_with("??") {
        println!("It appears that {archive} is not tracked by yadm's repository.");
        println!("Would you like to add it now? (y/n)");
        if let Some(answer) = util::read_tty_line() {
            if answer == "y" || answer == "Y" {
                let _ = git::run(ctx, &["add", &mixed_archive]);
            }
        }
    }

    ctx.changes_possible = true;
}

// ---------------------------------------------------------------------
// decrypt (yadm:1021-1042)
// ---------------------------------------------------------------------

pub fn decrypt(ctx: &mut Context) {
    require_archive(ctx);

    if Path::new(&ctx.encrypt_file).is_file() {
        exclude_encrypted(ctx);
    }

    let tar_option = if ctx.do_list { "t" } else { "x" };

    let archive = ctx.archive.clone();
    let (cipher_program, cipher_args) = decrypt_from_argv(ctx, &archive);

    // (_decrypt_from "$YADM_ARCHIVE" || echo 1) | tar v${tar_option}f - -C "$YADM_WORK"
    //
    // We can't stream this as a true zero-buffering pipeline while also
    // knowing whether to append the "1\n" fallback (that decision depends
    // on the cipher's *exit code*, which is only known after all of its
    // stdout has been produced). Instead: capture the cipher's stdout in
    // full, then feed tar the captured bytes (plus "1\n" appended on
    // failure), matching the subshell's observable behavior exactly.
    let mut cipher_child = Command::new(&cipher_program)
        .args(&cipher_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|e| util::error_out(ctx, &format!("Unable to spawn cipher: {e}")));
    let mut cipher_stdout = cipher_child.stdout.take().expect("cipher stdout piped");
    let mut captured = Vec::new();
    let _ = cipher_stdout.read_to_end(&mut captured);
    let cipher_status = cipher_child.wait();
    let cipher_success = matches!(cipher_status, Ok(s) if s.success());
    if !cipher_success {
        captured.extend_from_slice(b"1\n");
    }

    let tar_arg = format!("v{tar_option}f");
    let mut tar_child = Command::new("tar")
        .args([tar_arg.as_str(), "-", "-C", &ctx.work])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| util::error_out(ctx, &format!("Unable to spawn tar: {e}")));
    {
        let mut tar_stdin = tar_child.stdin.take().expect("tar stdin piped");
        let _ = tar_stdin.write_all(&captured);
        // drop tar_stdin here to close the pipe before waiting
    }
    let tar_status = tar_child.wait();
    let success = matches!(tar_status, Ok(s) if s.success());

    if success {
        if !ctx.do_list {
            println!("All files decrypted.");
        }
    } else {
        util::error_out(ctx, "Unable to extract encrypted files.");
    }

    ctx.changes_possible = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    // Tests that call parse_encrypt() mutate process-global state (GIT_DIR
    // env var, and the process's CWD via cd_work) and must not run
    // concurrently with each other or with themselves across threads.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_dir(name: &str) -> String {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "radm-encrypt-test-{}-{}-{}",
            std::process::id(),
            name,
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().into_owned()
    }

    fn base_ctx(work: &str) -> Context {
        let mut ctx = Context::new();
        ctx.work = work.to_string();
        ctx.encrypt_file = format!("{work}/.yadm/encrypt");
        ctx
    }

    fn git_init(work: &str) {
        // GIT_DIR may still be set (process-global) from a previous test's
        // call to parse_encrypt(); remove it so `git init` targets `work`
        // rather than the stale repo.
        let status = Command::new("git")
            .env_remove("GIT_DIR")
            .args(["init", "-q", work])
            .status()
            .expect("git init");
        assert!(status.success());
        // ensure a usable identity for potential future commits in this repo
        let _ = Command::new("git")
            .env_remove("GIT_DIR")
            .current_dir(work)
            .args(["config", "user.email", "test@example.com"])
            .status();
        let _ = Command::new("git")
            .env_remove("GIT_DIR")
            .current_dir(work)
            .args(["config", "user.name", "Test"])
            .status();
    }

    #[test]
    fn comment_and_blank_line_classification() {
        assert!(is_comment_or_blank(""));
        assert!(is_comment_or_blank("#comment"));
        assert!(is_comment_or_blank("    # a comment with leading space"));
        assert!(is_comment_or_blank("\t#tabbed"));
        assert!(is_comment_or_blank("   "));
        assert!(!is_comment_or_blank("simple_file"));
        assert!(!is_comment_or_blank("  not_a_comment"));
    }

    #[test]
    fn strip_one_trailing_slash_only_strips_one() {
        assert_eq!(strip_one_trailing_slash("dir/"), "dir");
        assert_eq!(strip_one_trailing_slash("dir//"), "dir/");
        assert_eq!(strip_one_trailing_slash("file"), "file");
    }

    #[test]
    fn parse_encrypt_missing_file_returns_empty() {
        let _guard = ENV_LOCK.lock().unwrap();
        let work = tmp_dir("missing");
        git_init(&work);
        let mut ctx = base_ctx(&work);
        // GIT_DIR must point at the repo for git::capture's subprocess.
        std::env::set_var("GIT_DIR", format!("{work}/.git"));

        parse_encrypt(&mut ctx);
        assert_eq!(ctx.encrypt_include_files, Some(vec![]));
        assert!(ctx.no_encrypt_tracked_files.is_empty());
    }

    #[test]
    fn parse_encrypt_empty_file_returns_empty() {
        let _guard = ENV_LOCK.lock().unwrap();
        let work = tmp_dir("empty");
        git_init(&work);
        std::fs::create_dir_all(format!("{work}/.yadm")).unwrap();
        std::fs::write(format!("{work}/.yadm/encrypt"), "").unwrap();
        let mut ctx = base_ctx(&work);
        std::env::set_var("GIT_DIR", format!("{work}/.git"));

        parse_encrypt(&mut ctx);
        assert_eq!(ctx.encrypt_include_files, Some(vec![]));
    }

    #[test]
    fn parse_encrypt_memoizes_after_first_call() {
        let _guard = ENV_LOCK.lock().unwrap();
        let work = tmp_dir("memo");
        git_init(&work);
        let mut ctx = base_ctx(&work);
        std::env::set_var("GIT_DIR", format!("{work}/.git"));

        parse_encrypt(&mut ctx);
        // Force a marker into the result to prove the 2nd call is a no-op.
        ctx.encrypt_include_files = Some(vec!["marker".to_string()]);
        parse_encrypt(&mut ctx);
        assert_eq!(ctx.encrypt_include_files, Some(vec!["marker".to_string()]));
    }

    #[test]
    fn parse_encrypt_include_and_exclude_globs() {
        let _guard = ENV_LOCK.lock().unwrap();
        let work = tmp_dir("globs");
        git_init(&work);
        std::fs::create_dir_all(format!("{work}/.yadm")).unwrap();
        std::fs::write(
            format!("{work}/.yadm/encrypt"),
            "simple_file\nwild*\n!excluded_file\n",
        )
        .unwrap();
        std::fs::write(format!("{work}/simple_file"), "x").unwrap();
        std::fs::write(format!("{work}/wildcard1"), "x").unwrap();
        std::fs::write(format!("{work}/excluded_file"), "x").unwrap();
        // wild* would match excluded_file too, since it doesn't start with
        // "wild" it won't; but wildcard1 matches "wild*".
        let mut ctx = base_ctx(&work);
        std::env::set_var("GIT_DIR", format!("{work}/.git"));

        parse_encrypt(&mut ctx);
        let mut got = ctx.encrypt_include_files.clone().unwrap();
        got.sort();
        assert_eq!(
            got,
            vec!["simple_file".to_string(), "wildcard1".to_string()]
        );
    }

    #[test]
    fn parse_encrypt_comment_and_blank_lines_ignored() {
        let _guard = ENV_LOCK.lock().unwrap();
        let work = tmp_dir("comments");
        git_init(&work);
        std::fs::create_dir_all(format!("{work}/.yadm")).unwrap();
        std::fs::write(
            format!("{work}/.yadm/encrypt"),
            "\n# a simple comment\n    # indented comment\nsimple_file\n",
        )
        .unwrap();
        std::fs::write(format!("{work}/simple_file"), "x").unwrap();
        let mut ctx = base_ctx(&work);
        std::env::set_var("GIT_DIR", format!("{work}/.git"));

        parse_encrypt(&mut ctx);
        assert_eq!(
            ctx.encrypt_include_files,
            Some(vec!["simple_file".to_string()])
        );
    }

    #[test]
    fn parse_encrypt_tracked_files_only_for_encrypt_command() {
        let _guard = ENV_LOCK.lock().unwrap();
        let work = tmp_dir("tracked");
        git_init(&work);
        std::fs::create_dir_all(format!("{work}/.yadm")).unwrap();
        std::fs::write(format!("{work}/.yadm/encrypt"), "tracked_file\n").unwrap();
        std::fs::write(format!("{work}/tracked_file"), "x").unwrap();
        std::env::set_var("GIT_DIR", format!("{work}/.git"));
        let status = Command::new("git")
            .current_dir(&work)
            .args(["add", "tracked_file"])
            .status()
            .unwrap();
        assert!(status.success());

        let mut ctx = base_ctx(&work);
        ctx.yadm_command = "encrypt".to_string();
        parse_encrypt(&mut ctx);
        // tracked_file is tracked, so it should NOT show up as an "others"
        // (untracked) include, but SHOULD show up in no_encrypt_tracked_files.
        assert_eq!(ctx.encrypt_include_files, Some(vec![]));
        assert_eq!(
            ctx.no_encrypt_tracked_files,
            vec!["tracked_file".to_string()]
        );
    }

    #[test]
    fn parse_encrypt_skips_tracked_query_when_not_encrypt_command() {
        let _guard = ENV_LOCK.lock().unwrap();
        let work = tmp_dir("nontracked");
        git_init(&work);
        std::fs::create_dir_all(format!("{work}/.yadm")).unwrap();
        std::fs::write(format!("{work}/.yadm/encrypt"), "tracked_file\n").unwrap();
        std::fs::write(format!("{work}/tracked_file"), "x").unwrap();
        std::env::set_var("GIT_DIR", format!("{work}/.git"));
        let status = Command::new("git")
            .current_dir(&work)
            .args(["add", "tracked_file"])
            .status()
            .unwrap();
        assert!(status.success());

        let mut ctx = base_ctx(&work);
        // yadm_command left empty ("" != "encrypt")
        parse_encrypt(&mut ctx);
        assert_eq!(ctx.encrypt_include_files, Some(vec![]));
        assert!(ctx.no_encrypt_tracked_files.is_empty());
    }

    #[test]
    fn set_gpg_options_ask() {
        let work = tmp_dir("gpg-ask");
        std::fs::create_dir_all(&work).unwrap();
        let mut ctx = Context::new();
        ctx.config_file = format!("{work}/config");
        std::fs::write(&ctx.config_file, "[yadm]\n\tgpg-recipient = ASK\n").unwrap();
        let opts = set_gpg_options(&ctx);
        assert_eq!(opts, vec!["--no-default-recipient", "-e"]);
    }

    #[test]
    fn set_gpg_options_present_recipient() {
        let work = tmp_dir("gpg-present");
        std::fs::create_dir_all(&work).unwrap();
        let mut ctx = Context::new();
        ctx.config_file = format!("{work}/config");
        std::fs::write(&ctx.config_file, "[yadm]\n\tgpg-recipient = present\n").unwrap();
        let opts = set_gpg_options(&ctx);
        assert_eq!(opts, vec!["-e", "-r present"]);
    }

    #[test]
    fn set_gpg_options_multi_recipient() {
        let work = tmp_dir("gpg-multi");
        std::fs::create_dir_all(&work).unwrap();
        let mut ctx = Context::new();
        ctx.config_file = format!("{work}/config");
        std::fs::write(
            &ctx.config_file,
            "[yadm]\n\tgpg-recipient = second-key yadm-test1\n",
        )
        .unwrap();
        let opts = set_gpg_options(&ctx);
        assert_eq!(opts, vec!["-e", "-r second-key", "-r yadm-test1"]);
    }

    #[test]
    fn set_gpg_options_empty_recipient_is_symmetric() {
        let work = tmp_dir("gpg-empty");
        std::fs::create_dir_all(&work).unwrap();
        let mut ctx = Context::new();
        ctx.config_file = format!("{work}/config");
        std::fs::write(&ctx.config_file, "").unwrap();
        let opts = set_gpg_options(&ctx);
        assert_eq!(opts, vec!["-c"]);
    }

    #[test]
    fn get_cipher_defaults_to_gpg() {
        let work = tmp_dir("cipher-default");
        std::fs::create_dir_all(&work).unwrap();
        let mut ctx = Context::new();
        ctx.config_file = format!("{work}/config");
        std::fs::write(&ctx.config_file, "").unwrap();
        assert_eq!(get_cipher(&ctx), "gpg");
    }

    #[test]
    fn get_cipher_override() {
        let work = tmp_dir("cipher-override");
        std::fs::create_dir_all(&work).unwrap();
        let mut ctx = Context::new();
        ctx.config_file = format!("{work}/config");
        std::fs::write(&ctx.config_file, "[yadm]\n\tcipher = override-cipher\n").unwrap();
        assert_eq!(get_cipher(&ctx), "override-cipher");
    }

    #[test]
    fn get_openssl_ciphername_default() {
        let work = tmp_dir("ossl-default");
        std::fs::create_dir_all(&work).unwrap();
        let mut ctx = Context::new();
        ctx.config_file = format!("{work}/config");
        std::fs::write(&ctx.config_file, "").unwrap();
        assert_eq!(get_openssl_ciphername(&ctx), "aes-256-cbc");
    }

    #[test]
    fn get_openssl_ciphername_override() {
        let work = tmp_dir("ossl-override");
        std::fs::create_dir_all(&work).unwrap();
        let mut ctx = Context::new();
        ctx.config_file = format!("{work}/config");
        std::fs::write(
            &ctx.config_file,
            "[yadm]\n\topenssl-ciphername = override-cipher\n",
        )
        .unwrap();
        assert_eq!(get_openssl_ciphername(&ctx), "override-cipher");
    }

    #[test]
    fn set_openssl_options_default_pbkdf2() {
        let work = tmp_dir("ossl-opts-default");
        std::fs::create_dir_all(&work).unwrap();
        let mut ctx = Context::new();
        ctx.config_file = format!("{work}/config");
        std::fs::write(
            &ctx.config_file,
            "[yadm]\n\topenssl-ciphername = testcipher\n",
        )
        .unwrap();
        let opts = set_openssl_options(&ctx);
        assert_eq!(
            opts,
            vec![
                "-testcipher",
                "-salt",
                "-pbkdf2",
                "-iter",
                "100000",
                "-md",
                "sha512"
            ]
        );
    }

    #[test]
    fn set_openssl_options_old_md5() {
        let work = tmp_dir("ossl-opts-old");
        std::fs::create_dir_all(&work).unwrap();
        let mut ctx = Context::new();
        ctx.config_file = format!("{work}/config");
        std::fs::write(
            &ctx.config_file,
            "[yadm]\n\topenssl-ciphername = testcipher\n\topenssl-old = true\n",
        )
        .unwrap();
        let opts = set_openssl_options(&ctx);
        assert_eq!(opts, vec!["-testcipher", "-salt", "-md", "md5"]);
    }
}
