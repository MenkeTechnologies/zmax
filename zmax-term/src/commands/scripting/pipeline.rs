//! In-process polyglot pipeline: one selection through N embedded languages
//! with no `fork`, no `exec`, and no pipe file descriptors.
//!
//! `:pipe` (`commands.rs`) spawns a shell per invocation, so a three-stage
//! `awk | ruby | python` costs three `fork`+`execve` pairs, six pipe ends, and
//! three `waitpid`s — plus a full encode/decode of the text at every boundary.
//! Every one of those interpreters is already linked into this binary, so the
//! same chain can run as three function calls on the editor thread instead.
//!
//! ## Stage separator
//!
//! Stages are separated by a whitespace-delimited `|>`. A bare `|` is live
//! syntax in most of the twelve (awk's `print | "cmd"`, ruby/js block params,
//! zsh pipelines), so it cannot be the separator; ` |> ` is not. A literal
//! `|>` inside a stage is written `\|>`.
//!
//! ```text
//! :xpipe awk '{print $2}' |> ruby 'stdin.split("\n").map(&:upcase).join("\n")'
//! ```
//!
//! ## How a stage receives its input
//!
//! Every stage binds the incoming text to a variable named `stdin`, spelled in
//! that language's own syntax (`$stdin` where variables carry a sigil). awk and
//! arb are line-oriented filters that take input natively, so they see it as
//! their record stream rather than as a variable.
//!
//! | language | binding |
//! |---|---|
//! | awk, arb | the record stream (no variable) |
//! | ruby, python, node, r, tcl, elisp | `stdin` |
//! | php, zsh, stryke | `$stdin` |
//! | vim | `g:stdin` |
//!
//! A stage's *output* is whatever that language's `:`-command would have shown:
//! what the program printed, or its last value when it printed nothing.
//!
//! ## Phase 1 binding mechanism, and what replaces it
//!
//! `rubylang::eval_str`, `pythonrs::eval_str` and `nodejs::eval_str` each call
//! `host::reset_host()` before running (`rubylang/src/lib.rs:138`,
//! `pythonrs/src/lib.rs:152`, `node-js/src/lib.rs:103`), so a global installed
//! through their `set_global` host API before the call is wiped by it. Until
//! those crates expose an entry point that seeds globals *after* the reset, the
//! input is bound by prepending a literal-string assignment to the program —
//! correct, but it escapes and copies the text once per stage. zsh is the
//! exception already: its executor persists, so its binding is a real
//! `set_scalar` with no escaping. Lifting the rest to the same mechanism is
//! upstream work in the vendored crates, and it is what removes the per-stage
//! copy and the fd-capture round trip.

/// The languages a stage can name, in the order the error message lists them.
/// The canonical name of each is the `:`-command that evaluates that language.
pub(super) const LANGUAGES: &[&str] = &[
    "awk", "arb", "ruby", "python", "node", "php", "rlang", "tcl", "zsh", "stryke", "elisp", "vim",
];

/// One stage of a pipeline: a language and the program text handed to it.
#[derive(Debug)]
pub(super) struct Stage {
    lang: Lang,
    code: String,
}

impl Stage {
    /// The stage's language name, for error messages.
    pub(super) fn lang_name(&self) -> &'static str {
        self.lang.name()
    }

    /// Run this stage over `input` and return what it produced.
    pub(super) fn run(&self, input: &str) -> Result<String, String> {
        match self.lang {
            // Line-oriented filters: input is the record stream, natively.
            Lang::Awk => super::awk::run(&self.code, input),
            Lang::Arb => super::arb::run(&self.code, input),
            // A persistent host that can be assigned directly — no escaping.
            Lang::Zsh => super::zsh::filter(&self.code, input),
            // PHP's source is a template by default, so its binding has to be
            // spliced inside a `<?php` tag rather than prepended as text.
            Lang::Php => super::php::filter(&self.code, input),
            // The rest take the binding as a prepended literal assignment.
            Lang::Ruby => {
                super::ruby::eval(&format!("stdin = \"{}\"\n{}", dq(input, &['#']), self.code))
            }
            Lang::Python => {
                super::python::eval(&format!("stdin = \"{}\"\n{}", dq(input, &[]), self.code))
            }
            Lang::Node => super::node::eval(&format!(
                "var stdin = \"{}\";\n{}",
                dq(input, &[]),
                self.code
            )),
            Lang::R => super::r::eval(&format!("stdin <- \"{}\"\n{}", dq(input, &[]), self.code)),
            Lang::Tcl => super::tcl::eval(&format!(
                "set stdin \"{}\"\n{}",
                dq(input, &['$', '[', ']']),
                self.code
            )),
            Lang::Viml => super::viml::eval(&format!(
                "let g:stdin = \"{}\"\n{}",
                dq(input, &[]),
                self.code
            )),
            Lang::Elisp => super::elisp::filter(&format!(
                "(setq stdin \"{}\")\n{}",
                dq(input, &[]),
                self.code
            )),
            // stryke is Perl-shaped: a single-quoted literal needs only `\` and
            // `'` escaped, and takes raw newlines as themselves.
            Lang::Stryke => {
                super::stryke::eval(&format!("my $stdin = '{}';\n{}", sq(input), self.code))
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Lang {
    Awk,
    Arb,
    Ruby,
    Python,
    Node,
    Php,
    R,
    Tcl,
    Zsh,
    Stryke,
    Elisp,
    Viml,
}

impl Lang {
    /// Resolve a stage's language token. The aliases are the ones the
    /// equivalent `:`-commands already answer to (`:rb`, `:js`, `:viml`, …).
    fn from_name(name: &str) -> Option<Lang> {
        Some(match name {
            "awk" | "awk-filter" => Lang::Awk,
            "arb" => Lang::Arb,
            "ruby" | "rb" => Lang::Ruby,
            "python" | "py" => Lang::Python,
            "node" | "js" | "javascript" => Lang::Node,
            "php" => Lang::Php,
            "rlang" | "r" => Lang::R,
            "tcl" => Lang::Tcl,
            "zsh" | "zshell" | "sh" => Lang::Zsh,
            "stryke" => Lang::Stryke,
            "elisp" | "el" => Lang::Elisp,
            "vim" | "viml" | "vimscript" => Lang::Viml,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Lang::Awk => "awk",
            Lang::Arb => "arb",
            Lang::Ruby => "ruby",
            Lang::Python => "python",
            Lang::Node => "node",
            Lang::Php => "php",
            Lang::R => "rlang",
            Lang::Tcl => "tcl",
            Lang::Zsh => "zsh",
            Lang::Stryke => "stryke",
            Lang::Elisp => "elisp",
            Lang::Viml => "vim",
        }
    }
}

/// Parse a pipeline spec into its stages. Fails on an unknown language, a stage
/// with no program, and an empty spec — each with the stage number, so a long
/// chain reports *which* stage is wrong.
pub(super) fn parse(spec: &str) -> Result<Vec<Stage>, String> {
    let mut stages = Vec::new();
    for (i, raw) in split_stages(spec).into_iter().enumerate() {
        let text = raw.trim();
        if text.is_empty() {
            return Err(format!("stage {}: empty stage", i + 1));
        }
        let (name, code) = match text.split_once(char::is_whitespace) {
            Some((name, code)) => (name, code.trim()),
            None => (text, ""),
        };
        let lang = Lang::from_name(name).ok_or_else(|| {
            format!(
                "stage {}: unknown language '{}' (one of: {})",
                i + 1,
                name,
                LANGUAGES.join(", ")
            )
        })?;
        if code.is_empty() {
            return Err(format!("stage {}: {} has no program", i + 1, lang.name()));
        }
        stages.push(Stage {
            lang,
            code: unquote(code).to_string(),
        });
    }
    if stages.is_empty() {
        return Err("empty pipeline".to_string());
    }
    Ok(stages)
}

/// Split on every unescaped, whitespace-delimited `|>`. The delimiter rule is
/// what lets a stage contain `|` freely: only a `|>` that stands alone as a
/// token separates stages, and `\|>` is a literal one.
fn split_stages(spec: &str) -> Vec<String> {
    let bytes = spec.as_bytes();
    let mut stages = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    while i < bytes.len() {
        // An escaped `\|>` contributes a literal `|>` and consumes the escape.
        if bytes[i] == b'\\' && bytes[i + 1..].starts_with(b"|>") {
            current.push_str("|>");
            i += 3;
            continue;
        }
        if bytes[i] == b'|' && bytes[i + 1..].starts_with(b">") {
            let before_ok = current.chars().next_back().is_none_or(char::is_whitespace);
            let after_ok = spec[i + 2..].chars().next().is_none_or(char::is_whitespace);
            if before_ok && after_ok {
                stages.push(std::mem::take(&mut current));
                i += 2;
                continue;
            }
        }
        // `i` always sits on a char boundary: the two branches above advance by
        // whole ASCII tokens, and this one by the full char.
        let ch = spec[i..].chars().next().expect("char boundary");
        current.push(ch);
        i += ch.len_utf8();
    }
    stages.push(current);
    stages
}

/// Strip one matching pair of *single* quotes, so the shell habit of
/// `awk '{print $2}'` survives. Double quotes are left alone — they are string
/// syntax in every language a stage can name.
fn unquote(code: &str) -> &str {
    match code.strip_prefix('\'').and_then(|c| c.strip_suffix('\'')) {
        Some(inner) => inner,
        None => code,
    }
}

/// Escape `input` for a double-quoted string literal: `\` and `"` always, the
/// three control characters no target language accepts raw inside quotes, and
/// any `extra` interpolation characters the language would otherwise act on
/// (ruby's `#`, php's `$`, tcl's `$`/`[`/`]`).
pub(super) fn dq(input: &str, extra: &[char]) -> String {
    let mut out = String::with_capacity(input.len() + input.len() / 8);
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if extra.contains(&c) => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

/// Escape `input` for a Perl-style single-quoted literal, where `\` and `'` are
/// the only escapes and a newline stands for itself.
fn sq(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + input.len() / 8);
    for ch in input.chars() {
        if ch == '\\' || ch == '\'' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The separator only splits when it stands alone, so `|` inside a program
    /// (awk's redirect, a ruby block param) never ends a stage.
    #[test]
    fn splits_only_on_standalone_separator() {
        let stages = parse("awk '{print $2}' |> ruby 'stdin.split(\"\\n\")'").unwrap();
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].lang_name(), "awk");
        assert_eq!(stages[0].code, "{print $2}");
        assert_eq!(stages[1].lang_name(), "ruby");

        let piped = parse("awk '{print | \"sort\"}'").unwrap();
        assert_eq!(piped.len(), 1);
        assert_eq!(piped[0].code, "{print | \"sort\"}");

        // `|>` glued to a token is an operator, not a separator.
        let glued = parse("ruby 'a|>b'").unwrap();
        assert_eq!(glued.len(), 1);
        assert_eq!(glued[0].code, "a|>b");
    }

    /// `\|>` is a literal separator inside one stage.
    #[test]
    fn escaped_separator_stays_in_the_stage() {
        let stages = parse("ruby 'x \\|> y'").unwrap();
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].code, "x |> y");
    }

    /// Every failure names the stage number, so a long chain says which one.
    #[test]
    fn errors_identify_the_stage() {
        assert_eq!(
            parse("awk '{print}' |> perl 'x'").unwrap_err(),
            format!(
                "stage 2: unknown language 'perl' (one of: {})",
                LANGUAGES.join(", ")
            )
        );
        assert_eq!(
            parse("awk '{print}' |> ruby").unwrap_err(),
            "stage 2: ruby has no program"
        );
        assert_eq!(
            parse("awk '{print}' |>  |> ruby 'x'").unwrap_err(),
            "stage 2: empty stage"
        );
        assert_eq!(parse("   ").unwrap_err(), "stage 1: empty stage");
    }

    /// Aliases resolve to the same language as the canonical name.
    #[test]
    fn aliases_resolve() {
        for (alias, canonical) in [
            ("rb", "ruby"),
            ("py", "python"),
            ("js", "node"),
            ("r", "rlang"),
            ("viml", "vim"),
            ("el", "elisp"),
            ("sh", "zsh"),
        ] {
            let stages = parse(&format!("{alias} 'x'")).unwrap();
            assert_eq!(stages[0].lang_name(), canonical);
        }
    }

    /// The escapers neutralise exactly what would otherwise change the meaning
    /// of the literal the input is spliced into.
    #[test]
    fn escapers_neutralise_interpolation() {
        assert_eq!(dq("a\"b\\c\nd\te", &[]), "a\\\"b\\\\c\\nd\\te");
        // ruby `#{}`, php `$x`, tcl `$x`/`[cmd]`.
        assert_eq!(dq("#{x}", &['#']), "\\#{x}");
        assert_eq!(dq("$x", &['$']), "\\$x");
        assert_eq!(dq("[cmd] $x", &['$', '[', ']']), "\\[cmd\\] \\$x");
        // Perl single quotes: `\` and `'` only, newline stands for itself.
        assert_eq!(sq("a'b\\c\nd"), "a\\'b\\\\c\nd");
    }

    /// A stage's outer single quotes are shell habit, not program text; double
    /// quotes are program text and must survive.
    #[test]
    fn only_single_quotes_are_stripped() {
        assert_eq!(unquote("'{print}'"), "{print}");
        assert_eq!(unquote("\"abc\""), "\"abc\"");
        assert_eq!(unquote("plain"), "plain");
    }

    /// Run a whole spec, as `run_pipeline` does but without an editor context.
    #[cfg(unix)]
    fn run_all(spec: &str, input: &str) -> Result<String, String> {
        let stages = parse(spec)?;
        let mut data = input.to_string();
        for stage in &stages {
            data = stage.run(&data)?;
        }
        Ok(data)
    }

    /// Three different languages, one process: awk picks a column, php upcases
    /// what awk produced, and elisp reverses what php produced. Each stage sees
    /// the previous stage's output, so a wrong hand-off cannot pass.
    ///
    /// These three are the engines that capture their own output, so their
    /// results are exact under libtest (ruby/python/node redirect the process
    /// fds, which libtest's own capture makes unreliable — see the notes on the
    /// per-engine tests in `super`).
    #[cfg(unix)]
    #[test]
    fn three_language_chain_runs_in_process() {
        let out = run_all(
            "awk '{print $2}' |> php 'echo strtoupper($stdin);' |> elisp '(reverse stdin)'",
            "one two\nthree four\n",
        )
        .expect("chain");
        // awk → "two\nfour", php → "TWO\nFOUR", elisp reverses those characters.
        assert_eq!(out, "RUOF\nOWT");
    }

    /// Text that would otherwise be read as syntax survives the hand-off: quotes,
    /// backslashes, newlines, php's `$var`, ruby's `#{}` and tcl's `[cmd]` all
    /// come back byte for byte.
    #[cfg(unix)]
    #[test]
    fn hostile_input_survives_the_binding() {
        let hostile = "a \"quoted\" \\ backslash\n#{ruby} $php [tcl]\ttab";
        assert_eq!(run_all("php 'echo $stdin;'", hostile).unwrap(), hostile);
        assert_eq!(
            run_all("tcl 'set stdin'", hostile).unwrap(),
            hostile,
            "tcl re-reads the variable it was handed"
        );
    }

    /// A failing stage names itself: the pipeline reports which stage broke
    /// rather than surfacing a bare interpreter error.
    #[cfg(unix)]
    #[test]
    fn a_failing_stage_is_an_error() {
        assert!(run_all("php 'echo $stdin;' |> tcl 'no-such-command'", "x").is_err());
    }

    /// Measurement harness, not an assertion: times the same two-stage filter
    /// in-process against the `:pipe` equivalent through `/bin/sh`. Ignored by
    /// default — it needs `awk` and `php` on `PATH` and a wall-clock number is
    /// not something to gate CI on.
    ///
    /// ```sh
    /// cargo test -p zmax-term --lib -- --ignored --nocapture xpipe_against_a_shell_pipe
    /// ```
    #[cfg(unix)]
    #[test]
    #[ignore = "benchmark: needs awk and php on PATH"]
    fn xpipe_against_a_shell_pipe() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        use std::time::Instant;

        const RUNS: u32 = 20;
        let input: String = (0..10_000).map(|i| format!("row{i} value{i}\n")).collect();
        let spec = "awk '{print $2}' |> php 'echo strtoupper($stdin);'";
        let shell_cmd = "awk '{print $2}' | php -r 'echo strtoupper(stream_get_contents(STDIN));'";

        // Warm both paths so neither pays a one-off cost inside the timed loop.
        let expected = run_all(spec, &input).expect("in-process warmup");

        let t0 = Instant::now();
        for _ in 0..RUNS {
            run_all(spec, &input).expect("in-process run");
        }
        let in_process = t0.elapsed();

        let run_shell = || {
            let mut child = Command::new("/bin/sh")
                .arg("-c")
                .arg(shell_cmd)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("spawn sh");
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(input.as_bytes())
                .expect("write");
            let out = child.wait_with_output().expect("wait");
            String::from_utf8_lossy(&out.stdout).trim_end().to_string()
        };
        assert_eq!(run_shell(), expected, "both paths must produce one result");

        let t1 = Instant::now();
        for _ in 0..RUNS {
            run_shell();
        }
        let subprocess = t1.elapsed();

        println!(
            "10k lines x {RUNS}: in-process {in_process:?}, shell {subprocess:?} ({:.1}x)",
            subprocess.as_secs_f64() / in_process.as_secs_f64()
        );
    }
}
