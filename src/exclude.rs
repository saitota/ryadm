//! Managed section of the repo's info/exclude (yadm's update_exclude).
//! Reference: yadm script lines 1487-1553.

use crate::config;
use crate::context::Context;
use crate::paths;
use crate::util;

/// yadm's update_exclude: maintain the "# yadm-auto-excludes" managed block
/// in $YADM_REPO/info/exclude. `suffix` is "alt" or "encrypt".
pub fn update_exclude(ctx: &Context, suffix: &str, entries: &[String]) {
    let auto_exclude = config::config_output(ctx, &["--bool", "yadm.auto-exclude"]);
    if auto_exclude == "false" {
        return;
    }

    let exclude_path = format!("{}/info/exclude", ctx.repo);
    let part_path = format!("{exclude_path}.yadm-{suffix}");
    let part_str = entries.join("\n");

    if std::path::Path::new(&part_path).exists() {
        // bash: $(<"$part_path") strips all trailing newlines, like $(...)
        let existing = std::fs::read_to_string(&part_path).unwrap_or_default();
        let existing = util::trim_trailing_newlines(&existing);
        if part_str == existing {
            return;
        }
        let _ = std::fs::remove_file(&part_path);
    } else if part_str.is_empty() {
        return;
    }

    if !part_str.is_empty() {
        paths::assert_parent(&part_path);
        // bash: cat >"$part_path" <<<"$part_str" appends exactly one newline
        let _ = std::fs::write(&part_path, format!("{part_str}\n"));
    }

    let exclude_flag = "# yadm-auto-excludes";
    let exclude_header = format!(
        "{exclude_flag}\n# This section is managed by yadm.\n# Any edits below will be lost.\n"
    );

    // read info/exclude, splitting into unmanaged (before the flag line) and
    // managed (from the flag line onward) sections.
    let mut unmanaged = String::new();
    let mut managed = String::new();
    if let Some(lines) = util::read_lines(&exclude_path) {
        let mut flag_seen = false;
        for line in lines {
            if line == exclude_flag {
                flag_seen = true;
            }
            if flag_seen {
                managed.push_str(&line);
                managed.push('\n');
            } else {
                unmanaged.push_str(&line);
                unmanaged.push('\n');
            }
        }
    }

    let mut exclude_str = String::new();
    for s in ["alt", "encrypt"] {
        let p = format!("{exclude_path}.yadm-{s}");
        if std::path::Path::new(&p).exists() {
            exclude_str.push_str(&format!("# yadm {s}\n"));
            let content = std::fs::read_to_string(&p).unwrap_or_default();
            exclude_str.push_str(util::trim_trailing_newlines(&content).as_str());
        }
    }

    if format!("{exclude_header}{exclude_str}\n") != managed {
        util::debug(ctx, &format!("Updating {exclude_path}"));
        // bash: cat >"$exclude_path" <<<"${unmanaged}${exclude_header}${exclude_str}"
        // appends exactly one trailing newline via the here-string.
        let _ = std::fs::write(
            &exclude_path,
            format!("{unmanaged}{exclude_header}{exclude_str}\n"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Context;

    fn test_ctx(repo: &str) -> Context {
        let mut ctx = Context::new();
        ctx.repo = repo.to_string();
        // point config_file somewhere harmless; config_output for
        // "yadm.auto-exclude" isn't a local.* option so it reads the yadm
        // config file (which won't exist) -> empty string, auto_exclude stays
        // enabled (not "false").
        ctx.config_file = format!("{repo}/config");
        ctx
    }

    #[test]
    fn writes_alt_entries_and_managed_block() {
        let tmp =
            std::env::temp_dir().join(format!("ryadm-exclude-test-{}-{}", std::process::id(), "a"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("info")).unwrap();
        let ctx = test_ctx(tmp.to_str().unwrap());

        update_exclude(&ctx, "alt", &["foo".to_string(), "bar".to_string()]);

        let exclude_path = tmp.join("info/exclude");
        let content = std::fs::read_to_string(&exclude_path).unwrap();
        assert!(content.contains("# yadm-auto-excludes"));
        assert!(content.contains("# yadm alt\nfoo\nbar"));

        let part = std::fs::read_to_string(tmp.join("info/exclude.yadm-alt")).unwrap();
        assert_eq!(part, "foo\nbar\n");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn empty_entries_noop_when_no_part_file() {
        let tmp =
            std::env::temp_dir().join(format!("ryadm-exclude-test-{}-{}", std::process::id(), "b"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("info")).unwrap();
        let ctx = test_ctx(tmp.to_str().unwrap());

        update_exclude(&ctx, "alt", &[]);

        assert!(!tmp.join("info/exclude").exists());
        assert!(!tmp.join("info/exclude.yadm-alt").exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn unchanged_part_content_is_noop() {
        let tmp =
            std::env::temp_dir().join(format!("ryadm-exclude-test-{}-{}", std::process::id(), "c"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("info")).unwrap();
        let ctx = test_ctx(tmp.to_str().unwrap());

        update_exclude(&ctx, "alt", &["same".to_string()]);
        let exclude_path = tmp.join("info/exclude");
        let mtime1 = std::fs::metadata(&exclude_path)
            .unwrap()
            .modified()
            .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));
        update_exclude(&ctx, "alt", &["same".to_string()]);
        let mtime2 = std::fs::metadata(&exclude_path)
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(mtime1, mtime2);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn removing_entries_clears_alt_section() {
        let tmp =
            std::env::temp_dir().join(format!("ryadm-exclude-test-{}-{}", std::process::id(), "d"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("info")).unwrap();
        let ctx = test_ctx(tmp.to_str().unwrap());

        update_exclude(&ctx, "alt", &["foo".to_string()]);
        assert!(tmp.join("info/exclude.yadm-alt").exists());

        update_exclude(&ctx, "alt", &[]);
        assert!(!tmp.join("info/exclude.yadm-alt").exists());
        let content = std::fs::read_to_string(tmp.join("info/exclude")).unwrap();
        assert!(!content.contains("# yadm alt"));
        assert!(content.contains("# yadm-auto-excludes"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
