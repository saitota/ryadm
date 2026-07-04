//! Shared harness for ryadm integration tests.
//!
//! Each `TestBed` is a hermetic environment: its own HOME, a private git
//! identity, deterministic commit timestamps, and no controlling terminal
//! (children run in a new session so `/dev/tty` prompts fail like they do in
//! CI instead of grabbing the developer's terminal).

#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub const GIT_DATE: &str = "2020-01-01T00:00:00Z";

pub struct TestBed {
    pub root: PathBuf,
    pub home: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl RunResult {
    pub fn success(&self) -> bool {
        self.code == 0
    }
    pub fn out_contains(&self, s: &str) -> bool {
        self.stdout.contains(s)
    }
    pub fn err_contains(&self, s: &str) -> bool {
        self.stderr.contains(s)
    }
}

impl TestBed {
    pub fn new(name: &str) -> TestBed {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("{name}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            root.join("gitconfig"),
            "[user]\n\tname = Test User\n\temail = test@example.com\n\
[init]\n\tdefaultBranch = master\n\
[protocol \"file\"]\n\tallow = always\n",
        )
        .unwrap();
        TestBed { root, home }
    }

    /// Environment applied to every child: hermetic and deterministic.
    pub fn apply_env(&self, c: &mut Command) {
        c.env_clear();
        c.env("HOME", &self.home)
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("SHELL", "/bin/bash")
            .env("TERM", "dumb")
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("PWD", &self.home)
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_DATE", GIT_DATE)
            .env("GIT_COMMITTER_DATE", GIT_DATE);
        c.current_dir(&self.home);
        detach_tty(c);
    }

    pub fn ryadm_bin() -> &'static str {
        env!("CARGO_BIN_EXE_ryadm")
    }

    /// Run ryadm with args; stdin closed.
    pub fn ryadm(&self, args: &[&str]) -> RunResult {
        let mut c = Command::new(Self::ryadm_bin());
        self.apply_env(&mut c);
        c.args(args);
        run(c, None)
    }

    /// Run ryadm with an extra environment variable.
    pub fn ryadm_env(&self, args: &[&str], key: &str, value: &str) -> RunResult {
        let mut c = Command::new(Self::ryadm_bin());
        self.apply_env(&mut c);
        c.env(key, value);
        c.args(args);
        run(c, None)
    }

    /// Run ryadm from a specific working directory.
    pub fn ryadm_in(&self, dir: &Path, args: &[&str]) -> RunResult {
        let mut c = Command::new(Self::ryadm_bin());
        self.apply_env(&mut c);
        c.current_dir(dir);
        c.env("PWD", dir);
        c.args(args);
        run(c, None)
    }

    /// Run a shell snippet inside the testbed env (fixtures).
    pub fn sh(&self, script: &str) -> RunResult {
        let mut c = Command::new("bash");
        self.apply_env(&mut c);
        c.arg("-ec").arg(script);
        run(c, None)
    }

    // ---- path helpers -------------------------------------------------

    pub fn yadm_dir(&self) -> PathBuf {
        self.home.join(".config/yadm")
    }
    pub fn yadm_data(&self) -> PathBuf {
        self.home.join(".local/share/yadm")
    }
    pub fn repo(&self) -> PathBuf {
        self.yadm_data().join("repo.git")
    }
    pub fn archive(&self) -> PathBuf {
        self.yadm_data().join("archive")
    }

    // ---- file helpers --------------------------------------------------

    pub fn write_home(&self, rel: &str, content: &str) {
        let p = self.home.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, content).unwrap();
    }

    pub fn write_home_mode(&self, rel: &str, content: &str, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        self.write_home(rel, content);
        std::fs::set_permissions(self.home.join(rel), std::fs::Permissions::from_mode(mode))
            .unwrap();
    }

    pub fn read_home(&self, rel: &str) -> String {
        std::fs::read_to_string(self.home.join(rel)).unwrap_or_default()
    }

    pub fn home_path(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.home.join(rel).symlink_metadata().is_ok()
    }

    pub fn is_symlink(&self, rel: &str) -> bool {
        self.home
            .join(rel)
            .symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }

    pub fn link_target(&self, rel: &str) -> Option<String> {
        std::fs::read_link(self.home.join(rel))
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }

    pub fn mode(&self, rel: &str) -> u32 {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(self.home.join(rel))
            .map(|m| m.mode() & 0o7777)
            .unwrap_or(0)
    }

    // ---- common fixtures -------------------------------------------------

    /// ryadm init + commit the given (path, content) files as tracked files.
    pub fn init_repo_with(&self, files: &[(&str, &str)]) {
        let r = self.ryadm(&["init"]);
        assert!(r.success(), "init failed: {r:?}");
        for (path, content) in files {
            self.write_home(path, content);
            let r = self.ryadm(&["add", path]);
            assert!(r.success(), "add {path} failed: {r:?}");
        }
        if !files.is_empty() {
            let r = self.ryadm(&["commit", "-m", "test data"]);
            assert!(r.success(), "commit failed: {r:?}");
        }
    }
}

/// Start the child in its own session so it has no controlling terminal:
/// `/dev/tty` prompts then fail deterministically (like CI) instead of
/// hanging the test run on the developer's terminal.
fn detach_tty(c: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        c.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
}

pub fn run(mut c: Command, stdin: Option<&str>) -> RunResult {
    c.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    c.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().expect("spawn failed");
    if let Some(s) = stdin {
        child.stdin.take().unwrap().write_all(s.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().expect("wait failed");
    use std::os::unix::process::ExitStatusExt;
    RunResult {
        code: out
            .status
            .code()
            .unwrap_or_else(|| 128 + out.status.signal().unwrap_or(0)),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Host facts used to build alt conditions that match/mismatch this machine.
pub fn host_os() -> String {
    capture("uname", &["-s"])
}
pub fn host_arch() -> String {
    capture("uname", &["-m"])
}
pub fn host_hostname_short() -> String {
    let h = capture("uname", &["-n"]);
    h.split('.').next().unwrap_or("").to_string()
}
pub fn host_user() -> String {
    capture("id", &["-un"])
}

pub fn capture(cmd: &str, args: &[&str]) -> String {
    let out = Command::new(cmd).args(args).output().expect("capture");
    String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string()
}
