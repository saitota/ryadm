//! Small commands: bootstrap, clean, help, introspect, list, perms, version.

use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::config;
use crate::context::{Context, RADM_VERSION, VERSION};
use crate::encrypt;
use crate::git;
use crate::hooks;
use crate::paths;
use crate::privdirs;
use crate::util;

pub fn bootstrap_available(ctx: &Context) -> bool {
    let p = Path::new(&ctx.bootstrap_file);
    p.is_file() && util::is_executable(p)
}

pub fn bootstrap(ctx: &mut Context) {
    if !bootstrap_available(ctx) {
        let msg = format!(
            "Cannot execute bootstrap\\n'{}' is not an executable program.",
            ctx.bootstrap_file
        );
        util::error_out(ctx, &msg);
    }

    // GIT_DIR should not be set for user's bootstrap code
    std::env::remove_var("GIT_DIR");

    println!("Executing {}", ctx.bootstrap_file);
    // exec: replace this process, like the script does
    use std::os::unix::process::CommandExt;
    let err = Command::new(&ctx.bootstrap_file).exec();
    eprintln!("radm: {}: {}", ctx.bootstrap_file, err);
    std::process::exit(126);
}

pub fn clean(ctx: &mut Context) {
    util::error_out(
        ctx,
        "\"git clean\" has been disabled for safety. You could end up removing all unmanaged files.",
    );
}

pub fn help(ctx: &Context) -> ! {
    // paths shown with a literal $HOME prefix
    let dollar_home = |p: &str| -> String {
        if ctx.home.is_empty() {
            p.to_string()
        } else {
            p.replacen(&ctx.home, "$HOME", 1)
        }
    };
    let config = dollar_home(&ctx.config_file);
    let encrypt = dollar_home(&ctx.encrypt_file);
    let bootstrap = dollar_home(&ctx.bootstrap_file);
    let repo = dollar_home(&ctx.repo);
    let archive = dollar_home(&ctx.archive);

    // column layout: "  <path><padding to 32> - <description>"
    let pad = |s: &str| -> String {
        let padding = 32usize.saturating_sub(s.len());
        format!("{}{}", s, " ".repeat(padding))
    };

    // raw string: `\n\` continuations would strip the leading indentation
    print!(
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

"#,
        config = pad(&config),
        encrypt = pad(&encrypt),
        bootstrap = pad(&bootstrap),
        repo = pad(&repo),
        archive = pad(&archive),
    );
    hooks::exit_with_hook(ctx, 1)
}

pub fn introspect(ctx: &Context, args: &[String]) {
    match args.first().map(|s| s.as_str()) {
        Some("commands") => print!("{}", config::INTROSPECT_COMMANDS),
        Some("configs") => print!("{}", config::INTROSPECT_CONFIGS),
        Some("repo") => println!("{}", ctx.repo),
        Some("switches") => print!("{}", config::INTROSPECT_SWITCHES),
        _ => (),
    }
}

pub fn list(ctx: &mut Context) {
    git::require_repo(ctx);

    // process relative to YADM_WORK when --all is specified
    if ctx.list_all && !paths::cd_work(ctx, "List") {
        return;
    }

    // list tracked files
    let _ = git::run(ctx, &["ls-files"]);
}

pub fn perms(ctx: &mut Context) {
    encrypt::parse_encrypt(ctx);

    if !paths::cd_work(ctx, "Perms") {
        return;
    }

    let mut globs: Vec<String> = Vec::new();

    // include the archive created by "encrypt"
    if Path::new(&ctx.archive).is_file() {
        globs.push(ctx.archive.clone());
    }

    // only include private globs if using HOME as worktree
    if ctx.work == ctx.home {
        // include all .ssh files (unless disabled)
        if config::config_output(ctx, &["--bool", "yadm.ssh-perms"]) != "false" {
            globs.extend([".ssh", ".ssh/*", ".ssh/.[!.]*"].map(String::from));
        }

        // include all gpg files (unless disabled)
        let gnupghome = privdirs::gnupg_dir(ctx);
        if config::config_output(ctx, &["--bool", "yadm.gpg-perms"]) != "false" {
            globs.push(gnupghome.clone());
            globs.push(format!("{gnupghome}/*"));
            globs.push(format!("{gnupghome}/.[!.]*"));
        }
    }

    // include any files we encrypt
    if let Some(files) = &ctx.encrypt_include_files {
        globs.extend(files.iter().cloned());
    }

    // remove group/other permissions from collected globs; entries are
    // word-split and glob-expanded like the unquoted ${GLOBS[@]} in yadm
    for glob in &globs {
        for word in glob.split_whitespace() {
            for path in expand_glob(word) {
                chmod_go_rwx(&path);
            }
        }
    }
}

/// chmod -f go-rwx: clear group/other bits, keep everything else, stay silent.
fn chmod_go_rwx(path: &str) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mode = meta.permissions().mode() & 0o7777;
        let new_mode = mode & !0o077;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(new_mode));
    }
}

/// Shell-style glob expansion (nullglob off: no match returns the literal).
fn expand_glob(pattern: &str) -> Vec<String> {
    if !pattern.contains(['*', '?', '[']) {
        return vec![pattern.to_string()];
    }
    let absolute = pattern.starts_with('/');
    let comps: Vec<&str> = pattern.split('/').filter(|c| !c.is_empty()).collect();
    let mut bases: Vec<String> = vec![if absolute {
        "/".to_string()
    } else {
        String::new()
    }];
    for comp in comps {
        let mut next: Vec<String> = Vec::new();
        if comp.contains(['*', '?', '[']) {
            for base in &bases {
                let dir = if base.is_empty() {
                    "."
                } else if base == "/" {
                    "/"
                } else {
                    base.as_str()
                };
                if let Ok(rd) = std::fs::read_dir(dir) {
                    let mut names: Vec<String> = rd
                        .filter_map(|e| e.ok())
                        .filter_map(|e| e.file_name().into_string().ok())
                        .filter(|n| glob_match(comp, n))
                        .collect();
                    names.sort();
                    for n in names {
                        next.push(join_component(base, &n));
                    }
                }
            }
        } else {
            for base in &bases {
                next.push(join_component(base, comp));
            }
        }
        bases = next;
    }
    if bases.is_empty() {
        vec![pattern.to_string()]
    } else {
        bases
    }
}

fn join_component(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else if base == "/" {
        format!("/{name}")
    } else {
        format!("{base}/{name}")
    }
}

/// Match one path component against a glob pattern (*, ?, [set], [!set]).
/// Dotfiles are only matched when the pattern starts with a literal dot.
fn glob_match(pattern: &str, name: &str) -> bool {
    if name.starts_with('.') && !pattern.starts_with('.') {
        return false;
    }
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    glob_helper(&p, &n)
}

fn glob_helper(p: &[char], n: &[char]) -> bool {
    if p.is_empty() {
        return n.is_empty();
    }
    match p[0] {
        '*' => (0..=n.len()).any(|i| glob_helper(&p[1..], &n[i..])),
        '?' => !n.is_empty() && glob_helper(&p[1..], &n[1..]),
        '[' => {
            if n.is_empty() {
                return false;
            }
            match parse_class(p) {
                Some((matched_fn, rest)) => matched_fn(n[0]) && glob_helper(rest, &n[1..]),
                None => n[0] == '[' && glob_helper(&p[1..], &n[1..]),
            }
        }
        c => !n.is_empty() && n[0] == c && glob_helper(&p[1..], &n[1..]),
    }
}

type ClassFn = Box<dyn Fn(char) -> bool>;

/// Parse a [...] class; returns a matcher and the remaining pattern.
fn parse_class(p: &[char]) -> Option<(ClassFn, &[char])> {
    let mut i = 1;
    let negate = matches!(p.get(i), Some('!') | Some('^'));
    if negate {
        i += 1;
    }
    let start = i;
    let mut end = None;
    let mut j = i;
    while j < p.len() {
        if p[j] == ']' && j > start {
            end = Some(j);
            break;
        }
        j += 1;
    }
    let end = end?;
    let mut singles: Vec<char> = Vec::new();
    let mut ranges: Vec<(char, char)> = Vec::new();
    let mut k = start;
    while k < end {
        if k + 2 < end && p[k + 1] == '-' {
            ranges.push((p[k], p[k + 2]));
            k += 3;
        } else {
            singles.push(p[k]);
            k += 1;
        }
    }
    let matcher: ClassFn = Box::new(move |c: char| {
        let hit = singles.contains(&c) || ranges.iter().any(|(a, b)| *a <= c && c <= *b);
        hit != negate
    });
    Some((matcher, &p[end + 1..]))
}

pub fn version(ctx: &Context) -> ! {
    println!("radm version {RADM_VERSION}");
    print!(" ");
    let _ = std::io::stdout().flush();
    let _ = git::cmd(ctx).arg("--version").status();
    println!("yadm version {VERSION}");
    hooks::exit_with_hook(ctx, 0)
}
