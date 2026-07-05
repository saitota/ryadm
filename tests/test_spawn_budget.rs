//! Spawn-count regression tests.
//!
//! ryadm shells out to git / uname / id many times per invocation, and the bulk
//! of a command's wall-clock time is those child processes. These tests pin the
//! number of external spawns a few representative commands make, so an
//! accidental change that reintroduces redundant spawns (a lost config cache, a
//! per-call PATH re-scan, an extra git round-trip) shows up as a failing test
//! rather than as silent slowness.
//!
//! They are intentionally loose upper bounds, not exact assertions: the goal is
//! to catch regressions ("this used to spawn ~20, now it spawns 60"), not to
//! freeze an exact count that churns on every benign refactor. When a change
//! legitimately alters the count, update the bound and note why.
//!
//! Spawns are observed via `RYADM_SPAWN_LOG`: ryadm appends one line per child
//! process it launches (see `util::record_spawn`). The env var is only honored
//! for this instrumentation and does not change ryadm's behaviour.

mod common;

use common::TestBed;

/// Count the external spawns ryadm makes while running `args`, by pointing
/// `RYADM_SPAWN_LOG` at a fresh file and counting its lines.
fn spawn_count(tb: &TestBed, args: &[&str]) -> usize {
    let log = tb.root.join(format!("spawn-{}.log", args.join("_")));
    let _ = std::fs::remove_file(&log);
    let r = tb.ryadm_env(args, "RYADM_SPAWN_LOG", log.to_str().unwrap());
    assert!(
        r.success() || !args.is_empty(),
        "command should run: {args:?}\nstderr: {}",
        r.stderr
    );
    std::fs::read_to_string(&log)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// The recorded spawn lines (program + NUL-joined args), for asserting on the
/// *shape* of what was spawned, not just the count.
fn spawn_lines(tb: &TestBed, args: &[&str]) -> Vec<String> {
    let log = tb.root.join(format!("spawnlines-{}.log", args.join("_")));
    let _ = std::fs::remove_file(&log);
    let _ = tb.ryadm_env(args, "RYADM_SPAWN_LOG", log.to_str().unwrap());
    std::fs::read_to_string(&log)
        .map(|s| {
            s.lines()
                .map(|l| l.replace('\0', " ").trim().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn config_get_local_stays_within_spawn_budget() {
    let tb = TestBed::new("spawn-config-get");
    assert!(tb.ryadm(&["init"]).success());

    // `config --get local.class` drags in the full auto_alt + auto_perms tail
    // (it's a repo-config op, so yadm sets CHANGES_POSSIBLE=1 — matching bash).
    // That's ~20 spawns of git/uname/id. 30 leaves headroom for platform
    // differences (distro probing) while still catching a doubling.
    let n = spawn_count(&tb, &["config", "--get", "local.class"]);
    assert!(
        n <= 30,
        "config --get local.class spawned {n} processes (budget 30); \
         a redundant-spawn regression likely crept in"
    );
}

#[test]
fn status_stays_within_spawn_budget() {
    let tb = TestBed::new("spawn-status");
    assert!(tb.ryadm(&["init"]).success());

    // status is a git passthrough: same auto_alt/auto_perms tail plus the git
    // status call itself.
    let n = spawn_count(&tb, &["status"]);
    assert!(n <= 30, "status spawned {n} processes (budget 30)");
}

#[test]
fn list_is_cheap() {
    let tb = TestBed::new("spawn-list");
    assert!(tb.ryadm(&["init"]).success());

    // `list -a` does not set CHANGES_POSSIBLE, so it skips the auto tail: just
    // OS detection + the git ls-files. A handful of spawns, no more.
    let n = spawn_count(&tb, &["list", "-a"]);
    assert!(n <= 10, "list -a spawned {n} processes (budget 10)");
}

#[test]
fn no_config_key_is_read_from_git_twice() {
    let tb = TestBed::new("spawn-no-dup");
    assert!(tb.ryadm(&["init"]).success());

    // The config read cache must collapse repeated reads of the same key within
    // a single invocation. Assert that no `git config ... <key>` line appears
    // more than once (identical arg vectors => a cache miss slipped through).
    let lines = spawn_lines(&tb, &["config", "--get", "local.class"]);
    let mut config_reads: Vec<&String> = lines
        .iter()
        .filter(|l| l.contains(" config ") || l.contains("--get") || l.contains("--bool"))
        .collect();
    let before = config_reads.len();
    config_reads.sort();
    config_reads.dedup();
    let after = config_reads.len();
    assert_eq!(
        before, after,
        "a git config read was issued twice for the same key (cache miss). \
         reads: {:#?}",
        lines
    );
}
