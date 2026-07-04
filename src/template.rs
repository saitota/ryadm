//! Template processing dispatch and the external processors (esh, j2, envtpl).
//! Reference: yadm script lines 307-539. Spec: scratchpad specs template.md.

use std::process::{Command, Stdio};

use crate::alt::LocalValues;
use crate::context::Context;
use crate::template_default;
use crate::util;

/// yadm's choose_template_processor: "" when none is supported.
pub fn choose_template_processor(ctx: &Context, kind: &str) -> String {
    let default_kind = if kind.is_empty() { "default" } else { kind };
    if default_kind == "default" {
        if util::command_exists(&ctx.awk_program[0]) {
            return "default".to_string();
        }
    } else if kind == "esh" {
        if util::command_exists(&ctx.esh_program) {
            return "esh".to_string();
        }
    } else if (kind == "j2cli" || kind == "j2") && util::command_exists(&ctx.j2cli_program) {
        return "j2cli".to_string();
    } else if (kind == "envtpl" || kind == "j2") && util::command_exists(&ctx.envtpl_program) {
        return "envtpl".to_string();
    }
    String::new()
}

/// `[ -r path ]`
fn is_readable(path: &str) -> bool {
    std::fs::File::open(path).is_ok()
}

/// `[ -w path ]` — owner/group/other write bit set (best-effort, mirrors the
/// simple permission-bit check `template()` needs; not a full access(2) check).
fn is_writable(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(m) => m.permissions().mode() & 0o200 != 0,
        Err(_) => false,
    }
}

/// yadm's template(): render input via the processor and write output
/// (skip-if-unchanged, temp file + rename, copy_perms).
pub fn template(
    ctx: &Context,
    processor: &str,
    input: &str,
    output: &str,
    values: &LocalValues,
    loud: bool,
) {
    let content = match render(ctx, processor, input, values) {
        Ok(c) => util::trim_trailing_newlines(&c),
        Err(()) => {
            eprintln!("Error: failed to process template '{input}'");
            return;
        }
    };

    if is_readable(output) {
        if let Ok(existing) = std::fs::read_to_string(output) {
            if content == util::trim_trailing_newlines(&existing) {
                util::debug(ctx, &format!("Template output '{output}' is unchanged"));
                return;
            }
        }
    }

    if std::path::Path::new(output).exists() && !is_writable(output) {
        if let Ok(meta) = std::fs::metadata(output) {
            use std::os::unix::fs::PermissionsExt;
            let mode = meta.permissions().mode() | 0o200;
            let _ = std::fs::set_permissions(output, std::fs::Permissions::from_mode(mode));
        }
    }

    if loud {
        println!("Creating {output} from template {input}");
    } else {
        util::debug(ctx, &format!("Creating {output} from template {input}"));
    }

    let pid = std::process::id();
    let random: u32 = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        nanos % 32768
    };
    let temp_file = format!("{output}.{pid}.{random}");

    let write_ok = std::fs::write(&temp_file, format!("{content}\n")).is_ok()
        && std::fs::rename(&temp_file, output).is_ok();
    if write_ok {
        util::copy_perms(ctx, input, output);
    } else {
        println!("Error: failed to create template output '{output}'");
        let _ = std::fs::remove_file(&temp_file);
    }
}

fn render(ctx: &Context, processor: &str, input: &str, values: &LocalValues) -> Result<String, ()> {
    match processor {
        "default" => template_default::template_default(ctx, input, values),
        "esh" => template_esh(ctx, input, values),
        "j2cli" => template_j2cli(ctx, input, values),
        "envtpl" => template_envtpl(ctx, input, values),
        _ => Err(()),
    }
}

fn capture_stdout(mut cmd: Command) -> Result<String, ()> {
    cmd.stderr(Stdio::inherit());
    match cmd.output() {
        Ok(out) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        _ => Err(()),
    }
}

fn template_j2cli(ctx: &Context, input: &str, values: &LocalValues) -> Result<String, ()> {
    let mut cmd = Command::new(&ctx.j2cli_program);
    cmd.arg(input);
    set_yadm_env(&mut cmd, input, values);
    capture_stdout(cmd)
}

fn template_envtpl(ctx: &Context, input: &str, values: &LocalValues) -> Result<String, ()> {
    let mut cmd = Command::new(&ctx.envtpl_program);
    cmd.args(["-o", "-", "--keep-template", input]);
    set_yadm_env(&mut cmd, input, values);
    capture_stdout(cmd)
}

fn template_esh(ctx: &Context, input: &str, values: &LocalValues) -> Result<String, ()> {
    let mut cmd = Command::new(&ctx.esh_program);
    cmd.arg(input);
    cmd.arg(format!("YADM_CLASS={}", values.class));
    cmd.arg(format!("YADM_ARCH={}", values.arch));
    cmd.arg(format!("YADM_OS={}", values.system));
    cmd.arg(format!("YADM_HOSTNAME={}", values.host));
    cmd.arg(format!("YADM_USER={}", values.user));
    cmd.arg(format!("YADM_DISTRO={}", values.distro));
    cmd.arg(format!("YADM_DISTRO_FAMILY={}", values.distro_family));
    cmd.arg(format!("YADM_SOURCE={input}"));
    cmd.env("YADM_CLASSES", values.classes.join("\n"));
    capture_stdout(cmd)
}

fn set_yadm_env(cmd: &mut Command, input: &str, values: &LocalValues) {
    cmd.env("YADM_CLASS", &values.class);
    cmd.env("YADM_ARCH", &values.arch);
    cmd.env("YADM_OS", &values.system);
    cmd.env("YADM_HOSTNAME", &values.host);
    cmd.env("YADM_USER", &values.user);
    cmd.env("YADM_DISTRO", &values.distro);
    cmd.env("YADM_DISTRO_FAMILY", &values.distro_family);
    cmd.env("YADM_SOURCE", input);
    cmd.env("YADM_CLASSES", values.classes.join("\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    const MISSING: &str = "radm-definitely-not-a-real-program-xyz-123";

    fn ctx_with(awk_available: bool, esh: bool, j2cli: bool, envtpl: bool) -> Context {
        let mut ctx = Context::new();
        ctx.awk_program = vec![if awk_available {
            "true".to_string()
        } else {
            MISSING.to_string()
        }];
        ctx.esh_program = if esh {
            "true".to_string()
        } else {
            MISSING.to_string()
        };
        ctx.j2cli_program = if j2cli {
            "true".to_string()
        } else {
            MISSING.to_string()
        };
        ctx.envtpl_program = if envtpl {
            "true".to_string()
        } else {
            MISSING.to_string()
        };
        ctx
    }

    // §1 test_kind_default: awk_available x label -> expected
    #[test]
    fn choose_kind_default_matrix() {
        let cases = [
            (true, "", "default"),
            (true, "default", "default"),
            (true, "other", ""),
            (false, "", ""),
            (false, "default", ""),
            (false, "other", ""),
        ];
        for (awk, label, expected) in cases {
            let ctx = ctx_with(awk, false, false, false);
            assert_eq!(
                choose_template_processor(&ctx, label),
                expected,
                "awk={awk} label={label}"
            );
        }
    }

    // §1 test_kind_j2cli_envtpl: label x envtpl x j2cli -> expected
    #[test]
    fn choose_kind_j2cli_envtpl_matrix() {
        let cases: &[(&str, bool, bool, &str)] = &[
            ("envtpl", true, true, "envtpl"),
            ("envtpl", true, false, ""),
            ("envtpl", false, true, "envtpl"),
            ("envtpl", false, false, ""),
            ("j2cli", true, true, "j2cli"),
            ("j2cli", true, false, "j2cli"),
            ("j2cli", false, true, ""),
            ("j2cli", false, false, ""),
            ("j2", true, true, "j2cli"),
            ("j2", true, false, "j2cli"),
            ("j2", false, true, "envtpl"),
            ("j2", false, false, ""),
            ("other", true, true, ""),
            ("other", true, false, ""),
            ("other", false, true, ""),
            ("other", false, false, ""),
        ];
        // table columns are (label, j2cli_avail, envtpl_avail, expected)
        for (label, j2cli_avail, envtpl_avail, expected) in cases.iter().copied() {
            let ctx = ctx_with(false, false, j2cli_avail, envtpl_avail);
            assert_eq!(
                choose_template_processor(&ctx, label),
                expected,
                "label={label} j2cli={j2cli_avail} envtpl={envtpl_avail}"
            );
        }
    }

    #[test]
    fn choose_esh() {
        let ctx = ctx_with(false, true, false, false);
        assert_eq!(choose_template_processor(&ctx, "esh"), "esh");
        let ctx = ctx_with(false, false, false, false);
        assert_eq!(choose_template_processor(&ctx, "esh"), "");
    }

    fn values() -> LocalValues {
        LocalValues {
            class: "c".into(),
            classes: vec!["c".into()],
            arch: "a".into(),
            system: "s".into(),
            host: "h".into(),
            user: "u".into(),
            distro: "d".into(),
            distro_family: "f".into(),
        }
    }

    fn unique_dir(hint: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "radm-template-test-{}-{}-{}",
            std::process::id(),
            n,
            hint
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn template_writes_rendered_content_with_trailing_newline() {
        let dir = unique_dir("write");
        let input = dir.join("input");
        std::fs::write(&input, "hello {{yadm.user}}").unwrap();
        let output = dir.join("output");

        let ctx = Context::new();
        template(
            &ctx,
            "default",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            &values(),
            false,
        );

        let content = std::fs::read_to_string(&output).unwrap();
        assert_eq!(content, "hello u\n");
    }

    #[test]
    fn template_copies_source_perms_onto_output() {
        use std::os::unix::fs::PermissionsExt;
        let dir = unique_dir("perms");
        let input = dir.join("input");
        std::fs::write(&input, "content").unwrap();
        std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o754)).unwrap();
        let output = dir.join("output");
        std::fs::write(&output, "existing").unwrap();
        std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o400)).unwrap();

        let ctx = Context::new();
        template(
            &ctx,
            "default",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            &values(),
            false,
        );

        let out_mode = std::fs::metadata(&output).unwrap().permissions().mode() & 0o777;
        assert_eq!(out_mode, 0o754);
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "content\n");
    }

    #[test]
    fn template_noop_when_output_already_matches_content() {
        use std::os::unix::fs::PermissionsExt;
        let dir = unique_dir("noop");
        let input = dir.join("input");
        std::fs::write(&input, "same content").unwrap();
        let output = dir.join("output");
        // Existing output already equals rendered content (trailing newlines
        // stripped per $(<file) semantics) but with a distinct mode we can
        // detect was NOT overwritten (copy_perms would change it otherwise).
        std::fs::write(&output, "same content\n\n\n").unwrap();
        std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o600)).unwrap();

        let ctx = Context::new();
        template(
            &ctx,
            "default",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            &values(),
            false,
        );

        let out_mode = std::fs::metadata(&output).unwrap().permissions().mode() & 0o777;
        assert_eq!(out_mode, 0o600, "unchanged output must not be rewritten");
        assert_eq!(
            std::fs::read_to_string(&output).unwrap(),
            "same content\n\n\n"
        );
    }

    #[test]
    fn template_processor_failure_leaves_output_untouched() {
        let dir = unique_dir("fail");
        let input = dir.join("input");
        std::fs::write(&input, "{% endif %}\n").unwrap(); // triggers an engine error
        let output = dir.join("output");

        let ctx = Context::new();
        template(
            &ctx,
            "default",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            &values(),
            false,
        );

        assert!(!output.exists());
    }
}
