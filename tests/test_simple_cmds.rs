//! Ports pytest assertions (spec: simple-cmds.md) for `config`, `enter`,
//! `list`, `introspect`, `help`, `version`, and `yadm.git-program` override
//! into explicit expectation tests (exact strings/exit codes/FS effects),
//! independent of the differential suite in compat_yadm.rs.

mod common;
use common::*;

// ---------------------------------------------------------------------
// 1. `config`
// ---------------------------------------------------------------------

const CONFIG_NO_ARGS_EXPECTED: &str = "\
yadm supports the following configurations:

  local.arch
  local.class
  local.distro
  local.distro-family
  local.hostname
  local.os
  local.user
  yadm.alt-copy
  yadm.auto-alt
  yadm.auto-exclude
  yadm.auto-perms
  yadm.auto-private-dirs
  yadm.cipher
  yadm.git-program
  yadm.gpg-perms
  yadm.gpg-program
  yadm.gpg-recipient
  yadm.openssl-ciphername
  yadm.openssl-old
  yadm.openssl-program
  yadm.ssh-perms

Please read the CONFIGURATION section in the man
page for more details about configurations, and
how to adjust them.

";

#[test]
fn config_no_args_prints_exact_help_text() {
    let tb = TestBed::new("cfg-noargs");
    let r = tb.radm(&["config"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert_eq!(r.stdout, CONFIG_NO_ARGS_EXPECTED);
    // spec 1.2: contains the CONFIGURATION-section pointer and every
    // supported config string (looser containment check preserved too).
    assert!(r.out_contains("Please read the CONFIGURATION section"));
    for cfg in [
        "local.arch",
        "local.class",
        "local.distro",
        "local.distro-family",
        "local.hostname",
        "local.os",
        "local.user",
        "yadm.alt-copy",
        "yadm.auto-alt",
        "yadm.auto-exclude",
        "yadm.auto-perms",
        "yadm.auto-private-dirs",
        "yadm.cipher",
        "yadm.git-program",
        "yadm.gpg-perms",
        "yadm.gpg-program",
        "yadm.gpg-recipient",
        "yadm.openssl-ciphername",
        "yadm.openssl-old",
        "yadm.openssl-program",
        "yadm.ssh-perms",
    ] {
        assert!(
            r.out_contains(cfg),
            "missing {cfg} in config no-args output"
        );
    }
}

#[test]
fn config_read_missing_key_is_silent_success() {
    let tb = TestBed::new("cfg-read-missing");
    let r = tb.radm(&["config", "test.attribute"]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");
}

#[test]
fn config_write_creates_yadm_config_file() {
    let tb = TestBed::new("cfg-write");
    let r = tb.radm(&["config", "test.attribute", "testvalue"]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");
    let content = tb.read_home(".config/yadm/config");
    assert_eq!(content.trim(), "[test]\n\tattribute = testvalue");
}

#[test]
fn config_read_roundtrip() {
    let tb = TestBed::new("cfg-read");
    tb.write_home(".config/yadm/config", "[test]\n\tattribute = testvalue\n");
    let r = tb.radm(&["config", "test.attribute"]);
    assert!(r.success());
    assert_eq!(r.stdout.trim_end_matches('\n'), "testvalue");
    assert_eq!(r.stderr, "");
}

#[test]
fn config_update_overwrites_single_valued_key() {
    let tb = TestBed::new("cfg-update");
    tb.write_home(".config/yadm/config", "[test]\n\tattribute = testvalue\n");
    let r = tb.radm(&["config", "test.attribute", "testvalueextra"]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");
    let content = tb.read_home(".config/yadm/config");
    assert_eq!(content.trim(), "[test]\n\tattribute = testvalueextra");
}

const LOCAL_CONFIGS: [&str; 7] = [
    "local.arch",
    "local.class",
    "local.distro",
    "local.distro-family",
    "local.hostname",
    "local.os",
    "local.user",
];

#[test]
fn config_local_star_keys_read_from_repo_config() {
    let tb = TestBed::new("cfg-local-read");
    tb.init_repo_with(&[]);
    let repo = tb.repo();
    for key in LOCAL_CONFIGS {
        let value = format!("value_of_{key}");
        let r = tb.sh(&format!(
            "GIT_DIR={:?} git config --local {key} {value:?}",
            repo.to_string_lossy(),
        ));
        assert!(r.success(), "seeding {key} failed: {r:?}");

        let r = tb.radm(&["config", key]);
        assert!(r.success(), "config {key} failed: {r:?}");
        assert_eq!(r.stderr, "");
        assert_eq!(r.stdout, format!("{value}\n"));
    }
}

#[test]
fn config_local_star_keys_write_to_repo_config_not_yadm_config() {
    let tb = TestBed::new("cfg-local-write");
    tb.init_repo_with(&[]);
    let repo = tb.repo();
    for key in LOCAL_CONFIGS {
        let value = format!("value_of_{key}");
        let r = tb.radm(&["config", key, &value]);
        assert!(r.success(), "config write {key} failed: {r:?}");
        assert_eq!(r.stdout, "");
        assert_eq!(r.stderr, "");

        // verify via plain `git config` against the repo's GIT_DIR
        let verify = tb.sh(&format!(
            "GIT_DIR={:?} git config {key}",
            repo.to_string_lossy()
        ));
        assert!(verify.success());
        assert_eq!(verify.stdout, format!("{value}\n"));
    }
    // never landed in the yadm config file
    assert!(!tb.exists(".config/yadm/config"));
}

#[test]
fn config_without_parent_directory_creates_nested_dirs() {
    let tb = TestBed::new("cfg-no-parent");
    let cfg_path = tb.home.join("folder/does/not/exist/config");
    let cfg_arg = cfg_path.to_string_lossy().into_owned();

    let r = tb.radm(&[
        "--yadm-config",
        &cfg_arg,
        "config",
        "test.attribute",
        "testvalue",
    ]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");
    assert!(cfg_path.is_file(), "nested config file was not created");

    let r = tb.radm(&["--yadm-config", &cfg_arg, "config", "test.attribute"]);
    assert!(r.success());
    assert_eq!(r.stdout, "testvalue\n");
    assert_eq!(r.stderr, "");
}

#[test]
fn config_dash_l_lists_yadm_config_contents() {
    let tb = TestBed::new("cfg-list");
    let r = tb.radm(&["config", "yadm.auto-perms", "false"]);
    assert!(r.success());
    let r = tb.radm(&["config", "-l"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert!(r.out_contains("yadm.auto-perms=false"));
}

// ---------------------------------------------------------------------
// 4. `introspect`
// ---------------------------------------------------------------------

const INTROSPECT_COMMANDS_EXPECTED: &str = "alt\n\
bootstrap\n\
clean\n\
clone\n\
config\n\
decrypt\n\
encrypt\n\
enter\n\
git-crypt\n\
gitconfig\n\
help\n\
init\n\
introspect\n\
list\n\
perms\n\
transcrypt\n\
upgrade\n\
version";

const INTROSPECT_CONFIGS_EXPECTED: &str = "local.arch\n\
local.class\n\
local.distro\n\
local.distro-family\n\
local.hostname\n\
local.os\n\
local.user\n\
yadm.alt-copy\n\
yadm.auto-alt\n\
yadm.auto-exclude\n\
yadm.auto-perms\n\
yadm.auto-private-dirs\n\
yadm.cipher\n\
yadm.git-program\n\
yadm.gpg-perms\n\
yadm.gpg-program\n\
yadm.gpg-recipient\n\
yadm.openssl-ciphername\n\
yadm.openssl-old\n\
yadm.openssl-program\n\
yadm.ssh-perms";

const INTROSPECT_SWITCHES_EXPECTED: &str = "--yadm-archive\n\
--yadm-bootstrap\n\
--yadm-config\n\
--yadm-data\n\
--yadm-dir\n\
--yadm-encrypt\n\
--yadm-repo\n\
-Y";

#[test]
fn introspect_commands_exact_no_trailing_newline() {
    let tb = TestBed::new("introspect-commands");
    let r = tb.radm(&["introspect", "commands"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert_eq!(r.stdout, INTROSPECT_COMMANDS_EXPECTED);
    assert!(
        !r.stdout.ends_with('\n'),
        "commands output must not end with \\n"
    );
}

#[test]
fn introspect_configs_exact_no_trailing_newline() {
    let tb = TestBed::new("introspect-configs");
    let r = tb.radm(&["introspect", "configs"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert_eq!(r.stdout, INTROSPECT_CONFIGS_EXPECTED);
    assert!(
        !r.stdout.ends_with('\n'),
        "configs output must not end with \\n"
    );
}

#[test]
fn introspect_switches_exact_no_trailing_newline() {
    let tb = TestBed::new("introspect-switches");
    let r = tb.radm(&["introspect", "switches"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert_eq!(r.stdout, INTROSPECT_SWITCHES_EXPECTED);
    assert!(
        !r.stdout.ends_with('\n'),
        "switches output must not end with \\n"
    );
}

#[test]
fn introspect_repo_prints_repo_path_with_trailing_newline() {
    let tb = TestBed::new("introspect-repo");
    tb.init_repo_with(&[]);
    let r = tb.radm(&["introspect", "repo"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert!(r.stdout.ends_with('\n'));
    let repo = tb.repo().to_string_lossy().into_owned();
    assert_eq!(r.stdout.trim_end_matches('\n'), repo);
    // exactly one trailing newline, no more
    assert!(!r.stdout.trim_end_matches('\n').ends_with('\n'));
}

#[test]
fn introspect_unknown_or_no_arg_prints_nothing_exit_0() {
    let tb = TestBed::new("introspect-unknown");

    let r = tb.radm(&["introspect"]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");

    let r = tb.radm(&["introspect", "invalid"]);
    assert!(r.success());
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "");
}

// ---------------------------------------------------------------------
// 5. `help` / no-args / `--help`
// ---------------------------------------------------------------------

/// Build the expected `help` text (paths shown with a literal `$HOME` prefix,
/// per yadm's `${VAR/$HOME/\$HOME}` substitution), using the exact 32-wide
/// column padding rule so this test doesn't drift from the real
/// implementation's formatting logic.
fn build_expected_help() -> String {
    let pad = |s: &str| -> String {
        let padding = 32usize.saturating_sub(s.chars().count());
        format!("{s}{}", " ".repeat(padding))
    };
    let config = pad("$HOME/.config/yadm/config");
    let encrypt = pad("$HOME/.config/yadm/encrypt");
    let bootstrap = pad("$HOME/.config/yadm/bootstrap");
    let repo = pad("$HOME/.local/share/yadm/repo.git");
    let archive = pad("$HOME/.local/share/yadm/archive");
    // Raw string (like the source's own r#"..."#) so leading spaces on each
    // line survive verbatim -- backslash-continuations would strip them.
    format!(
        r#"Usage: yadm <command> [options...]

Manage dotfiles maintained in a Git repository. Manage alternate files
for specific systems or hosts. Encrypt/decrypt private files.

Git Commands:
Any Git command or alias can be used as a <command>. It will operate
on yadm's repository and files in the work tree (usually $HOME).

Commands:
  yadm init [-f]             - Initialize an empty repository
  yadm clone <url> [-f]      - Clone an existing repository
  yadm config <name> <value> - Configure a setting
  yadm list [-a]             - List tracked files
  yadm alt                   - Create links for alternates
  yadm bootstrap             - Execute $HOME/.config/yadm/bootstrap
  yadm encrypt               - Encrypt files
  yadm decrypt [-l]          - Decrypt files
  yadm perms                 - Fix perms for private files
  yadm enter [COMMAND]       - Run sub-shell with GIT variables set
  yadm git-crypt [OPTIONS]   - Run git-crypt commands for the yadm repo
  yadm transcrypt [OPTIONS]  - Run transcrypt commands for the yadm repo

Files:
  {config} - yadm's configuration file
  {encrypt} - List of globs to encrypt/decrypt
  {bootstrap} - Script run via: yadm bootstrap
  {repo} - yadm's Git repository
  {archive} - Encrypted data stored here

Use "man yadm" for complete documentation.

"#
    )
}

#[test]
fn help_command_exact_text_exit_1_stderr_empty() {
    let tb = TestBed::new("help-cmd");
    let expected = build_expected_help();
    let r = tb.radm(&["help"]);
    assert!(!r.success());
    assert_eq!(r.code, 1);
    assert_eq!(r.stderr, "");
    assert_eq!(r.stdout, expected);
    assert!(r.stdout.starts_with("Usage: yadm"));
}

#[test]
fn help_via_double_dash_help_same_as_help() {
    let tb = TestBed::new("help-dashdash");
    let expected = build_expected_help();
    let r = tb.radm(&["--help"]);
    assert!(!r.success());
    assert_eq!(r.code, 1);
    assert_eq!(r.stderr, "");
    assert_eq!(r.stdout, expected);
}

#[test]
fn no_args_behaves_like_help() {
    let tb = TestBed::new("no-args-help");
    let expected = build_expected_help();
    let r = tb.radm(&[]);
    assert!(!r.success());
    assert_eq!(r.code, 1);
    assert_eq!(r.stderr, "");
    assert_eq!(r.stdout, expected);
    assert!(r.stdout.starts_with("Usage: yadm"));
}

// ---------------------------------------------------------------------
// 6. `version` / `--version`
// ---------------------------------------------------------------------

#[test]
fn version_command_shape_and_exit_0() {
    let tb = TestBed::new("version-cmd");
    let r = tb.radm(&["version"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    let mut lines = r.stdout.lines();
    let first = lines.next().unwrap_or_default();
    assert_eq!(first, "radm version 3.5.0");
    // second line: single leading space, then "git version ..."
    let rest = &r.stdout[first.len() + 1..]; // skip first line + its \n
    assert!(
        rest.starts_with(' '),
        "second line must start with a leading space"
    );
    assert!(r.out_contains("git version"));
    assert!(r.stdout.ends_with("\nyadm version 3.5.0\n"));
}

#[test]
fn version_via_double_dash_version_same_shape() {
    let tb = TestBed::new("version-dashdash");
    let r = tb.radm(&["--version"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert!(r.stdout.starts_with("radm version 3.5.0\n"));
    assert!(r.out_contains("git version"));
    assert!(r.stdout.ends_with("\nyadm version 3.5.0\n"));
}

// ---------------------------------------------------------------------
// 2. `enter`
// ---------------------------------------------------------------------

#[test]
fn enter_with_command_exposes_git_work_tree() {
    let tb = TestBed::new("enter-cmd");
    tb.init_repo_with(&[]);
    let r = tb.radm(&["enter", "printenv", "GIT_WORK_TREE"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    assert_eq!(r.stdout.trim_end_matches('\n'), tb.home.to_string_lossy());
    // command mode must not print Entering/Leaving banners
    assert!(!r.stdout.contains("Entering yadm repo"));
    assert!(!r.stdout.contains("Leaving yadm repo"));
}

#[test]
fn enter_without_command_prints_entering_and_leaving() {
    let tb = TestBed::new("enter-nocmd");
    tb.init_repo_with(&[]);
    // TestBed sets SHELL=/bin/bash and detaches the tty, so an interactive
    // bash started with --norc exits immediately (no stdin) after printing
    // the banners.
    let r = tb.radm(&["enter"]);
    assert!(r.stdout.starts_with("Entering yadm repo\n"));
    assert!(r.stdout.trim_end().ends_with("Leaving yadm repo"));
    assert_eq!(r.stderr, "");
}

#[test]
fn enter_bad_shell_errors_does_not_refer_to_executable() {
    let tb = TestBed::new("enter-badshell");
    tb.init_repo_with(&[]);
    let r = tb.radm_env(&["enter"], "SHELL", "/nonexistent-shell-xyz");
    assert!(!r.success());
    assert!(r.err_contains("does not refer to an executable"));
    assert!(r.err_contains("$SHELL does not refer to an executable."));
}

#[test]
fn enter_empty_shell_env_var_errors() {
    let tb = TestBed::new("enter-emptyshell");
    tb.init_repo_with(&[]);
    let r = tb.radm_env(&["enter"], "SHELL", "");
    assert!(!r.success());
    assert!(r.err_contains("does not refer to an executable"));
}

#[test]
fn enter_non_executable_shell_file_errors() {
    let tb = TestBed::new("enter-noexec");
    tb.init_repo_with(&[]);
    tb.write_home_mode("noexec-shell", "", 0o664);
    let noexec = tb.home_path("noexec-shell");
    let r = tb.radm_env(&["enter"], "SHELL", &noexec.to_string_lossy());
    assert!(!r.success());
    assert!(r.err_contains("does not refer to an executable"));
}

#[test]
fn enter_env_shell_dumps_git_and_prompt_vars() {
    let tb = TestBed::new("enter-env");
    tb.init_repo_with(&[]);
    let r = tb.radm_env(&["enter"], "SHELL", "/usr/bin/env");
    assert!(r.success());
    assert_eq!(r.stderr, "");
    let repo = tb.repo().to_string_lossy().into_owned();
    let work = tb.home.to_string_lossy().into_owned();
    assert!(r.out_contains(&format!("GIT_DIR={repo}")));
    assert!(r.out_contains(&format!("GIT_WORK_TREE={work}")));
    // /usr/bin/env matches none of bash/csh/zsh, so shell_path="" and the
    // prompt collapses to two spaces between ")" and ">".
    let prompt_prefix = format!("yadm shell ({repo})");
    assert!(r.out_contains(&format!("PROMPT={prompt_prefix}")));
    assert!(r.out_contains(&format!("PS1={prompt_prefix}")));
}

/// Install a synthetic `#!/bin/sh` script at `root/<name>` (name must end in
/// bash/csh/zsh to match yadm's `$SHELL` suffix matching) that echoes its
/// invocation options and $PROMPT, optionally forcing a non-zero exit.
fn install_fake_shell(tb: &TestBed, name: &str, bad_exit: bool) -> std::path::PathBuf {
    let path = tb.root.join(name);
    let mut script = String::from("#!/bin/sh\necho OPTS=$*\necho PROMPT=$PROMPT\n");
    if bad_exit {
        script.push_str("false\n");
    }
    std::fs::write(&path, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o775)).unwrap();
    path
}

struct ShellOptsCase {
    shell_name: &'static str,
    base_opts: &'static str,
    path_marker: &'static str,
}

const SHELL_OPTS_CASES: [ShellOptsCase; 3] = [
    ShellOptsCase {
        shell_name: "bash",
        base_opts: "--norc",
        path_marker: "\\w",
    },
    ShellOptsCase {
        shell_name: "csh",
        base_opts: "-f",
        path_marker: "%~",
    },
    ShellOptsCase {
        shell_name: "zsh",
        base_opts: "-f",
        path_marker: "%~",
    },
];

#[test]
fn enter_shell_ops_no_command_all_shells_all_terms() {
    for case in &SHELL_OPTS_CASES {
        for term in ["", "dumb"] {
            let tb = TestBed::new(&format!("enter-ops-nocmd-{}-{}", case.shell_name, term));
            tb.init_repo_with(&[]);
            let shell_path = install_fake_shell(&tb, case.shell_name, false);

            let mut opts = case.base_opts.to_string();
            if case.shell_name == "zsh" && term == "dumb" {
                opts.push_str(" --no-zle");
            }

            let mut c = std::process::Command::new(TestBed::radm_bin());
            tb.apply_env(&mut c);
            c.env("SHELL", &shell_path);
            c.env("TERM", term);
            c.arg("enter");
            let r = common::run(c, None);

            assert_eq!(r.stderr, "", "shell={} term={}", case.shell_name, term);
            assert!(
                r.success(),
                "shell={} term={}: {r:?}",
                case.shell_name,
                term
            );
            assert!(
                r.out_contains(&format!("OPTS={opts}")),
                "shell={} term={}: {:?}",
                case.shell_name,
                term,
                r.stdout
            );
            let repo = tb.repo().to_string_lossy().into_owned();
            assert!(
                r.out_contains(&format!(
                    "PROMPT=yadm shell ({repo}) {} >",
                    case.path_marker
                )),
                "shell={} term={}: {:?}",
                case.shell_name,
                term,
                r.stdout
            );
            assert!(r.out_contains("Entering yadm repo"));
            assert!(r.out_contains("Leaving yadm repo"));
        }
    }
}

#[test]
fn enter_shell_ops_with_command_suppresses_banners_and_propagates_exit() {
    for case in &SHELL_OPTS_CASES {
        for term in ["", "dumb"] {
            for bad_exit in [false, true] {
                let tb = TestBed::new(&format!(
                    "enter-ops-cmd-{}-{}-{}",
                    case.shell_name, term, bad_exit
                ));
                tb.init_repo_with(&[]);
                let shell_path = install_fake_shell(&tb, case.shell_name, bad_exit);

                let mut opts = case.base_opts.to_string();
                if case.shell_name == "zsh" && term == "dumb" {
                    opts.push_str(" --no-zle");
                }

                let mut c = std::process::Command::new(TestBed::radm_bin());
                tb.apply_env(&mut c);
                c.env("SHELL", &shell_path);
                c.env("TERM", term);
                c.args(["enter", "test1", "test2", "test3"]);
                let r = common::run(c, None);

                assert_eq!(r.stderr, "");
                if bad_exit {
                    assert!(!r.success(), "expected failure: {r:?}");
                } else {
                    assert!(r.success(), "expected success: {r:?}");
                }
                assert!(r.out_contains(&format!("OPTS={opts} -c test1 test2 test3")));
                let repo = tb.repo().to_string_lossy().into_owned();
                assert!(r.out_contains(&format!(
                    "PROMPT=yadm shell ({repo}) {} >",
                    case.path_marker
                )));
                assert!(!r.out_contains("Entering yadm repo"));
                assert!(!r.out_contains("Leaving yadm repo"));
            }
        }
    }
}

// ---------------------------------------------------------------------
// 3. `list`
// ---------------------------------------------------------------------

fn ds1_files() -> Vec<(&'static str, &'static str)> {
    vec![("t1", "t1 content\n"), ("d1/t2", "t2 content\n")]
}

#[test]
fn list_dash_a_lists_all_tracked_files_worktree_relative() {
    let tb = TestBed::new("list-all");
    tb.init_repo_with(&ds1_files());
    let r = tb.radm(&["list", "-a"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    let mut got: Vec<&str> = r.stdout.lines().collect();
    got.sort();
    assert_eq!(got, vec!["d1/t2", "t1"]);
}

#[test]
fn list_no_flag_from_work_dir_lists_all_tracked_files() {
    let tb = TestBed::new("list-work");
    tb.init_repo_with(&ds1_files());
    let r = tb.radm_in(&tb.home, &["list"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    let mut got: Vec<&str> = r.stdout.lines().collect();
    got.sort();
    assert_eq!(got, vec!["d1/t2", "t1"]);
}

#[test]
fn list_no_flag_outside_work_dir_still_lists_all_tracked_files() {
    let tb = TestBed::new("list-outside");
    tb.init_repo_with(&ds1_files());
    let outside = tb.home.parent().unwrap().to_path_buf();
    let r = tb.radm_in(&outside, &["list"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    let mut got: Vec<&str> = r.stdout.lines().collect();
    got.sort();
    assert_eq!(got, vec!["d1/t2", "t1"]);
}

#[test]
fn list_no_flag_from_subdir_lists_only_files_under_subdir_relative() {
    let tb = TestBed::new("list-subdir");
    tb.init_repo_with(&ds1_files());
    let subdir = tb.home_path("d1");
    let r = tb.radm_in(&subdir, &["list"]);
    assert!(r.success());
    assert_eq!(r.stderr, "");
    let got: Vec<&str> = r.stdout.lines().collect();
    assert_eq!(got, vec!["t2"]);
}

// ---------------------------------------------------------------------
// 7. `yadm.git-program` override (CLI-observable via a logging wrapper)
// ---------------------------------------------------------------------

#[test]
fn git_program_override_wrapper_is_used_and_logs() {
    let tb = TestBed::new("git-program-override");
    tb.init_repo_with(&ds1_files());

    let log_path = tb.home_path("wrapper.log");
    let wrapper_script = format!(
        "#!/bin/sh\necho \"$*\" >> {:?}\nexec git \"$@\"\n",
        log_path.to_string_lossy()
    );
    tb.write_home_mode("bin/git-wrapper.sh", &wrapper_script, 0o755);
    let wrapper_path = tb.home_path("bin/git-wrapper.sh");

    let r = tb.radm(&[
        "config",
        "yadm.git-program",
        &wrapper_path.to_string_lossy(),
    ]);
    assert!(r.success());

    let r = tb.radm(&["list"]);
    assert!(r.success(), "list via wrapper failed: {r:?}");
    assert_eq!(r.stderr, "");
    let mut got: Vec<&str> = r.stdout.lines().collect();
    got.sort();
    assert_eq!(got, vec!["d1/t2", "t1"]);

    let log = tb.read_home("wrapper.log");
    assert!(
        log.contains("ls-files"),
        "wrapper log missing ls-files call: {log:?}"
    );
}

#[test]
fn git_program_override_used_by_version_command_too() {
    let tb = TestBed::new("git-program-version");
    tb.init_repo_with(&[]);

    let log_path = tb.home_path("wrapper2.log");
    let wrapper_script = format!(
        "#!/bin/sh\necho \"$*\" >> {:?}\nexec git \"$@\"\n",
        log_path.to_string_lossy()
    );
    tb.write_home_mode("bin/git-wrapper2.sh", &wrapper_script, 0o755);
    let wrapper_path = tb.home_path("bin/git-wrapper2.sh");

    let r = tb.radm(&[
        "config",
        "yadm.git-program",
        &wrapper_path.to_string_lossy(),
    ]);
    assert!(r.success());

    let r = tb.radm(&["version"]);
    assert!(r.success());
    assert!(r.out_contains("git version"));

    let log = tb.read_home("wrapper2.log");
    assert!(
        log.contains("--version"),
        "wrapper log missing --version call: {log:?}"
    );
}
