//! Alternate file processing (yadm's alt command and scoring engine).
//! Reference: yadm script lines 167-745 (score_file, record_score,
//! set_local_alt_values, alt, alt_linking, ln_relative, report_invalid_alts).
//! Spec: scratchpad specs alt.md / template.md.

use crate::config;
use crate::context::Context;
use crate::exclude;
use crate::git;
use crate::os;
use crate::paths;
use crate::template;
use crate::util;

/// Values gathered by yadm's set_local_alt_values.
#[derive(Default, Clone)]
pub struct LocalValues {
    pub class: String,
    pub classes: Vec<String>,
    pub arch: String,
    pub system: String,
    pub host: String,
    pub user: String,
    pub distro: String,
    pub distro_family: String,
}

/// One recorded alt candidate (target -> best-scoring source), mirroring the
/// four parallel bash arrays alt_targets/alt_sources/alt_scores/
/// alt_template_processors.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AltCandidate {
    target: String,
    source: String,
    score: i64,
    template_processor: String,
}

/// Split `conditions` on ',' the way bash's `IFS=',' read -ra fields` does:
/// an empty string yields zero fields, and exactly one trailing empty field
/// (from a trailing comma) is dropped. Interior empty fields are preserved.
fn split_conditions(conditions: &str) -> Vec<String> {
    if conditions.is_empty() {
        return Vec::new();
    }
    let mut fields: Vec<String> = conditions.split(',').map(|s| s.to_string()).collect();
    if fields.last().is_some_and(|f| f.is_empty()) {
        fields.pop();
    }
    fields
}

/// yadm's score_file (yadm:169-254). Computes the score for `source`/`target`
/// under `conditions`, recording it (via `record_score`-equivalent state
/// mutation on `candidates`/`invalid_alt`) when the loop completes without an
/// early abort.
#[allow(clippy::too_many_arguments)]
fn score_file(
    ctx: &Context,
    values: &LocalValues,
    loud: bool,
    invalid_alt: &mut Vec<String>,
    candidates: &mut Vec<AltCandidate>,
    source: &str,
    target: &str,
    conditions: &str,
) {
    let mut score: i64 = 0;
    let mut template_processor = String::new();

    for field in split_conditions(conditions) {
        let (mut label, mut value) = match field.find('.') {
            Some(i) => (field[..i].to_string(), field[i + 1..].to_string()),
            None => (field.clone(), String::new()),
        };
        if field == label {
            value = String::new();
        }

        // Negation prefix check is a plain (case-sensitive) [ ] test in bash.
        let mut negate = false;
        if label.starts_with('~') {
            negate = true;
            label = label[1..].to_string();
        }

        let label_lower = label.to_lowercase();
        let mut delta: i64 = if negate { 1 } else { -1 };

        match label_lower.as_str() {
            "default" => {
                if negate {
                    invalid_alt.push(source.to_string());
                } else {
                    delta = 0;
                }
            }
            "a" | "arch" => {
                delta = if ci_eq(&value, &values.arch) { 1 } else { -1 };
            }
            "o" | "os" => {
                delta = if ci_eq(&value, &values.system) { 2 } else { -2 };
            }
            "d" | "distro" => {
                delta = if ci_eq(&value.replace(' ', "_"), &values.distro.replace(' ', "_")) {
                    4
                } else {
                    -4
                };
            }
            "f" | "distro_family" => {
                delta = if ci_eq(
                    &value.replace(' ', "_"),
                    &values.distro_family.replace(' ', "_"),
                ) {
                    8
                } else {
                    -8
                };
            }
            "c" | "class" => {
                delta = if in_list_ci(&value, &values.classes) {
                    16
                } else {
                    -16
                };
            }
            "h" | "hostname" => {
                delta = if ci_eq(&value, &values.host) { 32 } else { -32 };
            }
            "u" | "user" => {
                delta = if ci_eq(&value, &values.user) { 64 } else { -64 };
            }
            "e" | "extension" => {
                // extension isn't a condition and doesn't affect the score
                continue;
            }
            "s" | "seed" | "t" | "template" | "yadm" => {
                if negate {
                    invalid_alt.push(source.to_string());
                // bash checks the FIRST char of the original label,
                // case-sensitively: [ "${label:0:1}" != "s" ] — so "seed"
                // gets the seed rule but "Seed" is treated like a template.
                } else if !label.starts_with('s') || !std::path::Path::new(target).exists() {
                    template_processor = template::choose_template_processor(ctx, &value);
                    if !template_processor.is_empty() {
                        delta = 0;
                    } else if loud {
                        println!("No supported template processor for {source}");
                    } else {
                        util::debug(
                            ctx,
                            &format!("No supported template processor for {source}"),
                        );
                    }
                }
            }
            _ => {
                invalid_alt.push(source.to_string());
            }
        }

        if negate {
            delta = -delta;
        }
        if delta < 0 {
            return;
        }
        score += delta + if negate { 0 } else { 1000 };
    }

    record_score(
        candidates,
        score,
        target,
        source,
        &template_processor,
        &ctx.config_file,
    );
}

/// Case-insensitive equality (bash `[[ = ]]` under `shopt -s nocasematch`).
fn ci_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// yadm's in_list, case-insensitively (called from within the nocasematch case).
fn in_list_ci(element: &str, list: &[String]) -> bool {
    list.iter().any(|e| ci_eq(e, element))
}

/// yadm's record_score (yadm:256-305).
fn record_score(
    candidates: &mut Vec<AltCandidate>,
    score: i64,
    target: &str,
    source: &str,
    template_processor: &str,
    config_file: &str,
) {
    if score == 0 && template_processor.is_empty() {
        return;
    }

    // search from the end, like the bash backward loop
    let index = candidates.iter().rposition(|c| c.target == target);

    let index = match index {
        None => {
            let candidate = AltCandidate {
                target: target.to_string(),
                source: source.to_string(),
                score,
                template_processor: template_processor.to_string(),
            };
            if target == config_file {
                candidates.insert(0, candidate);
            } else {
                candidates.push(candidate);
            }
            return;
        }
        Some(i) => i,
    };

    if !candidates[index].template_processor.is_empty() {
        if template_processor.is_empty() || score < candidates[index].score {
            return;
        }
    } else if template_processor.is_empty() && score <= candidates[index].score {
        return;
    }

    candidates[index].source = source.to_string();
    candidates[index].score = score;
    candidates[index].template_processor = template_processor.to_string();
}

/// yadm's set_local_alt_values (yadm:654-694).
pub fn set_local_alt_values(ctx: &Context) -> LocalValues {
    let all_classes = config::config_output(ctx, &["--get-all", "local.class"]);
    // bash: `while IFS='' read -r class; do ...; done <<<"$all_classes"` — a
    // here-string on an empty variable still executes the loop body once
    // with an empty line, so an unset local.class yields classes=[""].
    let classes: Vec<String> = all_classes.split('\n').map(|s| s.to_string()).collect();
    let class = classes.last().cloned().unwrap_or_default();

    let mut arch = config::config_output(ctx, &["local.arch"]);
    if arch.is_empty() {
        arch = os::capture("uname", &["-m"]);
    }

    let mut system = config::config_output(ctx, &["local.os"]);
    if system.is_empty() {
        system = ctx.operating_system.clone();
    }

    let mut host = config::config_output(ctx, &["local.hostname"]);
    if host.is_empty() {
        host = os::capture("uname", &["-n"]);
        // trim any domain from hostname
        host = host.split('.').next().unwrap_or("").to_string();
    }

    let mut user = config::config_output(ctx, &["local.user"]);
    if user.is_empty() {
        user = os::capture("id", &["-u", "-n"]);
    }

    let mut distro = config::config_output(ctx, &["local.distro"]);
    if distro.is_empty() {
        distro = os::query_distro(ctx);
    }

    let mut distro_family = config::config_output(ctx, &["local.distro-family"]);
    if distro_family.is_empty() {
        distro_family = os::query_distro_family(ctx);
    }

    LocalValues {
        class,
        classes,
        arch,
        system,
        host,
        user,
        distro,
        distro_family,
    }
}

/// yadm's alt() (yadm:543-618).
pub fn alt(ctx: &mut Context) {
    git::require_repo(ctx);
    crate::encrypt::parse_encrypt(ctx);

    let values = set_local_alt_values(ctx);

    // only be noisy if the "alt" command was run directly
    let loud = ctx.yadm_command == "alt";

    // decide if a copy should be done instead of a symbolic link
    let do_copy = config::config_output(ctx, &["--bool", "yadm.alt-copy"]) == "true";

    if !paths::cd_work(ctx, "Alternates") {
        return;
    }

    // determine all tracked files
    let (tracked_out, _) = git::capture(ctx, &["ls-files", "--", "*##*"], false);
    let tracked_files: Vec<String> = if tracked_out.is_empty() {
        Vec::new()
    } else {
        tracked_out.lines().map(|s| s.to_string()).collect()
    };

    let mut candidates: Vec<AltCandidate> = Vec::new();
    let mut invalid_alt: Vec<String> = Vec::new();

    let encrypt_files: Vec<String> = ctx.encrypt_include_files.clone().unwrap_or_default();

    for filename in tracked_files.iter().chain(encrypt_files.iter()) {
        let suffix_full = match filename.find("##") {
            Some(i) => &filename[i + 2..],
            None => {
                // filename == suffix (no "##" at all): skip
                continue;
            }
        };
        let conditions = match suffix_full.find('/') {
            Some(i) => suffix_full[..i].to_string(),
            None => suffix_full.to_string(),
        };
        let mut suffix = suffix_full[conditions.len()..].to_string();

        let filename_before_marker = &filename[..filename.find("##").unwrap()];
        let mut target = format!("{}/{}", ctx.base, filename_before_marker);
        let alt_prefix = format!("{}/", ctx.alt_dir);
        if let Some(stripped) = target.strip_prefix(&alt_prefix) {
            target = format!("{}/{}", ctx.base, stripped);
        }
        let source = format!("{}/{}", ctx.base, filename);

        // Legacy dir-alt symlink cleanup: if conditions carry a path suffix
        // (this is a per-file alt inside a directory target), and the
        // directory-level target is a stale legacy symlink pointing at the
        // alt-dir source (minus the suffix), remove it.
        if !suffix.is_empty() {
            let other = format!(
                "{}/{}",
                ctx.base,
                filename.strip_suffix(&suffix).unwrap_or(filename.as_str())
            );
            if is_ef(&target, &other) {
                let _ = std::fs::remove_file(&target);
            }
            target = format!("{target}{suffix}");
        }
        suffix.clear();

        // Remove target if it's a symlink pointing at source
        if is_ef(&target, &source) {
            let _ = std::fs::remove_file(&target);
        }

        score_file(
            ctx,
            &values,
            loud,
            &mut invalid_alt,
            &mut candidates,
            &source,
            &target,
            &conditions,
        );
    }

    alt_linking(ctx, &values, loud, do_copy, &candidates);
    ctx.invalid_alt = invalid_alt;
    report_invalid_alts(ctx);
}

/// `[ -L target ] && [ target -ef other ]`: true only if target is a symlink
/// and both paths resolve (canonicalize) to the same file.
fn is_ef(target: &str, other: &str) -> bool {
    let target_path = std::path::Path::new(target);
    match target_path.symlink_metadata() {
        Ok(m) if m.file_type().is_symlink() => {}
        _ => return false,
    }
    match (std::fs::canonicalize(target), std::fs::canonicalize(other)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// yadm's alt_linking (yadm:696-734).
fn alt_linking(
    ctx: &Context,
    values: &LocalValues,
    loud: bool,
    do_copy: bool,
    candidates: &[AltCandidate],
) {
    let mut exclude_entries: Vec<String> = Vec::new();

    for candidate in candidates {
        let target = &candidate.target;
        let source = &candidate.source;
        let template_processor = &candidate.template_processor;

        let target_path = std::path::Path::new(target);
        let is_symlink = target_path
            .symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        if is_symlink {
            let _ = std::fs::remove_file(target);
        } else if target_path.is_dir() {
            println!("Skipping alt {source} as {target} is a directory");
            continue;
        } else {
            paths::assert_parent(target);
        }

        if !template_processor.is_empty() {
            template::template(ctx, template_processor, source, target, values, loud);
        } else if do_copy {
            if loud {
                println!("Copying {source} to {target}");
            } else {
                util::debug(ctx, &format!("Copying {source} to {target}"));
            }
            let _ = std::fs::copy(source, target);
        } else if target_path.exists() {
            println!("Skipping alt {source} as {target} exists");
            continue;
        } else {
            if loud {
                println!("Linking {source} to {target}");
            } else {
                util::debug(ctx, &format!("Linking {source} to {target}"));
            }
            ln_relative(source, target);
        }

        // bash: ${target#"$YADM_WORK"} strips the YADM_WORK prefix once, if present
        let stripped = target.strip_prefix(&ctx.work).unwrap_or(target);
        exclude_entries.push(stripped.to_string());
    }

    exclude::update_exclude(ctx, "alt", &exclude_entries);
}

/// yadm's ln_relative (yadm:736-745).
fn ln_relative(source: &str, target: &str) {
    let rel_source = paths::relative_path(&paths::builtin_dirname(target), source);
    let _ = std::os::unix::fs::symlink(&rel_source, target);
}

/// yadm's report_invalid_alts (yadm:620-652).
pub fn report_invalid_alts(ctx: &Context) {
    if ctx.legacy_warning_issued {
        return;
    }
    if ctx.invalid_alt.is_empty() {
        return;
    }
    let mut path_list = String::new();
    for invalid in &ctx.invalid_alt {
        path_list.push_str(&format!("    * {invalid}\n"));
    }
    // raw string: `\n\` continuations would strip the leading indentation
    eprint!(
        r#"
**WARNING**
  Invalid alternates have been detected.

  Beginning with version 2.0.0, yadm uses a new naming convention for alternate
  files. Read more about this change here:

    https://yadm.io/docs/upgrade_from_1

  Or to learn more about alternates in general, read:

    https://yadm.io/docs/alternates

  To rename the invalid alternates run:

    yadm mv <old name> <new name>

  Invalid alternates detected:
{path_list}
***********

"#
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> LocalValues {
        LocalValues {
            class: "testClass".into(),
            classes: vec!["testClass".into()],
            arch: "testARch".into(),
            system: "TESTsystem".into(),
            host: "testHost".into(),
            user: "testUser".into(),
            distro: "testDISTro".into(),
            distro_family: String::new(),
        }
    }

    fn ctx_for_tests() -> Context {
        Context::new()
    }

    fn score(conditions: &str) -> i64 {
        let ctx = ctx_for_tests();
        let vals = values();
        let mut invalid_alt = Vec::new();
        let mut candidates = Vec::new();
        score_file(
            &ctx,
            &vals,
            false,
            &mut invalid_alt,
            &mut candidates,
            "source",
            "/nonexistent/target/for/tests",
            conditions,
        );
        candidates.first().map(|c| c.score).unwrap_or(0)
    }

    #[test]
    fn split_conditions_matches_bash_ifs_read() {
        assert_eq!(split_conditions(""), Vec::<String>::new());
        assert_eq!(split_conditions("a"), vec!["a"]);
        assert_eq!(split_conditions("a,b"), vec!["a", "b"]);
        assert_eq!(split_conditions("a,b,"), vec!["a", "b"]);
        assert_eq!(split_conditions(",a"), vec!["", "a"]);
        assert_eq!(split_conditions("a,,b"), vec!["a", "", "b"]);
    }

    #[test]
    fn extension_never_affects_score() {
        assert_eq!(score("u.testUser"), 1064);
        assert_eq!(score("u.testUser,e.xyz"), 1064);
        assert_eq!(score("u.testUser,extension.xyz"), 1064);
    }

    #[test]
    fn arch_os_distro_class_hostname_user_match() {
        assert_eq!(score("a.testARch"), 1001);
        assert_eq!(score("arch.testARch"), 1001);
        assert_eq!(score("o.TESTsystem"), 1002);
        assert_eq!(score("os.TESTsystem"), 1002);
        assert_eq!(score("d.testDISTro"), 1004);
        assert_eq!(score("distro.testDISTro"), 1004);
        assert_eq!(score("c.testClass"), 1016);
        assert_eq!(score("class.testClass"), 1016);
        assert_eq!(score("h.testHost"), 1032);
        assert_eq!(score("hostname.testHost"), 1032);
        assert_eq!(score("u.testUser"), 1064);
        assert_eq!(score("user.testUser"), 1064);
    }

    #[test]
    fn case_insensitive_matching() {
        assert_eq!(score("arch.testarch"), 1001);
        assert_eq!(score("ARCH.testARch"), 1001);
    }

    #[test]
    fn non_matching_condition_aborts_with_zero_score() {
        assert_eq!(score("arch.somethingelse"), 0);
        assert_eq!(score("class.testClass,arch.somethingelse"), 0);
    }

    #[test]
    fn default_contributes_1000() {
        assert_eq!(score("default"), 1000);
    }

    #[test]
    fn negated_default_is_invalid() {
        let ctx = ctx_for_tests();
        let vals = values();
        let mut invalid_alt = Vec::new();
        let mut candidates = Vec::new();
        score_file(
            &ctx,
            &vals,
            false,
            &mut invalid_alt,
            &mut candidates,
            "srcpath",
            "/nonexistent/target",
            "~default",
        );
        assert_eq!(invalid_alt, vec!["srcpath".to_string()]);
        // negate(default) -> delta stays negate?1:-1 == 1 pre-negation, but
        // default branch under negate pushes to INVALID_ALT and leaves delta
        // at its init value (1 for negate=1); then delta = -delta = -1 < 0 -> abort
        assert!(candidates.is_empty());
    }

    #[test]
    fn unknown_label_is_invalid_and_aborts() {
        let ctx = ctx_for_tests();
        let vals = values();
        let mut invalid_alt = Vec::new();
        let mut candidates = Vec::new();
        score_file(
            &ctx,
            &vals,
            false,
            &mut invalid_alt,
            &mut candidates,
            "srcpath",
            "/nonexistent/target",
            "bogus.value",
        );
        assert_eq!(invalid_alt, vec!["srcpath".to_string()]);
        assert!(candidates.is_empty());
    }

    #[test]
    fn distro_and_family_space_underscore_case_insensitive() {
        let ctx = ctx_for_tests();
        let mut vals = values();
        vals.distro = "test distro".into();
        vals.distro_family = "test family".into();

        let run = |cond: &str, vals: &LocalValues| -> i64 {
            let mut invalid_alt = Vec::new();
            let mut candidates = Vec::new();
            score_file(
                &ctx,
                vals,
                false,
                &mut invalid_alt,
                &mut candidates,
                "source",
                "/nonexistent/target",
                cond,
            );
            candidates.first().map(|c| c.score).unwrap_or(0)
        };

        assert_eq!(run("distro.Test Distro", &vals), 1004);
        assert_eq!(run("distro.test-distro", &vals), 0);
        assert_eq!(run("distro.test_distro", &vals), 1004);
        assert_eq!(run("distro_family.test FAMILY", &vals), 1008);
        assert_eq!(run("distro_family.test-family", &vals), 0);
        assert_eq!(run("distro_family.test_family", &vals), 1008);
    }

    #[test]
    fn negative_class_condition() {
        let ctx = ctx_for_tests();
        let mut vals = values();
        vals.class = "testclass".into();
        vals.classes = vec!["testclass".into()];

        let run = |cond: &str| -> i64 {
            let mut invalid_alt = Vec::new();
            let mut candidates = Vec::new();
            score_file(
                &ctx,
                &vals,
                false,
                &mut invalid_alt,
                &mut candidates,
                "source",
                "/nonexistent/target",
                cond,
            );
            candidates.first().map(|c| c.score).unwrap_or(0)
        };

        assert_eq!(run("~class.testclass"), 0);
        assert_eq!(run("~class.badclass"), 16);
        assert_eq!(run("~c.badclass"), 16);
    }

    #[test]
    fn negative_combined_conditions() {
        let ctx = ctx_for_tests();
        let mut vals = values();
        vals.class = "testclass".into();
        vals.classes = vec!["testclass".into()];
        vals.distro = "testdistro".into();

        let run = |cond: &str| -> i64 {
            let mut invalid_alt = Vec::new();
            let mut candidates = Vec::new();
            score_file(
                &ctx,
                &vals,
                false,
                &mut invalid_alt,
                &mut candidates,
                "source",
                "/nonexistent/target",
                cond,
            );
            candidates.first().map(|c| c.score).unwrap_or(0)
        };

        assert_eq!(run("~class.testclass,~distro.testdistro"), 0);
        assert_eq!(run("class.testclass,distro.testdistro"), 2020);
        assert_eq!(run("~class.badclass,~distro.testdistro"), 0);
        assert_eq!(run("class.testclass,~distro.baddistro"), 1020);
        assert_eq!(run("class.testclass,~class.badclass"), 1032);
    }

    // ---- record_score ----

    fn rec(candidates: &mut Vec<AltCandidate>, score: i64, target: &str, source: &str, tp: &str) {
        record_score(
            candidates,
            score,
            target,
            source,
            tp,
            "/config/does/not/match",
        );
    }

    #[test]
    fn dont_record_zeros() {
        let mut candidates = Vec::new();
        rec(&mut candidates, 0, "testtgt", "testsrc", "");
        assert!(candidates.is_empty());
    }

    #[test]
    fn new_scores_recorded_in_order() {
        let mut candidates = Vec::new();
        rec(&mut candidates, 1, "t1", "s1", "");
        rec(&mut candidates, 2, "t2", "s2", "");
        rec(&mut candidates, 4, "t3", "s3", "");
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].target, "t1");
        assert_eq!(candidates[1].target, "t2");
        assert_eq!(candidates[2].target, "t3");
    }

    #[test]
    fn existing_scores_lower_keeps_existing() {
        let mut candidates = vec![AltCandidate {
            target: "t".into(),
            source: "existing_src".into(),
            score: 2,
            template_processor: String::new(),
        }];
        rec(&mut candidates, 1, "t", "new_src", "");
        assert_eq!(candidates[0].score, 2);
        assert_eq!(candidates[0].source, "existing_src");
    }

    #[test]
    fn existing_scores_equal_keeps_existing() {
        let mut candidates = vec![AltCandidate {
            target: "t".into(),
            source: "existing_src".into(),
            score: 2,
            template_processor: String::new(),
        }];
        rec(&mut candidates, 2, "t", "new_src", "");
        assert_eq!(candidates[0].score, 2);
        assert_eq!(candidates[0].source, "existing_src");
    }

    #[test]
    fn existing_scores_higher_replaces() {
        let mut candidates = vec![AltCandidate {
            target: "t".into(),
            source: "existing_src".into(),
            score: 2,
            template_processor: String::new(),
        }];
        rec(&mut candidates, 4, "t", "new_src", "");
        assert_eq!(candidates[0].score, 4);
        assert_eq!(candidates[0].source, "new_src");
    }

    #[test]
    fn existing_template_beats_new_nontemplate() {
        let mut candidates = vec![AltCandidate {
            target: "t".into(),
            source: "existing_src".into(),
            score: 1,
            template_processor: "existing_template".into(),
        }];
        rec(&mut candidates, 2, "t", "new_src", "");
        assert_eq!(candidates[0].source, "existing_src");
        assert_eq!(candidates[0].template_processor, "existing_template");
    }

    #[test]
    fn config_target_is_prepended() {
        let config_file = "/config/file";
        let mut candidates = Vec::new();
        record_score(&mut candidates, 1, "t1", "s1", "", config_file);
        record_score(&mut candidates, 2, "t2", "s2", "", config_file);
        record_score(&mut candidates, 3, config_file, "sconfig", "", config_file);
        record_score(&mut candidates, 4, "t3", "s3", "", config_file);

        assert_eq!(candidates[0].target, config_file);
        assert_eq!(candidates[1].target, "t1");
        assert_eq!(candidates[2].target, "t2");
        assert_eq!(candidates[3].target, "t3");
    }

    #[test]
    fn new_templates_recorded_at_zero_score() {
        let mut candidates = Vec::new();
        rec(&mut candidates, 0, "t1", "s1", "proc1");
        rec(&mut candidates, 0, "t2", "s2", "proc2");
        rec(&mut candidates, 0, "t3", "s3", "proc3");
        assert_eq!(candidates.len(), 3);
    }

    #[test]
    fn overwrite_existing_template_tie_goes_to_new() {
        let mut candidates = vec![AltCandidate {
            target: "t".into(),
            source: "existing_src".into(),
            score: 0,
            template_processor: "existing_processor".into(),
        }];
        rec(&mut candidates, 0, "t", "new_src", "new_processor");
        assert_eq!(candidates[0].source, "new_src");
        assert_eq!(candidates[0].template_processor, "new_processor");
    }

    // ---- report_invalid_alts ----

    #[test]
    fn report_invalid_alts_noop_when_empty_or_legacy_issued() {
        let mut ctx = Context::new();
        ctx.invalid_alt = Vec::new();
        ctx.legacy_warning_issued = false;
        report_invalid_alts(&ctx); // no panic, no assertion on stderr here

        ctx.invalid_alt = vec!["file##invalid".to_string()];
        ctx.legacy_warning_issued = true;
        report_invalid_alts(&ctx); // suppressed, no panic
    }
}
