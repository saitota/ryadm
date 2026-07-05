//! Runtime context mirroring yadm's global variables.
//!
//! Paths are kept as `String` (not `PathBuf`) on purpose: yadm manipulates
//! paths with plain string operations (prefix/suffix stripping, concatenation)
//! and byte-for-byte compatible behavior requires the same semantics.

/// ryadm's own version, from the Git release tag at build time (see build.rs),
/// falling back to the Cargo.toml version for plain `cargo build`.
pub const RYADM_VERSION: &str = env!("RYADM_VERSION");
/// Name of the yadm v1 archive file, relative to the legacy dir.
pub const LEGACY_ARCHIVE: &str = "files.gpg";

pub struct Context {
    pub home: String,
    pub pwd: String,
    /// YADM_WORK — the work tree (defaults to $HOME).
    pub work: String,
    /// YADM_DIR — config dir; empty until resolved by set_yadm_dirs.
    pub dir: String,
    /// YADM_DATA — data dir; empty until resolved by set_yadm_dirs.
    pub data: String,
    /// YADM_LEGACY_DIR — $HOME/.yadm
    pub legacy_dir: String,

    // Relative names until configure_paths() joins them onto dir/data.
    pub config_file: String,    // YADM_CONFIG
    pub encrypt_file: String,   // YADM_ENCRYPT
    pub bootstrap_file: String, // YADM_BOOTSTRAP
    pub hooks_dir: String,      // YADM_HOOKS
    pub alt_dir: String,        // YADM_ALT
    pub repo: String,           // YADM_REPO
    pub archive: String,        // YADM_ARCHIVE
    /// YADM_BASE — work tree base for alt processing ("" when work is "/").
    pub base: String,

    // --yadm-* command line overrides (empty when unset).
    pub override_repo: String,
    pub override_config: String,
    pub override_encrypt: String,
    pub override_archive: String,
    pub override_bootstrap: String,

    // External programs.
    pub git_program: String,
    /// Absolute path `git_program` resolves to, cached by `require_git` for
    /// spawning only. Empty until resolved; `git_exe` then falls back to
    /// `git_program`. Avoids `Command` re-scanning a large `PATH` per spawn.
    pub git_program_resolved: String,
    pub gpg_program: String,
    pub openssl_program: String,
    /// Candidate list; set_awk() narrows it to the first available.
    pub awk_program: Vec<String>,
    pub git_crypt_program: String,
    pub transcrypt_program: String,
    pub j2cli_program: String,
    pub envtpl_program: String,
    pub esh_program: String,
    pub lsb_release_program: String,

    /// Files consulted for OS detection (fields so unit tests can redirect them).
    pub os_release: String,
    pub proc_version: String,
    pub operating_system: String,
    pub use_cygpath: bool,

    // Flags parsed from internal-command arguments.
    pub debug: bool,    // -d
    pub force: bool,    // -f
    pub list_all: bool, // -a
    pub do_list: bool,  // -l

    // Command state.
    /// Internal command being run (underscored form), "" for git passthrough.
    pub yadm_command: String,
    pub hook_command: String,
    pub full_command: String,
    pub changes_possible: bool,
    /// 0: skip auto_bootstrap, 1: ask, 2: perform bootstrap, 3: prevent bootstrap
    pub do_bootstrap: i32,
    /// None == yadm's "unparsed" sentinel.
    pub encrypt_include_files: Option<Vec<String>>,
    pub no_encrypt_tracked_files: Vec<String>,
    pub invalid_alt: Vec<String>,
    pub legacy_warning_issued: bool,

    /// Memoizes `config_output` reads so one invocation never spawns `git
    /// config` twice for the same key. Writes clear it via
    /// `invalidate_config_cache`. `RefCell` because `config_output` takes
    /// `&Context`.
    pub config_cache: std::cell::RefCell<std::collections::HashMap<String, String>>,
}

impl Context {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let pwd = std::env::var("PWD")
            .ok()
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        Context {
            work: home.clone(),
            legacy_dir: format!("{home}/.yadm"),
            pwd,
            home,
            dir: String::new(),
            data: String::new(),
            config_file: "config".into(),
            encrypt_file: "encrypt".into(),
            bootstrap_file: "bootstrap".into(),
            hooks_dir: "hooks".into(),
            alt_dir: "alt".into(),
            repo: "repo.git".into(),
            archive: "archive".into(),
            base: String::new(),
            override_repo: String::new(),
            override_config: String::new(),
            override_encrypt: String::new(),
            override_archive: String::new(),
            override_bootstrap: String::new(),
            git_program: "git".into(),
            git_program_resolved: String::new(),
            gpg_program: "gpg".into(),
            openssl_program: "openssl".into(),
            awk_program: vec!["gawk".into(), "awk".into()],
            git_crypt_program: "git-crypt".into(),
            transcrypt_program: "transcrypt".into(),
            j2cli_program: "j2".into(),
            envtpl_program: "envtpl".into(),
            esh_program: "esh".into(),
            lsb_release_program: "lsb_release".into(),
            os_release: "/etc/os-release".into(),
            proc_version: "/proc/version".into(),
            operating_system: "Unknown".into(),
            use_cygpath: false,
            debug: false,
            force: false,
            list_all: false,
            do_list: false,
            yadm_command: String::new(),
            hook_command: String::new(),
            full_command: String::new(),
            changes_possible: false,
            do_bootstrap: 0,
            encrypt_include_files: None,
            no_encrypt_tracked_files: Vec::new(),
            invalid_alt: Vec::new(),
            legacy_warning_issued: false,
            config_cache: std::cell::RefCell::new(std::collections::HashMap::new()),
        }
    }

    /// Drop every memoized `config_output` read. Called before/after operations
    /// that may change configuration (the `config` command) so a subsequent
    /// read never returns a stale value.
    pub fn invalidate_config_cache(&self) {
        self.config_cache.borrow_mut().clear();
    }
}
