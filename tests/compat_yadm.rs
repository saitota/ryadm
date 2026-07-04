//! Differential compatibility tests: run identical scenarios against the
//! original bash yadm (RADM_COMPAT_YADM=<path to script>) and radm, then
//! compare every command's stdout/stderr/exit code and the resulting
//! filesystem state byte-for-byte (after normalizing testbed roots).
//!
//! Without RADM_COMPAT_YADM in the environment these tests are skipped, so a
//! plain `cargo test` works everywhere; `task test:compat` extracts the
//! pinned reference script and runs them.

mod common;

use std::path::Path;
use std::process::Command;

use common::*;

#[derive(Clone)]
enum Step {
    /// Run the dotfile manager with these args.
    Run(Vec<String>),
    /// Run with one extra environment variable.
    RunEnv(Vec<String>, String, String),
    /// Write a file (path relative to HOME).
    Write(String, String),
    /// Write a file with an explicit mode.
    WriteMode(String, String, u32),
    /// Remove a file or directory (relative to HOME).
    Remove(String),
    /// Fixture shell snippet (recorded and compared too).
    Sh(String),
}

fn s(v: &str) -> String {
    v.to_string()
}

fn run_step(args: &[&str]) -> Step {
    Step::Run(args.iter().map(|a| s(a)).collect())
}

fn yadm_ref() -> Option<String> {
    std::env::var("RADM_COMPAT_YADM")
        .ok()
        .filter(|v| !v.is_empty())
}

/// Substitute placeholders that must resolve per-testbed.
fn subst(input: &str, tb: &TestBed) -> String {
    input
        .replace("@HOME@", &tb.home.to_string_lossy())
        .replace("@ROOT@", &tb.root.to_string_lossy())
}

/// Normalize output for comparison across the two implementations.
fn normalize(input: &str, tb: &TestBed) -> String {
    let root = tb.root.to_string_lossy().into_owned();
    let mut out = input.replace(&root, "$ROOT");
    // `version` intentionally differs in its first line:
    // yadm prints "bash version ...", radm prints "radm version ...".
    let mut lines: Vec<String> = out.split('\n').map(|l| l.to_string()).collect();
    for l in &mut lines {
        if l.starts_with("bash version ") || l.starts_with("radm version ") {
            *l = "<impl> version".to_string();
        }
    }
    // bash prints its own read error when /dev/tty is unavailable; radm just
    // silently gets no answer (same visible behavior otherwise).
    lines.retain(|l| !l.ends_with(": /dev/tty: Device not configured"));
    out = lines.join("\n");
    out
}

struct SideResult {
    records: Vec<String>,
    snapshot: String,
}

fn execute(
    tb: &TestBed,
    yadm: Option<&str>,
    args: &[String],
    extra: Option<(&str, &str)>,
) -> RunResult {
    let mut c = match yadm {
        Some(script) => {
            let mut c = Command::new("bash");
            c.arg(script);
            c
        }
        None => Command::new(TestBed::radm_bin()),
    };
    tb.apply_env(&mut c);
    if let Some((k, v)) = extra {
        c.env(k, subst(v, tb));
    }
    for a in args {
        c.arg(subst(a, tb));
    }
    run(c, None)
}

fn run_side(name: &str, steps: &[Step], yadm: Option<&str>) -> SideResult {
    let side = if yadm.is_some() { "yadm" } else { "radm" };
    let tb = TestBed::new(&format!("compat-{name}-{side}"));
    let mut records: Vec<String> = Vec::new();

    for step in steps {
        match step {
            Step::Run(args) => {
                let r = execute(&tb, yadm, args, None);
                records.push(record(&tb, args, &r));
            }
            Step::RunEnv(args, k, v) => {
                let r = execute(&tb, yadm, args, Some((k, v)));
                records.push(record(&tb, args, &r));
            }
            Step::Write(rel, content) => tb.write_home(&subst(rel, &tb), &subst(content, &tb)),
            Step::WriteMode(rel, content, mode) => {
                tb.write_home_mode(&subst(rel, &tb), &subst(content, &tb), *mode)
            }
            Step::Remove(rel) => {
                let p = tb.home.join(subst(rel, &tb));
                if p.is_dir() {
                    let _ = std::fs::remove_dir_all(&p);
                } else {
                    let _ = std::fs::remove_file(&p);
                }
            }
            Step::Sh(script) => {
                let r = tb.sh(&subst(script, &tb));
                assert!(r.success(), "[{side}] fixture failed: {script}\n{r:?}");
                records.push(format!(
                    "$ sh fixture\nexit={}\n<<<stdout\n{}>>>\n<<<stderr\n{}>>>",
                    r.code,
                    normalize(&r.stdout, &tb),
                    normalize(&r.stderr, &tb)
                ));
            }
        }
    }

    let snapshot = snapshot(&tb);
    SideResult { records, snapshot }
}

fn record(tb: &TestBed, args: &[String], r: &RunResult) -> String {
    format!(
        "$ <bin> {}\nexit={}\n<<<stdout\n{}>>>\n<<<stderr\n{}>>>",
        normalize(&args.join(" "), tb),
        r.code,
        normalize(&r.stdout, tb),
        normalize(&r.stderr, tb)
    )
}

/// Deterministic textual snapshot of the testbed filesystem.
fn snapshot(tb: &TestBed) -> String {
    let mut entries: Vec<String> = Vec::new();
    walk(&tb.root, &tb.root, &mut entries, tb);
    entries.sort();
    entries.join("\n")
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>, tb: &TestBed) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.filter_map(|e| e.ok()) {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let Ok(meta) = path.symlink_metadata() else {
            continue;
        };
        if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&path)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(format!("L {rel} -> {}", normalize(&target, tb)));
        } else if meta.is_dir() {
            if is_git_dir(&path) {
                snapshot_git_dir(root, &path, out, tb);
            } else {
                out.push(format!("D {rel} {:04o}", mode_of(&meta)));
                walk(root, &path, out, tb);
            }
        } else {
            out.push(file_line(&rel, &path, &meta, tb));
        }
    }
}

fn is_git_dir(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    (name.ends_with(".git") || name == ".git") && path.join("HEAD").is_file()
}

/// Git dirs contain non-deterministic data (index stat cache, object mtimes);
/// snapshot only the parts that define behavior-visible state.
fn snapshot_git_dir(root: &Path, git: &Path, out: &mut Vec<String>, tb: &TestBed) {
    let rel = git
        .strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    out.push(format!("G {rel}"));
    for name in ["config", "HEAD", "packed-refs", "info/exclude"] {
        let p = git.join(name);
        if let Ok(meta) = p.symlink_metadata() {
            if meta.is_file() {
                out.push(file_line(&format!("{rel}/{name}"), &p, &meta, tb));
            }
        }
    }
    // refs (deterministic given fixed commit dates)
    let refs = git.join("refs");
    let mut stack = vec![refs];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in rd.filter_map(|e| e.ok()) {
            let p = entry.path();
            let Ok(meta) = p.symlink_metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(p);
            } else if meta.is_file() {
                let r = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                out.push(file_line(&r, &p, &meta, tb));
            }
        }
    }
}

fn file_line(rel: &str, path: &Path, meta: &std::fs::Metadata, tb: &TestBed) -> String {
    // the encryption archive embeds a random salt: compare length only
    let content = if rel.ends_with(".local/share/yadm/archive") {
        format!("<binary len={}>", meta.len())
    } else {
        match std::fs::read(path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => normalize(&text, tb).replace('\n', "\\n"),
                Err(e) => format!("<binary len={}>", e.as_bytes().len()),
            },
            Err(_) => "<unreadable>".to_string(),
        }
    };
    format!("F {rel} {:04o} {content}", mode_of(meta))
}

fn mode_of(meta: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.mode() & 0o7777
}

/// Run the scenario against both implementations and compare everything.
fn compat_check(name: &str, steps: Vec<Step>) {
    let Some(yadm) = yadm_ref() else {
        eprintln!("compat[{name}]: skipped (RADM_COMPAT_YADM not set)");
        return;
    };
    let y = run_side(name, &steps, Some(&yadm));
    let r = run_side(name, &steps, None);

    assert_eq!(
        y.records.len(),
        r.records.len(),
        "compat[{name}]: record count mismatch"
    );
    for (i, (yr, rr)) in y.records.iter().zip(r.records.iter()).enumerate() {
        assert_eq!(
            yr, rr,
            "compat[{name}]: step {i} differs\n===== yadm =====\n{yr}\n===== radm =====\n{rr}\n"
        );
    }
    assert_eq!(
        y.snapshot, r.snapshot,
        "compat[{name}]: filesystem snapshot differs"
    );
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

#[test]
fn compat_version_help_introspect() {
    compat_check(
        "version-help",
        vec![
            run_step(&["version"]),
            run_step(&["--version"]),
            run_step(&["help"]),
            run_step(&["--help"]),
            Step::Run(vec![]),
            run_step(&["introspect", "commands"]),
            run_step(&["introspect", "configs"]),
            run_step(&["introspect", "switches"]),
            run_step(&["introspect", "repo"]),
            run_step(&["introspect", "bogus"]),
            run_step(&["introspect"]),
        ],
    );
}

#[test]
fn compat_config_basics() {
    compat_check(
        "config",
        vec![
            run_step(&["config"]),
            run_step(&["config", "yadm.auto-alt", "false"]),
            run_step(&["config", "yadm.auto-alt"]),
            run_step(&["config", "--bool", "yadm.auto-alt"]),
            run_step(&["config", "local.class", "devbox"]), // no repo yet -> error
            run_step(&["init"]),
            run_step(&["config", "local.class", "devbox"]),
            run_step(&["config", "local.class"]),
            run_step(&["config", "-l"]),
            run_step(&["gitconfig", "user.name"]),
        ],
    );
}

#[test]
fn compat_init_git_list() {
    compat_check(
        "init-git",
        vec![
            run_step(&["init"]),
            Step::Write(s(".file1"), s("file one\n")),
            run_step(&["add", ".file1"]),
            run_step(&["commit", "-m", "one"]),
            run_step(&["list"]),
            run_step(&["list", "-a"]),
            run_step(&["status"]),
            run_step(&["log", "--oneline"]),
            run_step(&["bogus-command"]),
            run_step(&["clean"]),
            run_step(&["init"]),
            run_step(&["init", "-f"]),
            run_step(&["list"]),
        ],
    );
}

fn add_tracked(steps: &mut Vec<Step>, path: &str, content: &str) {
    steps.push(Step::Write(s(path), s(content)));
    steps.push(Step::Run(vec![s("add"), s(path)]));
}

#[test]
fn compat_alt_conditions() {
    let os = host_os();
    let host = host_hostname_short();
    let user = host_user();
    let arch = host_arch();

    let mut steps = vec![
        run_step(&["config", "yadm.auto-alt", "false"]),
        run_step(&["init"]),
    ];
    add_tracked(&mut steps, "t1##default", "t1 default\n");
    add_tracked(&mut steps, &format!("t2##os.{os}"), "t2 os match\n");
    add_tracked(&mut steps, "t2##os.Bogus", "t2 os bogus\n");
    add_tracked(&mut steps, &format!("t3##hostname.{host}"), "t3 host\n");
    add_tracked(&mut steps, &format!("t4##user.{user}"), "t4 user\n");
    add_tracked(&mut steps, &format!("t5##arch.{arch}"), "t5 arch\n");
    add_tracked(&mut steps, "t6##class.testclass", "t6 class\n");
    add_tracked(&mut steps, "t7##~os.Bogus", "t7 negated bogus\n");
    add_tracked(&mut steps, &format!("t8##~os.{os}"), "t8 negated match\n");
    add_tracked(
        &mut steps,
        &format!("t9##os.{os},hostname.{host}"),
        "t9 combo\n",
    );
    add_tracked(&mut steps, "t10##bogus.x", "t10 invalid\n");
    add_tracked(&mut steps, &format!("t12##os.{os},e.txt"), "t12 ext\n");
    steps.push(run_step(&["commit", "-m", "alts"]));
    steps.push(run_step(&["alt"]));
    steps.push(run_step(&["config", "local.class", "testclass"]));
    steps.push(run_step(&["alt"]));
    steps.push(run_step(&["list"]));

    compat_check("alt-conditions", steps);
}

#[test]
fn compat_alt_copy_and_dir() {
    let os = host_os();
    let mut steps = vec![
        run_step(&["init"]),
        run_step(&["config", "yadm.alt-copy", "true"]),
    ];
    add_tracked(&mut steps, &format!("c1##os.{os}"), "copied content\n");
    add_tracked(&mut steps, &format!("dir1##os.{os}/inner"), "inner file\n");
    steps.push(run_step(&["commit", "-m", "copy alts"]));
    steps.push(run_step(&["alt"]));
    compat_check("alt-copy-dir", steps);
}

#[test]
fn compat_alt_template_default() {
    let user = host_user();
    let mut steps = vec![
        run_step(&["config", "yadm.auto-alt", "false"]),
        run_step(&["init"]),
        run_step(&["config", "local.class", "TplClass"]),
        Step::Write(s("inc.part"), s("included line\n")),
    ];
    add_tracked(
        &mut steps,
        "tpl1##template",
        &format!(
            "os is {{{{yadm.os}}}}\nclass is {{{{yadm.class}}}}\n\
{{% if yadm.user == \"{user}\" %}}\nuser match\n{{% else %}}\nuser other\n{{% endif %}}\n\
{{% if yadm.class == \"nope\" %}}\nwrong class\n{{% endif %}}\n\
{{% include inc.part %}}\nsource is {{{{yadm.source}}}}\n"
        ),
    );
    add_tracked(&mut steps, "seed1##seed", "seeded {{yadm.class}}\n");
    add_tracked(
        &mut steps,
        "bad##template",
        "line one\n{% endif %}\nline three\n",
    );
    steps.push(run_step(&["commit", "-m", "templates"]));
    steps.push(run_step(&["alt"]));
    // seed must not be re-rendered once the target exists
    steps.push(Step::Write(s("seed1"), s("locally edited\n")));
    steps.push(run_step(&["alt"]));
    compat_check("alt-template", steps);
}

#[test]
fn compat_encrypt_openssl() {
    let wrapper = "#!/bin/sh\nexec openssl \"$@\" -pass pass:compat-test\n";
    let mut steps = vec![
        Step::WriteMode(s(".wrap/openssl-wrap"), s(wrapper), 0o755),
        run_step(&["init"]),
        run_step(&["config", "yadm.cipher", "openssl"]),
        run_step(&[
            "config",
            "yadm.openssl-program",
            "@HOME@/.wrap/openssl-wrap",
        ]),
        Step::Write(
            s(".config/yadm/encrypt"),
            s(".ssh/sec\nsecret*\n!secret2\n# a comment\n\n"),
        ),
        Step::Write(s(".ssh/sec"), s("ssh secret\n")),
        Step::Write(s("secret1"), s("secret one\n")),
        Step::Write(s("secret2"), s("secret two (excluded)\n")),
    ];
    add_tracked(&mut steps, "secret3", "secret three (tracked)\n");
    steps.push(run_step(&["commit", "-m", "tracked secret"]));
    // fix mtimes so `tar tv` listings are identical across the two runs
    steps.push(Step::Sh(s(
        "touch -t 202001010000 \"$HOME/.ssh/sec\" \"$HOME/secret1\" \"$HOME/secret2\" \"$HOME/secret3\"",
    )));
    steps.push(run_step(&["encrypt"]));
    steps.push(Step::Remove(s("secret1")));
    steps.push(Step::Remove(s(".ssh/sec")));
    steps.push(run_step(&["decrypt"]));
    steps.push(run_step(&["decrypt", "-l"]));
    compat_check("encrypt-openssl", steps);
}

#[test]
fn compat_perms() {
    compat_check(
        "perms",
        vec![
            run_step(&["init"]),
            Step::WriteMode(s(".ssh/key"), s("k\n"), 0o644),
            Step::WriteMode(s(".ssh/.hidden"), s("h\n"), 0o644),
            Step::WriteMode(s(".gnupg/trust"), s("t\n"), 0o666),
            run_step(&["perms"]),
            run_step(&["config", "yadm.ssh-perms", "false"]),
            Step::Sh(s(
                "chmod 644 \"$HOME/.ssh/key\" && chmod 666 \"$HOME/.gnupg/trust\"",
            )),
            run_step(&["perms"]),
        ],
    );
}

#[test]
fn compat_hooks() {
    let dump = "#!/bin/sh\necho pre-status-ran\nenv | grep '^YADM_' | sort\nexit 0\n";
    let post = "#!/bin/sh\necho \"post exit=$YADM_HOOK_EXIT cmd=$YADM_HOOK_COMMAND\"\n";
    let fail = "#!/bin/sh\necho failing-pre-hook\nexit 13\n";
    compat_check(
        "hooks",
        vec![
            run_step(&["init"]),
            Step::WriteMode(s(".config/yadm/hooks/pre_status"), s(dump), 0o755),
            Step::WriteMode(s(".config/yadm/hooks/post_status"), s(post), 0o755),
            run_step(&["status"]),
            Step::WriteMode(s(".config/yadm/hooks/pre_commit"), s(fail), 0o755),
            run_step(&["commit", "-m", "blocked"]),
        ],
    );
}

fn remote_fixture() -> Step {
    Step::Sh(s("git init -q \"@ROOT@/seed\" && cd \"@ROOT@/seed\" && \
git checkout -qb master 2>/dev/null || true; \
echo 'remote file one' > .cfile1 && mkdir -p .config && echo cfg > .config/app && \
git add .cfile1 .config/app && git commit -qm seed && \
git clone -q --bare \"@ROOT@/seed\" \"@ROOT@/remote.git\""))
}

#[test]
fn compat_clone() {
    compat_check(
        "clone",
        vec![
            remote_fixture(),
            run_step(&["clone", "@ROOT@/does-not-exist.git"]),
            Step::Write(s(".cfile1"), s("local conflicting content\n")),
            run_step(&["clone", "@ROOT@/remote.git"]),
            run_step(&["status"]),
            run_step(&["list"]),
        ],
    );
}

#[test]
fn compat_clone_bootstrap() {
    let bootstrap = "#!/bin/sh\necho bootstrapped-ok\nexit 0\n";
    compat_check(
        "clone-bootstrap",
        vec![
            remote_fixture(),
            Step::WriteMode(s(".config/yadm/bootstrap"), s(bootstrap), 0o755),
            run_step(&["clone", "--bootstrap", "@ROOT@/remote.git"]),
            run_step(&["list"]),
        ],
    );
}

#[test]
fn compat_clone_bootstrap_prompt_no_tty() {
    let bootstrap = "#!/bin/sh\necho should-not-run\nexit 0\n";
    compat_check(
        "clone-bootstrap-prompt",
        vec![
            remote_fixture(),
            Step::WriteMode(s(".config/yadm/bootstrap"), s(bootstrap), 0o755),
            // no tty: the y/n prompt is printed, the read fails, bootstrap is skipped
            run_step(&["clone", "@ROOT@/remote.git"]),
            run_step(&["clone", "--no-bootstrap", "-f", "@ROOT@/remote.git"]),
        ],
    );
}

#[test]
fn compat_upgrade() {
    compat_check(
        "upgrade",
        vec![
            run_step(&["init"]),
            Step::Write(s(".f1"), s("data\n")),
            run_step(&["add", ".f1"]),
            run_step(&["commit", "-m", "one"]),
            Step::Sh(s(
                "mv \"$HOME/.local/share/yadm/repo.git\" \"$HOME/.config/yadm/repo.git\" && \
touch \"$HOME/.config/yadm/files.gpg\"",
            )),
            run_step(&["status"]), // legacy warning + repo missing error
            run_step(&["upgrade"]),
            run_step(&["status"]),
            run_step(&["list"]),
            run_step(&["--yadm-repo", "@HOME@/other-repo.git", "upgrade"]),
        ],
    );
}

#[test]
fn compat_error_paths() {
    compat_check(
        "errors",
        vec![
            run_step(&["alt"]),
            run_step(&["list"]),
            run_step(&["bootstrap"]),
            run_step(&["decrypt"]),
            run_step(&["encrypt"]),
            run_step(&["alt", "-w", "/nonexistent/workdir-xyz"]),
            run_step(&["-Y"]),
            run_step(&["--yadm-data"]),
        ],
    );
}

#[test]
fn compat_enter() {
    compat_check(
        "enter",
        vec![
            run_step(&["init"]),
            run_step(&["enter", "pwd"]),
            run_step(&["enter", "echo", "hello", "world"]),
            run_step(&["enter"]),
            Step::RunEnv(
                vec![s("enter"), s("pwd")],
                s("SHELL"),
                s("/nonexistent-shell"),
            ),
        ],
    );
}

#[test]
fn compat_auto_flags() {
    let os = host_os();
    let mut steps = vec![run_step(&["init"])];
    // auto-alt on by default: adding a tracked alt links it automatically
    add_tracked(&mut steps, &format!("a1##os.{os}"), "auto one\n");
    steps.push(run_step(&["commit", "-m", "a1"]));
    steps.push(run_step(&["config", "yadm.auto-alt", "false"]));
    add_tracked(&mut steps, &format!("a2##os.{os}"), "auto two\n");
    steps.push(run_step(&["commit", "-m", "a2"]));
    steps.push(run_step(&["alt"]));
    steps.push(run_step(&["config", "yadm.auto-exclude", "false"]));
    steps.push(run_step(&["alt"]));
    compat_check("auto-flags", steps);
}

#[test]
fn compat_legacy_warning() {
    compat_check(
        "legacy-warning",
        vec![
            Step::Sh(s(
                "mkdir -p \"$HOME/.yadm\" && touch \"$HOME/.yadm/config\"",
            )),
            run_step(&["list"]),
            run_step(&["--yadm-repo", "@HOME@/r.git", "list"]),
            run_step(&["upgrade"]),
        ],
    );
}
