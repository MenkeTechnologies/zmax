//! The fzf.vim picker, run **in this process** by the embedded arb.
//!
//! zmax used to hand the terminal to the external `fzf` binary for `:Files`,
//! `:Rg`, `:GFiles`, `:Buffers` and the rest of the fzf.vim surface. It does not
//! any more: `arblang` is already linked into this binary (the `scripting`
//! feature, the same dependency behind `:arb` and `:xpipe`), and its `--fzf`
//! mode is a drop-in for fzf — it ingests `$FZF_DEFAULT_OPTS_FILE` and
//! `$FZF_DEFAULT_OPTS`, and paints fzf 0.74's own palette when nothing themed it
//! (`arb::fzf`). So the picker is a function call here, not a fork.
//!
//! What arb's own `main` does for `--fzf`, this module does with its public API:
//!
//! | arb `main.rs` | here |
//! |---|---|
//! | splice `fzf::env_args()` into argv | [`compose_argv`] |
//! | `fzf::Look::parse(&argv)` | same, on the composed argv |
//! | clap parses `--prompt`/`--header`/`--query`/`-e`/`+s`/`-m` | [`Flags::parse`] |
//! | `spec::build(parse("select .sel\nsource .sel { in }"))` | [`select_spec`] |
//! | stdin reader thread fills `StreamState` | [`spawn_source`] |
//! | `spawn_item_preview` thread on `Controls.current` | [`spawn_preview`] |
//! | `tui::run(...)`, then print `Controls.result` | [`pick`], which returns it |
//!
//! The one thing that still spawns is the *source* command (`git ls-files`,
//! `rg …`), which has to be a process because it is one. fzf forked for the
//! producer too; the difference is that the picker itself no longer does.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arb::stream::StreamState;

/// Everything the picker needs. Composed by the caller so this module never
/// reaches into editor state.
pub struct Request<'a> {
    /// Lines to pick from. Empty means the source comes from `command`, the
    /// `$FZF_*` environment, or `fallback`.
    pub candidates: &'a [String],
    /// fzf.vim's `source`: a shell command whose stdout is the candidate list.
    pub command: Option<String>,
    /// Per-request fzf flags (`+m`, `--no-sort`, …).
    pub options: &'a [String],
    /// The composed `FZF_DEFAULT_OPTS` string — the user's own environment plus
    /// zmax's `fzf.options`/`fzf.preview` config. Parsed with fzf's quoting
    /// rules, exactly as arb parses the env var.
    pub default_opts: &'a str,
}

/// The flags arb's clap layer reads that `fzf::Look` does not carry. Parsed off
/// the same composed argv so `$FZF_DEFAULT_OPTS='--prompt="> "'` reaches the
/// prompt line just as it does under the real binary.
#[derive(Default)]
struct Flags {
    prompt: String,
    header: String,
    query: String,
    preview: Option<String>,
    height: Option<String>,
    exact: bool,
    no_sort: bool,
    multi: bool,
}

impl Flags {
    /// fzf accepts `--flag value` and `--flag=value` for the same flag, and its
    /// `+x` forms turn a default *off* — `+m` (no multi-select) and `+s` (no
    /// sort, keep input order). Short `-e`/`-q`/`-m` are the fzf spellings.
    fn parse(argv: &[String]) -> Self {
        /// The value of `--flag=value` (already split off) or of the next word.
        /// Advances past the consumed word so the caller's scan does not re-read
        /// a value as if it were a flag.
        fn value(argv: &[String], i: &mut usize, inline: Option<&str>) -> String {
            match inline {
                Some(v) => v.to_string(),
                None => match argv.get(*i + 1) {
                    Some(v) => {
                        *i += 1;
                        v.clone()
                    }
                    None => String::new(),
                },
            }
        }

        let mut f = Flags::default();
        let mut i = 0;
        while i < argv.len() {
            let (name, inline) = match argv[i].split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (argv[i].clone(), None),
            };
            let inline = inline.as_deref();
            match name.as_str() {
                "--prompt" => f.prompt = value(argv, &mut i, inline),
                "--header" => f.header = value(argv, &mut i, inline),
                "--query" | "-q" => f.query = value(argv, &mut i, inline),
                "--height" => f.height = Some(value(argv, &mut i, inline)),
                "--preview" => f.preview = Some(value(argv, &mut i, inline)),
                "--exact" | "-e" => f.exact = true,
                "--no-sort" | "+s" => f.no_sort = true,
                "--multi" | "-m" => f.multi = true,
                "+m" => f.multi = false,
                _ => {}
            }
            i += 1;
        }
        f
    }
}

/// argv as arb's `main` would see it: the program name, `--fzf`, the user's fzf
/// configuration, then the per-request flags. Later wins, which is fzf's own
/// precedence — an explicit per-command flag overrides `$FZF_DEFAULT_OPTS`.
fn compose_argv(req: &Request<'_>) -> Vec<String> {
    let mut argv = vec!["arb".to_string(), "--fzf".to_string()];
    argv.extend(arb::fzf::shell_split(req.default_opts));
    argv.extend(req.options.iter().cloned());
    argv
}

/// The one-widget spec that IS fzf mode: arb's `--fzf` synthesizes exactly this
/// (`default_spec_src` in its `main`), so building it here gets the same picker.
fn select_spec() -> Option<arb::spec::Spec> {
    let cmds = arb::parser::parse("select .sel\nsource .sel { in }").ok()?;
    let mut spec = arb::spec::build(&cmds).ok()?;
    // An unthemed picker renders in fzf's palette rather than an arb theme —
    // the point of the drop-in. Ctrl-T still switches to an arb theme live.
    spec.theme = None;
    spec
        .widgets
        .iter()
        .any(|w| w.kind == arb::spec::WidgetKind::Select)
        .then_some(spec)
}

/// The user's shell, for the source and preview commands. fzf runs both through
/// `$SHELL -c`, so a preview that calls one of their shell functions works.
fn shell() -> Vec<String> {
    let sh = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    vec![sh, "-c".to_string()]
}

/// Stream `command`'s stdout into the picker's candidate list as it arrives, so
/// a slow producer (`find /`) fills the list live instead of blocking on it.
/// Returns the child so the caller can kill it when the picker exits.
fn spawn_source(command: &str, state: Arc<Mutex<StreamState>>) -> Option<Arc<Mutex<Child>>> {
    let sh = shell();
    let child = Command::new(&sh[0])
        .args(&sh[1..])
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let child = Arc::new(Mutex::new(child));
    let reader = child.lock().ok()?.stdout.take()?;
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            match state.lock() {
                Ok(mut s) => s.push(line),
                Err(_) => break,
            }
        }
    });
    Some(child)
}

/// fzf `--preview`: re-run `template` (with `{}` = the line under the cursor)
/// whenever the cursor moves, into the pane arb renders below the list. This is
/// arb's own `spawn_item_preview`, which lives in its binary rather than its
/// library, so it is reproduced against the same `Controls` contract.
fn spawn_preview(
    template: String,
    controls: Arc<Mutex<arb::tui::Controls>>,
    pane: Arc<Mutex<StreamState>>,
) {
    let sh = shell();
    std::thread::spawn(move || {
        // Not the empty string: the first real line must always be a change, and
        // an empty candidate list should not fire a preview of nothing.
        let mut last = String::from("\u{0}");
        loop {
            std::thread::sleep(Duration::from_millis(120));
            let (cur, quit) = match controls.lock() {
                Ok(c) => (c.current.clone(), c.quit),
                Err(_) => break,
            };
            if quit {
                break;
            }
            if cur == last || cur.is_empty() {
                continue;
            }
            last = cur.clone();
            let cmd = template.replace("{}", &shell_quote(&cur));
            let out = Command::new(&sh[0]).args(&sh[1..]).arg(&cmd).output();
            let lines: Vec<String> = match out {
                Ok(o) => String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .map(String::from)
                    .collect(),
                Err(e) => vec![format!("zmax: preview: {e}")],
            };
            if let Ok(mut p) = pane.lock() {
                *p = StreamState::new();
                for l in lines {
                    p.push(l);
                }
            }
        }
    });
}

/// Single-quote for `sh -c`, the way fzf quotes the `{}` substitution: wrap in
/// `'…'` and close/escape/reopen around any embedded quote.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Run the picker and return the chosen line, or `None` if the user aborted.
///
/// The caller must have released the terminal first (arb opens `/dev/tty` and
/// takes raw mode) and must re-claim it afterwards — the same handoff the
/// external binary needed, minus the process.
///
/// `fallback` supplies candidates when nothing else does: no explicit list, no
/// per-request `source`, and no `$FZF_DEFAULT_COMMAND`/`$FZF_CTRL_T_COMMAND` in
/// the environment. The real fzf falls back to its own `find` walk; zmax hands
/// over the walk its own file picker uses, so `:Files` and `SPC f f` list the
/// same files and honour the same `file-picker` config.
pub fn pick(req: Request<'_>, fallback: impl FnOnce() -> Vec<String>) -> Option<String> {
    let argv = compose_argv(&req);
    let look = arb::fzf::Look::parse(&argv);
    let flags = Flags::parse(&argv);
    let spec = select_spec()?;

    // `usize::MAX`: the retention cap that keeps every line. Dropping the oldest
    // would lose marks and shift the cursor while the producer is still running.
    let state = Arc::new(Mutex::new(StreamState::with_cap(usize::MAX)));
    let mut source_child = None;
    if !req.candidates.is_empty() {
        if let Ok(mut s) = state.lock() {
            for line in req.candidates {
                s.push(line.clone());
            }
        }
    } else {
        // fzf.vim's `source` wins; then the environment fzf itself would have
        // read (`FZF_DEFAULT_COMMAND`, then the shell's CTRL-T command).
        let cmd = req
            .command
            .clone()
            .filter(|c| !c.is_empty())
            .or_else(|| std::env::var("FZF_DEFAULT_COMMAND").ok().filter(|c| !c.is_empty()))
            .or_else(|| std::env::var("FZF_CTRL_T_COMMAND").ok().filter(|c| !c.is_empty()));
        match cmd {
            Some(cmd) => source_child = spawn_source(&cmd, Arc::clone(&state)),
            None => {
                if let Ok(mut s) = state.lock() {
                    for line in fallback() {
                        s.push(line);
                    }
                }
            }
        }
    }

    let controls = Arc::new(Mutex::new(arb::tui::Controls::default()));
    {
        let mut c = controls.lock().ok()?;
        c.fzf = true;
        c.look = look;
        c.prompt = flags.prompt.clone();
        c.header = flags.header.clone();
        c.filter = flags.query.clone();
        c.exact = flags.exact;
        c.no_sort = flags.no_sort;
        // fzf only marks with `-m`/`--multi`, and `arb --fzf` matches that.
        c.multi = flags.multi;
    }

    // The preview pane is arb's `down` stream: a second StreamState it renders
    // below the list, fed by the preview thread.
    let down = flags.preview.as_ref().map(|template| {
        let pane = Arc::new(Mutex::new(StreamState::new()));
        spawn_preview(template.clone(), Arc::clone(&controls), Arc::clone(&pane));
        (pane, "preview".to_string())
    });

    let ran = arb::tui::run(
        &spec,
        state,
        Arc::clone(&controls),
        down,
        None,
        true,
        flags.height.clone(),
    );

    // The producer outlives the picker otherwise: `find /` would keep walking
    // the disk after the user has already chosen.
    if let Some(child) = source_child.take() {
        if let Ok(mut c) = child.lock() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
    if ran.is_err() {
        return None;
    }
    let c = controls.lock().ok()?;
    // Esc / Ctrl-C leaves `submit` false: no pick, and the caller must not act.
    if !c.submit {
        return None;
    }
    c.result.first().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_read_both_fzf_spellings() {
        let argv: Vec<String> = ["arb", "--fzf", "--prompt=> ", "--query", "src", "-e", "+s"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let f = Flags::parse(&argv);
        assert_eq!(f.prompt, "> ");
        assert_eq!(f.query, "src");
        assert!(f.exact, "-e is fzf's exact-match flag");
        assert!(f.no_sort, "+s turns sorting off, it does not enable it");
        assert!(!f.multi, "multi is off unless -m/--multi is passed");
    }

    #[test]
    fn default_opts_are_split_with_shell_quoting() {
        // The prompt's trailing space only survives if the quotes are honoured,
        // which is the whole reason arb's shell_split exists.
        let req = Request {
            candidates: &[],
            command: None,
            options: &[],
            default_opts: "--prompt='<<)ZPWR(>> ' --reverse",
        };
        let argv = compose_argv(&req);
        let f = Flags::parse(&argv);
        assert_eq!(f.prompt, "<<)ZPWR(>> ");
    }

    #[test]
    fn per_request_options_override_the_environment() {
        let req = Request {
            candidates: &[],
            command: None,
            options: &["--prompt=Maps> ".to_string()],
            default_opts: "--prompt='env> '",
        };
        let f = Flags::parse(&compose_argv(&req));
        assert_eq!(f.prompt, "Maps> ", "the later flag wins, as in fzf");
    }

    #[test]
    fn the_select_spec_still_builds_a_picker() {
        // arb's DSL is the contract this module is written against: `--fzf` is
        // literally this one-widget spec. If arb ever renames the widget or the
        // `source .name { in }` form, every fzf.vim command would silently stop
        // opening a picker — this is where that shows up.
        let spec = select_spec().expect("arb still parses the select spec");
        assert!(spec.theme.is_none(), "an unthemed picker uses fzf's palette");
    }

    #[test]
    fn preview_substitution_quotes_the_line() {
        assert_eq!(shell_quote("a b.rs"), "'a b.rs'");
        assert_eq!(shell_quote("it's.rs"), r"'it'\''s.rs'");
    }
}
