//! Pure-Rust port of yadm's built-in "default" template engine (the awk
//! program embedded in template_default, yadm script lines 362-494).
//! Must be behavior-identical, including error messages "file:line: error: ..."
//! printed to stderr.

use crate::alt::LocalValues;
use crate::context::Context;
use crate::paths;

/// One open file in the include stack: its path, the lines it contains (split
/// on '\n', matching awk `getline`'s line-by-line reads), and the index of the
/// next line to read.
struct OpenFile {
    filename: String,
    lines: Vec<String>,
    /// Index of the next line to read from `lines`.
    next: usize,
    /// 1-based line number of the last line read (yadm's `line[current]`).
    line_no: usize,
}

impl OpenFile {
    /// awk `getline`: returns the raw line and advances, or None at EOF.
    /// Increments the per-file line counter on a successful read, mirroring
    /// yadm's `++line[current]` right after the read.
    fn getline(&mut self) -> Option<String> {
        if self.next >= self.lines.len() {
            return None;
        }
        let idx = self.next;
        self.next += 1;
        self.line_no += 1;
        Some(self.lines[idx].clone())
    }
}

/// Read a file's contents split into "getline" lines. A trailing newline
/// produces no extra empty final line (awk's getline never yields an empty
/// line for the text after a final "\n"); a file with no trailing newline
/// still yields its last (partial) line, matching awk's getline behavior.
/// Returns None if the file could not be read (open/read failure).
fn read_getline_lines(path: &str) -> Option<Vec<String>> {
    let content = std::fs::read_to_string(path).ok()?;
    if content.is_empty() {
        return Some(Vec::new());
    }
    let mut lines: Vec<String> = content.split('\n').map(|s| s.to_string()).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    Some(lines)
}

/// Print the awk `error()` function's exact stderr line:
/// `"%s:%d: error: %s\n" % (filename, line, text)`.
fn print_error(filename: &str, line_no: usize, text: &str) {
    eprintln!("{filename}:{line_no}: error: {text}");
}

/// `(env|yadm)\.[a-zA-Z0-9_]+` — find the first match at-or-after `start`.
/// Returns (match_start, match_end) byte offsets into `s`.
fn find_variable(s: &str, start: usize) -> Option<(usize, usize)> {
    let bytes = s.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        // the pattern is pure ASCII, so a match can never start inside a
        // multibyte character; skip non-boundaries instead of panicking
        if !s.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let rest = &s[i..];
        let prefix = if rest.starts_with("env.") {
            Some("env.".len())
        } else if rest.starts_with("yadm.") {
            Some("yadm.".len())
        } else {
            None
        };
        if let Some(plen) = prefix {
            let ident_start = i + plen;
            let mut j = ident_start;
            while j < bytes.len() {
                let c = bytes[j] as char;
                if c.is_ascii_alphanumeric() || c == '_' {
                    j += 1;
                } else {
                    break;
                }
            }
            if j > ident_start {
                return Some((i, j));
            }
        }
        i += 1;
    }
    None
}

/// Matches the full-line `{% if VARIABLE [!=]= "..." %}` directive against
/// the RAW line. Returns (lhs variable text e.g. "yadm.class", op "=="/"!=",
/// rhs literal text WITHOUT surrounding quotes) on match.
fn match_if_directive(line: &str) -> Option<(String, String, String)> {
    let trimmed_start = line.trim_start_matches([' ', '\t']);
    let rest = trimmed_start.strip_prefix("{%")?;
    let rest = rest.trim_start_matches([' ', '\t']);
    let rest = rest.strip_prefix("if")?;
    // require at least one space/tab after "if"
    let after_if_len = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    if after_if_len == 0 {
        return None;
    }
    let rest = rest.trim_start_matches([' ', '\t']);

    let (vstart, vend) = find_variable(rest, 0)?;
    if vstart != 0 {
        return None;
    }
    let variable = rest[vstart..vend].to_string();
    let rest = &rest[vend..];
    let rest = rest.trim_start_matches([' ', '\t']);

    let op = if let Some(r) = rest.strip_prefix("==") {
        let _ = r;
        "=="
    } else if rest.strip_prefix("!=").is_some() {
        "!="
    } else {
        return None;
    };
    let rest = &rest[2..];
    let rest = rest.trim_start_matches([' ', '\t']);

    // rhs: ".*" greedy to the LAST quote on the line, then optional trailing
    // whitespace + %} must end the line exactly.
    if !rest.starts_with('"') {
        return None;
    }
    let after_quote = &rest[1..];
    let last_quote_rel = after_quote.rfind('"')?;
    let rhs = &after_quote[..last_quote_rel];
    let after_rhs = &after_quote[last_quote_rel + 1..];
    let after_rhs = after_rhs.trim_start_matches([' ', '\t']);
    let after_rhs = after_rhs.strip_suffix("%}")?;
    // only whitespace may remain between the closing quote and "%}"
    if !after_rhs.chars().all(|c| c == ' ' || c == '\t') {
        return None;
    }

    Some((variable, op.to_string(), rhs.to_string()))
}

fn is_else_directive(line: &str) -> bool {
    let t = line.trim_start_matches([' ', '\t']);
    let Some(t) = t.strip_prefix("{%") else {
        return false;
    };
    let t = t.trim_start_matches([' ', '\t']);
    let Some(t) = t.strip_prefix("else") else {
        return false;
    };
    let t = t.trim_start_matches([' ', '\t']);
    t == "%}"
}

fn is_endif_directive(line: &str) -> bool {
    let t = line.trim_start_matches([' ', '\t']);
    let Some(t) = t.strip_prefix("{%") else {
        return false;
    };
    let t = t.trim_start_matches([' ', '\t']);
    let Some(t) = t.strip_prefix("endif") else {
        return false;
    };
    let t = t.trim_start_matches([' ', '\t']);
    t == "%}"
}

/// Matches the full-line `{% include NAME %}` directive (NAME optionally
/// double-quoted). Returns the raw (unsubstituted at this stage — the caller
/// already ran replace_vars on the whole line before calling this) include
/// target text.
fn match_include_directive(line: &str) -> Option<String> {
    let t = line.trim_start_matches([' ', '\t']);
    let t = t.strip_prefix("{%")?;
    let t = t.trim_start_matches([' ', '\t']);
    let t = t.strip_prefix("include")?;
    let after_len = t.len() - t.trim_start_matches([' ', '\t']).len();
    if after_len == 0 {
        return None;
    }
    let t = t.trim_start_matches([' ', '\t']);

    // NAME is "[^"]+" or bare [^"]+: whatever precedes the trailing [ \t]*%}
    let t_trimmed_end = t;
    let without_close = t_trimmed_end.strip_suffix("%}")?;
    let mut name_part = without_close;
    let end_trim = name_part.trim_end_matches([' ', '\t']);
    name_part = end_trim;

    if name_part.is_empty() {
        return None;
    }

    let name = if let Some(rest) = name_part.strip_prefix('"') {
        // must end with a matching quote and contain no other quote inside
        // (regex is "[^"]+" — no internal quotes allowed).
        let inner = rest.strip_suffix('"')?;
        if inner.is_empty() || inner.contains('"') {
            return None;
        }
        inner.to_string()
    } else {
        if name_part.contains('"') {
            return None;
        }
        name_part.to_string()
    };

    Some(name)
}

/// yadm.filename / env.X / yadm.X substitution (`replace_vars`).
fn replace_vars(
    input: &str,
    stack: &[OpenFile],
    classes: &str,
    values: &LocalValues,
    source: &str,
) -> String {
    let mut output = String::new();
    let mut rest = input.to_string();

    loop {
        match find_braces_variable(&rest) {
            Some((start, end, data)) => {
                if start > 0 {
                    output.push_str(&rest[..start]);
                }
                let after = rest[end..].to_string();

                let mut data_no_ws = String::with_capacity(data.len());
                for c in data.chars() {
                    if c != ' ' && c != '\t' {
                        data_no_ws.push(c);
                    }
                }
                let mut fields = data_no_ws.splitn(2, '.');
                let namespace = fields.next().unwrap_or("");
                let field = fields.next().unwrap_or("");

                if namespace == "env" {
                    output.push_str(&std::env::var(field).unwrap_or_default());
                } else if field == "filename" {
                    if let Some(top) = stack.last() {
                        output.push_str(&top.filename);
                    }
                } else {
                    output.push_str(yadm_field(field, classes, values, source));
                }

                rest = after;
            }
            None => {
                output.push_str(&rest);
                break;
            }
        }
    }
    output
}

fn yadm_field<'a>(
    field: &str,
    classes: &'a str,
    values: &'a LocalValues,
    source: &'a str,
) -> &'a str {
    match field {
        "class" => &values.class,
        "classes" => classes,
        "arch" => &values.arch,
        "os" => &values.system,
        "hostname" => &values.host,
        "user" => &values.user,
        "distro" => &values.distro,
        "distro_family" => &values.distro_family,
        "source" => source,
        _ => "",
    }
}

/// Finds the first `\{\{[ \t]*VARIABLE[ \t]*\}\}` occurrence. Returns
/// (start, end, inner_data) where inner_data is the raw text between the
/// braces (whitespace not yet stripped).
fn find_braces_variable(s: &str) -> Option<(usize, usize, String)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            let after_open = i + 2;
            let rest = &s[after_open..];
            let ws1 = rest.len() - rest.trim_start_matches([' ', '\t']).len();
            let var_region = &rest[ws1..];
            if let Some((vs, ve)) = find_variable(var_region, 0) {
                if vs == 0 {
                    let after_var = &var_region[ve..];
                    let ws2 = after_var.len() - after_var.trim_start_matches([' ', '\t']).len();
                    let after_ws2 = &after_var[ws2..];
                    if let Some(closing) = after_ws2.strip_prefix("}}") {
                        let _ = closing;
                        let inner = &rest[..ws1 + ve];
                        let total_len = 2 + inner.len() + ws2 + 2;
                        return Some((i, i + total_len, inner.to_string()));
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Render `input` with the default engine. Returns Err(()) after printing the
/// awk-equivalent error to stderr (the caller prints the generic failure).
pub fn template_default(_ctx: &Context, input: &str, values: &LocalValues) -> Result<String, ()> {
    let source_dir = paths::builtin_dirname(input);
    let classes = values.classes.join("\n");

    let mut out = String::new();

    let mut stack: Vec<OpenFile> = Vec::new();
    match read_getline_lines(input) {
        Some(lines) => stack.push(OpenFile {
            filename: input.to_string(),
            lines,
            next: 0,
            line_no: 0,
        }),
        None => {
            print_error(input, 0, "could not read input file");
            return Err(());
        }
    }

    // Preserve the top-level file's line-number progress even after the
    // stack unwinds, for the "unterminated if" error (always attributed to
    // filename[0]/line[0] per yadm's `current = 0; error(...)`).
    let mut top_line_no: usize = 0;

    // if-stack: skip[level]; skip[0] == 0 always (top level never skipped).
    let mut skip: Vec<i32> = vec![0];

    'outer: while !stack.is_empty() {
        while let Some(raw) = stack.last_mut().unwrap().getline() {
            if stack.len() == 1 {
                top_line_no = stack[0].line_no;
            }
            let level = skip.len() - 1;

            if let Some((lhs_var, op, rhs_lit)) = match_if_directive(&raw) {
                if skip[level] != 0 {
                    skip.push(1);
                    continue;
                }
                // awk's tolower() in the C locale only folds ASCII A-Z
                let rhs =
                    replace_vars(&rhs_lit, &stack, &classes, values, input).to_ascii_lowercase();
                let lhs = if lhs_var == "yadm.class" {
                    let mut matched = None;
                    for cls in classes.split('\n') {
                        if rhs == cls.to_ascii_lowercase() {
                            matched = Some(rhs.clone());
                            break;
                        }
                    }
                    matched.unwrap_or_else(|| format!("not{rhs}"))
                } else {
                    replace_vars(
                        &format!("{{{{{lhs_var}}}}}"),
                        &stack,
                        &classes,
                        values,
                        input,
                    )
                    .to_ascii_lowercase()
                };
                if op == "==" {
                    skip.push(if lhs != rhs { 1 } else { 0 });
                } else {
                    skip.push(if lhs == rhs { 1 } else { 0 });
                }
                continue;
            }

            if is_else_directive(&raw) {
                let cur = stack.last().unwrap();
                let (filename, line_no) = (cur.filename.clone(), cur.line_no);
                if level == 0 || skip[level] < 0 {
                    print_error(&filename, line_no, "else without matching if");
                    return Err(());
                }
                skip[level] = if skip[level] != 0 {
                    skip[level - 1]
                } else {
                    -1
                };
                continue;
            }

            if is_endif_directive(&raw) {
                skip.pop();
                if skip.is_empty() {
                    let cur = stack.last().unwrap();
                    let (filename, line_no) = (cur.filename.clone(), cur.line_no);
                    print_error(&filename, line_no, "endif without matching if");
                    return Err(());
                }
                continue;
            }

            let level = skip.len() - 1;
            if skip[level] == 0 {
                let line = replace_vars(&raw, &stack, &classes, values, input);
                if let Some(include_name) = match_include_directive(&line) {
                    let resolved = if include_name.starts_with('/') {
                        include_name
                    } else {
                        format!("{source_dir}/{include_name}")
                    };
                    match read_getline_lines(&resolved) {
                        Some(lines) => {
                            stack.push(OpenFile {
                                filename: resolved,
                                lines,
                                next: 0,
                                line_no: 0,
                            });
                            continue 'outer;
                        }
                        None => {
                            let (filename, line_no) = {
                                let cur = stack.last().unwrap();
                                (cur.filename.clone(), cur.line_no)
                            };
                            print_error(
                                &filename,
                                line_no,
                                &format!("could not read include file '{resolved}'"),
                            );
                            return Err(());
                        }
                    }
                } else {
                    out.push_str(&line);
                    out.push('\n');
                }
            }
        }
        stack.pop();
    }

    if skip.len() - 1 > 0 {
        print_error(input, top_line_no, "unterminated if");
        return Err(());
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Create a unique temp file with the given content, returning its path.
    fn write_temp(name_hint: &str, content: &str) -> String {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "radm-template-default-test-{}-{}-{}",
            std::process::id(),
            n,
            name_hint
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name_hint);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn write_temp_in_dir(dir: &str, rel: &str, content: &str) -> String {
        let path = std::path::Path::new(dir).join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn base_values() -> LocalValues {
        LocalValues::default()
    }

    #[test]
    fn test_source_renders_input_path() {
        let input = write_temp("source_test", "{{yadm.source}}");
        let values = base_values();
        let ctx = Context::new();
        let result = template_default(&ctx, &input, &values).unwrap();
        assert_eq!(result.trim(), input);
    }

    #[test]
    fn test_env_renders_pwd() {
        let input = write_temp("env_test", "{{env.PWD}}");
        let values = base_values();
        let ctx = Context::new();
        let pwd = std::env::var("PWD").unwrap_or_default();
        let result = template_default(&ctx, &input, &values).unwrap();
        assert_eq!(result.trim(), pwd);
    }

    #[test]
    fn test_multibyte_text_passes_through() {
        let input = write_temp("multibyte", "日本語 テスト 🎉\n日本語{{yadm.os}}後置き\n");
        let values = LocalValues {
            system: "TestOS".to_string(),
            ..base_values()
        };
        let ctx = Context::new();
        let result = template_default(&ctx, &input, &values).unwrap();
        assert_eq!(result, "日本語 テスト 🎉\n日本語TestOS後置き\n");
    }

    #[test]
    fn test_braced_multibyte_is_not_a_variable() {
        // `{{ 日本語 }}` doesn't match the ASCII-only VARIABLE pattern and
        // must pass through verbatim (this used to panic on the non-ASCII
        // byte after "{{").
        let input = write_temp("multibyte_braces", "{{ 日本語 }}\nplain\n");
        let values = base_values();
        let ctx = Context::new();
        let result = template_default(&ctx, &input, &values).unwrap();
        assert_eq!(result, "{{ 日本語 }}\nplain\n");
    }

    #[test]
    fn test_if_directive_with_multibyte_rhs() {
        let lines = [
            "{% if yadm.class == \"クラス\" %}",
            "matched",
            "{% else %}",
            "other",
            "{% endif %}",
        ];
        let input = write_temp("multibyte_if", &(lines.join("\n") + "\n"));
        let values = LocalValues {
            class: "クラス".to_string(),
            classes: vec!["クラス".to_string()],
            ..base_values()
        };
        let ctx = Context::new();
        let result = template_default(&ctx, &input, &values).unwrap();
        assert_eq!(result, "matched\n");
    }

    #[test]
    fn test_case_folding_is_ascii_only_like_awk_c_locale() {
        // awk's tolower() in the C locale folds only A-Z; "É" and "é" stay
        // distinct while ASCII letters still compare case-insensitively.
        let lines = [
            "{% if yadm.class == \"éclass\" %}",
            "folded",
            "{% else %}",
            "unfolded",
            "{% endif %}",
            "{% if yadm.class == \"Éclass\" %}",
            "exact",
            "{% endif %}",
        ];
        let input = write_temp("ascii_fold", &(lines.join("\n") + "\n"));
        let values = LocalValues {
            class: "Éclass".to_string(),
            classes: vec!["Éclass".to_string()],
            ..base_values()
        };
        let ctx = Context::new();
        let result = template_default(&ctx, &input, &values).unwrap();
        assert_eq!(result, "unfolded\nexact\n");
    }

    #[test]
    fn test_unknown_vars_expand_to_empty() {
        let input = write_temp(
            "unknown_vars",
            "yadm.no_such_var=\"{{ yadm.no_such_var }}\" and env.NO_SUCH_VAR_XYZ=\"{{ env.NO_SUCH_VAR_XYZ }}\"\n",
        );
        let values = base_values();
        let ctx = Context::new();
        let result = template_default(&ctx, &input, &values).unwrap();
        assert_eq!(
            result,
            "yadm.no_such_var=\"\" and env.NO_SUCH_VAR_XYZ=\"\"\n"
        );
    }

    #[test]
    fn test_template_default_full_feature_matrix() {
        const LOCAL_CLASS: &str = "default_Test+@-!^Class";
        const LOCAL_CLASS2: &str = "default_Test+@-|^2nd_Class withSpace";
        const LOCAL_ARCH: &str = "default_Test+@-!^Arch";
        const LOCAL_SYSTEM: &str = "default_Test+@-!^System";
        const LOCAL_HOST: &str = "default_Test+@-!^Host";
        const LOCAL_USER: &str = "default_Test+@-!^User";
        const LOCAL_DISTRO: &str = "default_Test+@-!^Distro";
        const LOCAL_DISTRO_FAMILY: &str = "default_Test+@-!^Family";
        const ENV_VAR: &str = "default_Test+@-!^Env";

        std::env::set_var("VAR", ENV_VAR);

        let values = LocalValues {
            class: LOCAL_CLASS.to_string(),
            classes: vec![LOCAL_CLASS2.to_string(), LOCAL_CLASS.to_string()],
            arch: LOCAL_ARCH.to_string(),
            system: LOCAL_SYSTEM.to_string(),
            host: LOCAL_HOST.to_string(),
            user: LOCAL_USER.to_string(),
            distro: LOCAL_DISTRO.to_string(),
            distro_family: LOCAL_DISTRO_FAMILY.to_string(),
        };

        let template = "\n\
start of template\n\
default class         = >{{yadm.class}}<\n\
default arch          = >{{yadm.arch}}<\n\
default os            = >{{yadm.os}}<\n\
default host          = >{{yadm.hostname}}<\n\
default user          = >{{yadm.user}}<\n\
default distro        = >{{yadm.distro}}<\n\
default distro_family = >{{yadm.distro_family}}<\n\
classes = >{{yadm.classes}}<\n\
{% if yadm.class == \"else1\" %}\n\
wrong else 1\n\
{% else %}\n\
Included section from else\n\
{% endif %}\n\
{% if yadm.class == \"wrongclass1\" %}\n\
wrong class 1\n\
{% endif %}\n\
{% if yadm.class != \"wronglcass\" %}\n\
Included section from !=\n\
{%     endif\t\t  %}\n\
{% if yadm.class == \"default_test+@-!^class\" %}\n\
Included section for class = {{yadm.class}} ({{yadm.class}} repeated)\n\
Multiple lines\n\
{% else %}\n\
Should not be included...\n\
{% endif %}\n\
{% if yadm.class == \"DEFAULT_TEST+@-|^2ND_CLASS WITHSPACE\" %}\n\
Included section for second class\n\
{% endif %}\n\
{% if yadm.class == \"wrongclass2\" %}\n\
wrong class 2\n\
{% endif %}\n\
{% if yadm.arch == \"wrongarch1\" %}\n\
wrong arch 1\n\
{% endif %}\n\
{% if yadm.arch == \"Default_test+@-!^Arch\" %}\n\
Included section for arch = {{yadm.arch}} ({{yadm.arch}} repeated)\n\
{% endif %}\n\
{% if yadm.arch == \"wrongarch2\" %}\n\
wrong arch 2\n\
{% endif %}\n\
{% if yadm.os == \"wrongos1\" %}\n\
wrong os 1\n\
{% endif %}\n\
{% if yadm.os == \"default_test+@-!^system\" %}\n\
Included section for os = {{yadm.os}} ({{yadm.os}} repeated)\n\
{% endif %}\n\
{% if yadm.os == \"wrongos2\" %}\n\
wrong os 2\n\
{% endif %}\n\
{% if yadm.hostname == \"wronghost1\" %}\n\
wrong host 1\n\
{% endif %}\n\
{% if yadm.hostname == \"DEFAULT_TEST+@-!^HOST\" %}\n\
Included section for host = {{yadm.hostname}} ({{yadm.hostname}} again)\n\
{% endif %}\n\
{% if yadm.hostname == \"wronghost2\" %}\n\
wrong host 2\n\
{% endif %}\n\
{% if yadm.user == \"wronguser1\" %}\n\
wrong user 1\n\
{% endif %}\n\
{% if yadm.user == \"Default_test+@-!^User\" %}\n\
Included section for user = {{yadm.user}} ({{yadm.user}} repeated)\n\
{% endif %}\n\
{% if yadm.user == \"wronguser2\" %}\n\
wrong user 2\n\
{% endif %}\n\
{% if yadm.distro == \"wrongdistro1\" %}\n\
wrong distro 1\n\
{% endif %}\n\
{% if yadm.distro == \"default_test+@-!^distro\" %}\n\
Included section for distro = {{yadm.distro}} ({{yadm.distro}} again)\n\
{% endif %}\n\
{% if yadm.distro == \"wrongdistro2\" %}\n\
wrong distro 2\n\
{% endif %}\n\
{% if yadm.distro_family == \"wrongfamily1\" %}\n\
wrong family 1\n\
{% endif %}\n\
{% if yadm.distro_family == \"DEFAULT_TEST+@-!^FAMILY\" %}\n\
Included section for distro_family = {{yadm.distro_family}} ({{yadm.distro_family}} again)\n\
{% endif %}\n\
{% if yadm.distro_family == \"wrongfamily2\" %}\n\
wrong family 2\n\
{% endif %}\n\
{% if env.VAR == \"Default_test+@-!^Env\" %}\n\
Included section for env.VAR = {{env.VAR}} ({{env.VAR}} again)\n\
{% endif %}\n\
{% if env.VAR == \"wrongenvvar\" %}\n\
wrong env.VAR\n\
{% endif %}\n\
yadm.no_such_var=\"{{ yadm.no_such_var }}\" and env.NO_SUCH_VAR=\"{{ env.NO_SUCH_VAR }}\"\n\
end of template\n\
";

        let expected = "\n\
start of template\n\
default class         = >default_Test+@-!^Class<\n\
default arch          = >default_Test+@-!^Arch<\n\
default os            = >default_Test+@-!^System<\n\
default host          = >default_Test+@-!^Host<\n\
default user          = >default_Test+@-!^User<\n\
default distro        = >default_Test+@-!^Distro<\n\
default distro_family = >default_Test+@-!^Family<\n\
classes = >default_Test+@-|^2nd_Class withSpace\n\
default_Test+@-!^Class<\n\
Included section from else\n\
Included section from !=\n\
Included section for class = default_Test+@-!^Class (default_Test+@-!^Class repeated)\n\
Multiple lines\n\
Included section for second class\n\
Included section for arch = default_Test+@-!^Arch (default_Test+@-!^Arch repeated)\n\
Included section for os = default_Test+@-!^System (default_Test+@-!^System repeated)\n\
Included section for host = default_Test+@-!^Host (default_Test+@-!^Host again)\n\
Included section for user = default_Test+@-!^User (default_Test+@-!^User repeated)\n\
Included section for distro = default_Test+@-!^Distro (default_Test+@-!^Distro again)\n\
Included section for distro_family = default_Test+@-!^Family (default_Test+@-!^Family again)\n\
Included section for env.VAR = default_Test+@-!^Env (default_Test+@-!^Env again)\n\
yadm.no_such_var=\"\" and env.NO_SUCH_VAR=\"\"\n\
end of template\n\
";

        let input = write_temp("full_matrix", template);
        let ctx = Context::new();
        let result = template_default(&ctx, &input, &values).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_include_semantics() {
        const LOCAL_CLASS: &str = "default_Test+@-!^Class";
        const LOCAL_SYSTEM: &str = "default_Test+@-!^System";

        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir()
            .join(format!(
                "radm-template-default-test-{}-{}-include",
                std::process::id(),
                n
            ))
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(&dir).unwrap();

        write_temp_in_dir(&dir, "empty", "");
        write_temp_in_dir(&dir, "basic", "basic\n");
        let variables_filename = format!("variables.{LOCAL_SYSTEM}");
        let variables_path = write_temp_in_dir(
            &dir,
            &variables_filename,
            "included <{{ yadm.class }}> file ({{yadm.filename}})\n\nempty line above\n",
        );
        write_temp_in_dir(&dir, "dir/nested", "no newline at the end");

        // built line-by-line to preserve the fixture's leading spaces
        let input_lines = [
            "The first line",
            "{% include empty %}",
            "An empty file removes the line above",
            "{%include ./basic%}",
            "{% include \"variables.{{ yadm.os }}\"  %}",
            "  {% include dir/nested %}",
            "Include basic again:",
            "{% include basic %}",
        ];
        let input_content = input_lines.join("\n") + "\n";
        let input = write_temp_in_dir(&dir, "input", &input_content);

        let mut values = base_values();
        values.class = LOCAL_CLASS.to_string();
        values.system = LOCAL_SYSTEM.to_string();

        let ctx = Context::new();
        let result = template_default(&ctx, &input, &values).unwrap();

        let expected = format!(
            "The first line\n\
An empty file removes the line above\n\
basic\n\
included <{LOCAL_CLASS}> file ({variables_path})\n\
\n\
empty line above\n\
no newline at the end\n\
Include basic again:\n\
basic\n"
        );

        assert_eq!(result, expected);
    }

    #[test]
    fn test_nested_ifs() {
        // Built from an array of line literals (rather than a single
        // backslash-continued string literal) because Rust's `\`
        // line-continuation inside string literals strips ALL leading
        // whitespace from the following source line — which would silently
        // eat the leading spaces these fixture lines depend on.
        let lines = [
            "{% if yadm.user == \"me\" %}",
            "    print1",
            "  {% if yadm.user == \"me\" %}",
            "    print2",
            "  {% else %}",
            "    no print1",
            "  {% endif %}",
            "{% else %}",
            "  {% if yadm.user == \"me\" %}",
            "    no print2",
            "  {% else %}",
            "    no print3",
            "  {% endif %}",
            "{% endif %}",
            "{% if yadm.user != \"me\" %}",
            "    no print4",
            "  {% if yadm.user == \"me\" %}",
            "    no print5",
            "  {% else %}",
            "    no print6",
            "  {% endif %}",
            "{% else %}",
            "  {% if yadm.user == \"me\" %}",
            "    print3",
            "  {% else %}",
            "    no print7",
            "  {% endif %}",
            "{% endif %}",
        ];
        let template = lines.join("\n") + "\n";

        let mut values = base_values();
        values.user = "me".to_string();

        let input = write_temp("nested_ifs", &template);
        let ctx = Context::new();
        let result = template_default(&ctx, &input, &values).unwrap();
        assert_eq!(result, "    print1\n    print2\n    print3\n");
    }

    #[test]
    fn test_else_without_matching_if_errors() {
        let input = write_temp("else_no_if", "{% else %}\n");
        let ctx = Context::new();
        let values = base_values();
        assert!(template_default(&ctx, &input, &values).is_err());
    }

    #[test]
    fn test_double_else_errors() {
        // The if-branch must be TAKEN (condition true) for the first `{%
        // else %}` to mark skip[level] = -1 (the "already elsed" sentinel);
        // a second `{% else %}` then hits `skip[level] < 0` and errors.
        let input = write_temp(
            "double_else",
            "{% if yadm.user == \"x\" %}\na\n{% else %}\nb\n{% else %}\nc\n{% endif %}\n",
        );
        let ctx = Context::new();
        let mut values = base_values();
        values.user = "x".to_string();
        assert!(template_default(&ctx, &input, &values).is_err());
    }

    #[test]
    fn test_endif_without_matching_if_errors() {
        let input = write_temp("endif_no_if", "{% endif %}\n");
        let ctx = Context::new();
        let values = base_values();
        assert!(template_default(&ctx, &input, &values).is_err());
    }

    #[test]
    fn test_unterminated_if_errors() {
        let input = write_temp(
            "unterminated_if",
            "line1\n{% if yadm.user == \"x\" %}\nline2\n",
        );
        let ctx = Context::new();
        let values = base_values();
        assert!(template_default(&ctx, &input, &values).is_err());
    }

    #[test]
    fn test_could_not_read_input_file() {
        let ctx = Context::new();
        let values = base_values();
        let missing = std::env::temp_dir()
            .join("radm-template-default-does-not-exist-at-all")
            .to_string_lossy()
            .into_owned();
        assert!(template_default(&ctx, &missing, &values).is_err());
    }

    #[test]
    fn test_could_not_read_include_file() {
        let dir = std::env::temp_dir()
            .join(format!(
                "radm-template-default-test-{}-missing-include",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(&dir).unwrap();
        let input = write_temp_in_dir(&dir, "input", "{% include does_not_exist %}\n");
        let ctx = Context::new();
        let values = base_values();
        assert!(template_default(&ctx, &input, &values).is_err());
    }

    #[test]
    fn test_include_empty_file_leaves_no_blank_line() {
        let dir = std::env::temp_dir()
            .join(format!(
                "radm-template-default-test-{}-include-empty",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_in_dir(&dir, "empty", "");
        let input = write_temp_in_dir(&dir, "input", "before\n{% include empty %}\nafter\n");
        let ctx = Context::new();
        let values = base_values();
        let result = template_default(&ctx, &input, &values).unwrap();
        assert_eq!(result, "before\nafter\n");
    }

    #[test]
    fn test_blank_rhs_matches_empty_variable() {
        let input = write_temp(
            "blank_rhs",
            "{% if yadm.arch == \"\" %}\nempty arch\n{% else %}\nnon-empty arch\n{% endif %}\n",
        );
        let ctx = Context::new();
        let values = base_values(); // arch == ""
        let result = template_default(&ctx, &input, &values).unwrap();
        assert_eq!(result, "empty arch\n");

        let mut values2 = base_values();
        values2.arch = "x86_64".to_string();
        let result2 = template_default(&ctx, &input, &values2).unwrap();
        assert_eq!(result2, "non-empty arch\n");
    }

    #[test]
    fn test_class_membership_across_multiple_classes() {
        let input = write_temp(
            "class_membership",
            "{% if yadm.class == \"secondary\" %}\nmatched\n{% else %}\nno match\n{% endif %}\n",
        );
        let mut values = base_values();
        values.class = "primary".to_string();
        values.classes = vec!["primary".to_string(), "secondary".to_string()];
        let ctx = Context::new();
        let result = template_default(&ctx, &input, &values).unwrap();
        assert_eq!(result, "matched\n");
    }

    #[test]
    fn test_skipped_block_not_variable_expanded() {
        // A line inside a false branch must not be expanded or error even if
        // it would reference an undefined pattern; here we simply assert it's
        // dropped verbatim (not printed), proving no evaluation occurs.
        let input = write_temp(
            "skipped_block",
            "{% if yadm.user == \"nope\" %}\n{{ env.SOME_RANDOM_VAR_QQ }}\n{% endif %}\nafter\n",
        );
        let ctx = Context::new();
        let values = base_values();
        let result = template_default(&ctx, &input, &values).unwrap();
        assert_eq!(result, "after\n");
    }
}
