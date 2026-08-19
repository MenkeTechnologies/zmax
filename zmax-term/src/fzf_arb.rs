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
    /// zmax's own additions on top of the user's fzf configuration: the
    /// `fzf.options`/`fzf.preview` config and the CTRL-T options. Parsed with
    /// fzf's quoting rules. `$FZF_DEFAULT_OPTS_FILE` and `$FZF_DEFAULT_OPTS`
    /// are NOT included here — [`compose_argv`] reads them itself, in fzf's own
    /// precedence, so there is one place that knows the environment.
    pub default_opts: &'a str,
    /// The terminal's size, for the `FZF_LINES`/`FZF_COLUMNS` a preview command
    /// is entitled to read. The caller has it; a worker thread does not.
    pub term_size: (u16, u16),
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
    // The rest are not acted on here — they are read back out and handed to
    // preview children as the `FZF_*` variables fzf exports (see [`child_env`]),
    // which is the only way a preview command can know them.
    nth: String,
    with_nth: String,
    ghost: String,
    wrap: String,
    preview_label: String,
    border_label: String,
    list_label: String,
    input_label: String,
    header_label: String,
    no_input: bool,
    hidden_input: bool,
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
                "--nth" | "-n" => f.nth = value(argv, &mut i, inline),
                "--with-nth" => f.with_nth = value(argv, &mut i, inline),
                "--ghost" => f.ghost = value(argv, &mut i, inline),
                "--wrap" => f.wrap = "char".to_string(),
                "--wrap-sign" => f.wrap = "char".to_string(),
                "--preview-label" => f.preview_label = value(argv, &mut i, inline),
                "--border-label" => f.border_label = value(argv, &mut i, inline),
                "--list-label" => f.list_label = value(argv, &mut i, inline),
                "--input-label" => f.input_label = value(argv, &mut i, inline),
                "--header-label" => f.header_label = value(argv, &mut i, inline),
                "--no-input" => f.no_input = true,
                "--hidden-input" => f.hidden_input = true,
                _ => {}
            }
            i += 1;
        }
        f
    }
}

/// argv as arb's `main` would see it, in fzf's own precedence (fzf(1),
/// ENVIRONMENT VARIABLES): `$FZF_DEFAULT_OPTS_FILE`, then `$FZF_DEFAULT_OPTS`,
/// then the command line — later wins, so an explicit per-command flag
/// overrides the environment, and zmax's own config overrides both.
///
/// `arb::fzf::env_args` is the reader for the two environment entries: the same
/// one `arb --fzf` uses, so a themed setup keeps its prompt, layout, border,
/// colors and `--bind` table here without any of it being re-implemented.
fn compose_argv(req: &Request<'_>) -> Vec<String> {
    let mut argv = vec!["arb".to_string(), "--fzf".to_string()];
    argv.extend(arb::fzf::env_args());
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
#[allow(clippy::too_many_arguments)]
fn spawn_preview(
    template: String,
    controls: Arc<Mutex<arb::tui::Controls>>,
    pane: Arc<Mutex<StreamState>>,
    state: Arc<Mutex<StreamState>>,
    look: arb::fzf::Look,
    flags: Arc<PreviewFlags>,
    term_size: (u16, u16),
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
            let out = Command::new(&sh[0])
                .args(&sh[1..])
                .arg(&cmd)
                .envs(child_env(&controls, &state, &look, &flags, term_size))
                .output();
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

/// The subset of [`Flags`] a preview child is entitled to see. Split out so the
/// preview thread can hold it without holding the whole parse.
pub(crate) struct PreviewFlags {
    prompt: String,
    ghost: String,
    wrap: String,
    nth: String,
    with_nth: String,
    preview_label: String,
    border_label: String,
    list_label: String,
    input_label: String,
    header_label: String,
    input_state: &'static str,
}

/// The `FZF_*` variables fzf exports to its child processes (fzf(1),
/// "ENVIRONMENT VARIABLES EXPORTED TO CHILD PROCESSES"), computed fresh for
/// every preview run because most of them move as you type.
///
/// Deliberately NOT set, because this process cannot compute them honestly and
/// an invented value is worse than an unset one:
///
/// * `FZF_ACTION`, `FZF_KEY`, `FZF_IDLE_TIME`, `FZF_IDLE_TIME_MS` — per-keystroke
///   state that lives inside arb's event loop and is not on `Controls`.
/// * `FZF_PORT`, `FZF_SOCK` — `--listen` only, which this picker does not run.
///   fzf leaves them unset without it too.
/// * `FZF_RAW` — raw mode only.
fn child_env(
    controls: &Arc<Mutex<arb::tui::Controls>>,
    state: &Arc<Mutex<StreamState>>,
    look: &arb::fzf::Look,
    flags: &PreviewFlags,
    term_size: (u16, u16),
) -> Vec<(String, String)> {
    let (cols, rows) = term_size;
    let (query, current, marks, cursor) = match controls.lock() {
        Ok(c) => (
            c.filter.clone(),
            c.current.clone(),
            c.marks.len(),
            c.cursor,
        ),
        Err(_) => return Vec::new(),
    };
    // The match count is the renderer's own predicate over the same stream, so
    // the number a preview reads is the number on screen.
    let (total, matched) = match state.lock() {
        Ok(s) => (
            s.lines.len(),
            s.lines
                .iter()
                .filter(|l| arb::tui::filter_matches(l, &query))
                .count(),
        ),
        Err(_) => (0, 0),
    };
    let (pv_rows, pv_cols) = preview_geometry(&look.preview_window, term_size);
    let mut env: Vec<(String, String)> = vec![
        ("FZF_LINES".into(), rows.to_string()),
        ("FZF_COLUMNS".into(), cols.to_string()),
        // fzf reports where the LIST grows: its default layout puts the prompt
        // at the bottom and grows upward, `--reverse` puts it at the top.
        (
            "FZF_DIRECTION".into(),
            match look.layout {
                arb::fzf::Layout::Default => "up",
                arb::fzf::Layout::Reverse | arb::fzf::Layout::ReverseList => "down",
            }
            .into(),
        ),
        ("FZF_TOTAL_COUNT".into(), total.to_string()),
        ("FZF_MATCH_COUNT".into(), matched.to_string()),
        ("FZF_SELECT_COUNT".into(), marks.to_string()),
        // fzf's position is 1-based over the matches; arb's cursor is a 0-based
        // offset from the best match, so the two differ by one.
        ("FZF_POS".into(), (cursor + 1).to_string()),
        ("FZF_QUERY".into(), query),
        ("FZF_INPUT_STATE".into(), flags.input_state.into()),
        ("FZF_PROMPT".into(), flags.prompt.clone()),
        ("FZF_POINTER".into(), look.pointer.clone()),
        ("FZF_PREVIEW_LINES".into(), pv_rows.to_string()),
        ("FZF_PREVIEW_COLUMNS".into(), pv_cols.to_string()),
    ];
    if let Some(pane) = look.preview_window.split(ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: cols,
        height: rows.saturating_sub(2),
    }).1
    {
        // The box's own border is where fzf counts its content from.
        env.push(("FZF_PREVIEW_TOP".into(), (pane.y + 1).to_string()));
        env.push(("FZF_PREVIEW_LEFT".into(), (pane.x + 1).to_string()));
    }
    // "FZF_CURRENT_ITEM is omitted when the item contains a NUL byte, because
    // exec(2) cannot pass it. It is also omitted when the item is larger than
    // 64 KB, so that a huge item cannot overflow the environment size limit."
    if !current.contains('\0') && current.len() <= 64 * 1024 {
        env.push(("FZF_CURRENT_ITEM".into(), current));
    }
    // The remaining ones exist only when the corresponding option was given;
    // fzf leaves them unset otherwise rather than exporting an empty string.
    for (name, value) in [
        ("FZF_GHOST", &flags.ghost),
        ("FZF_WRAP", &flags.wrap),
        ("FZF_NTH", &flags.nth),
        ("FZF_WITH_NTH", &flags.with_nth),
        ("FZF_PREVIEW_LABEL", &flags.preview_label),
        ("FZF_BORDER_LABEL", &flags.border_label),
        ("FZF_LIST_LABEL", &flags.list_label),
        ("FZF_INPUT_LABEL", &flags.input_label),
        ("FZF_HEADER_LABEL", &flags.header_label),
    ] {
        if !value.is_empty() {
            env.push((name.into(), value.clone()));
        }
    }
    env
}

/// Size of the preview pane, in rows and columns, for `FZF_PREVIEW_LINES` and
/// `FZF_PREVIEW_COLUMNS`.
///
/// Asks arb's own `--preview-window` layout for the rectangle rather than
/// re-deriving it, so the numbers a preview command reads are the ones it is
/// actually drawn into. The box's border costs a row and a column at each edge,
/// which is what the pane loses against its rectangle
/// (`render_preview_pane`, vendor/arblang/src/tui.rs).
fn preview_geometry(window: &arb::fzf::PreviewWindow, (cols, rows): (u16, u16)) -> (u16, u16) {
    // The body is the terminal minus the prompt row and the info/separator row.
    let body = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: cols,
        height: rows.saturating_sub(2),
    };
    match window.split(body).1 {
        Some(pane) => (
            pane.height.saturating_sub(2),
            pane.width.saturating_sub(2),
        ),
        // `hidden`, or a zero size: fzf still runs the command, and reports the
        // window it would have drawn as empty.
        None => (0, 0),
    }
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

    let look_for_preview = look.clone();
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
        let pv = Arc::new(PreviewFlags {
            // fzf exports the prompt it actually shows, which is `> ` when the
            // option was never given.
            prompt: match flags.prompt.is_empty() {
                true => "> ".to_string(),
                false => flags.prompt.clone(),
            },
            ghost: flags.ghost.clone(),
            wrap: flags.wrap.clone(),
            nth: flags.nth.clone(),
            with_nth: flags.with_nth.clone(),
            preview_label: flags.preview_label.clone(),
            border_label: flags.border_label.clone(),
            list_label: flags.list_label.clone(),
            input_label: flags.input_label.clone(),
            header_label: flags.header_label.clone(),
            input_state: match (flags.no_input, flags.hidden_input) {
                (true, _) => "disabled",
                (_, true) => "hidden",
                _ => "enabled",
            },
        });
        spawn_preview(
            template.clone(),
            Arc::clone(&controls),
            Arc::clone(&pane),
            Arc::clone(&state),
            look_for_preview.clone(),
            pv,
            req.term_size,
        );
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
            term_size: (80, 24),
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
            term_size: (80, 24),
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

    /// Every variable fzf(1) lists under "ENVIRONMENT VARIABLES EXPORTED TO
    /// CHILD PROCESSES", and whether this picker can set it. The four `false`
    /// rows are the ones no honest value exists for here; the test exists so
    /// that list stays a decision rather than an oversight.
    const EXPORTED: &[(&str, bool)] = &[
        ("FZF_LINES", true),
        ("FZF_COLUMNS", true),
        ("FZF_DIRECTION", true),
        ("FZF_TOTAL_COUNT", true),
        ("FZF_MATCH_COUNT", true),
        ("FZF_SELECT_COUNT", true),
        ("FZF_POS", true),
        ("FZF_CURRENT_ITEM", true),
        ("FZF_QUERY", true),
        ("FZF_INPUT_STATE", true),
        ("FZF_PROMPT", true),
        ("FZF_POINTER", true),
        ("FZF_PREVIEW_TOP", true),
        ("FZF_PREVIEW_LEFT", true),
        ("FZF_PREVIEW_LINES", true),
        ("FZF_PREVIEW_COLUMNS", true),
        ("FZF_NTH", true),
        ("FZF_WITH_NTH", true),
        ("FZF_GHOST", true),
        ("FZF_WRAP", true),
        ("FZF_PREVIEW_LABEL", true),
        ("FZF_BORDER_LABEL", true),
        ("FZF_LIST_LABEL", true),
        ("FZF_INPUT_LABEL", true),
        ("FZF_HEADER_LABEL", true),
        // arb's event loop owns these and `Controls` does not publish them.
        ("FZF_ACTION", false),
        ("FZF_KEY", false),
        ("FZF_IDLE_TIME", false),
        ("FZF_IDLE_TIME_MS", false),
        // `--listen` / raw mode, neither of which this picker runs. fzf leaves
        // them unset in that case too.
        ("FZF_PORT", false),
        ("FZF_SOCK", false),
        ("FZF_RAW", false),
    ];

    fn env_for_test(query: &str, current: &str, lines: &[&str]) -> Vec<(String, String)> {
        let controls = Arc::new(Mutex::new(arb::tui::Controls::default()));
        {
            let mut c = controls.lock().unwrap();
            c.filter = query.to_string();
            c.current = current.to_string();
            c.cursor = 2;
            c.marks = vec!["one".into()];
        }
        let state = Arc::new(Mutex::new(StreamState::with_cap(usize::MAX)));
        for l in lines {
            state.lock().unwrap().push(l.to_string());
        }
        // Every optional flag is populated: the table below asks what this
        // picker CAN export, and an option that was never passed is omitted on
        // purpose (asserted separately).
        let flags = PreviewFlags {
            prompt: "> ".into(),
            ghost: "type to filter".into(),
            wrap: "char".into(),
            nth: "2..".into(),
            with_nth: "1,2".into(),
            preview_label: "preview".into(),
            border_label: "border".into(),
            list_label: "list".into(),
            input_label: "input".into(),
            header_label: "header".into(),
            input_state: "enabled",
        };
        child_env(
            &controls,
            &state,
            &arb::fzf::Look::default(),
            &flags,
            (100, 30),
        )
    }

    #[test]
    fn every_exported_variable_is_accounted_for() {
        let env = env_for_test("a", "alpha", &["alpha", "beta"]);
        let set: std::collections::HashSet<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        for (name, expected) in EXPORTED {
            assert_eq!(
                set.contains(name),
                *expected,
                "{name} should{} be exported to preview children",
                if *expected { "" } else { " not" }
            );
        }
    }

    #[test]
    fn counts_and_position_follow_the_query() {
        let env = env_for_test("al", "alpha", &["alpha", "beta", "alfalfa"]);
        let get = |k: &str| {
            env.iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        assert_eq!(get("FZF_TOTAL_COUNT"), "3");
        // "beta" does not contain "al"; the other two do.
        assert_eq!(get("FZF_MATCH_COUNT"), "2");
        assert_eq!(get("FZF_SELECT_COUNT"), "1");
        // arb's cursor is a 0-based offset, fzf's position is 1-based.
        assert_eq!(get("FZF_POS"), "3");
        assert_eq!(get("FZF_QUERY"), "al");
        assert_eq!(get("FZF_CURRENT_ITEM"), "alpha");
        assert_eq!(get("FZF_NTH"), "2..");
        assert_eq!(get("FZF_LINES"), "30");
        assert_eq!(get("FZF_COLUMNS"), "100");
    }

    #[test]
    fn an_option_that_was_never_passed_is_not_exported_as_empty() {
        // A preview that tests `[ -n "$FZF_GHOST" ]` must see the option's
        // absence, not an empty string that looks like it was set.
        let controls = Arc::new(Mutex::new(arb::tui::Controls::default()));
        let state = Arc::new(Mutex::new(StreamState::with_cap(usize::MAX)));
        let flags = PreviewFlags {
            prompt: "> ".into(),
            ghost: String::new(),
            wrap: String::new(),
            nth: String::new(),
            with_nth: String::new(),
            preview_label: String::new(),
            border_label: String::new(),
            list_label: String::new(),
            input_label: String::new(),
            header_label: String::new(),
            input_state: "enabled",
        };
        let env = child_env(
            &controls,
            &state,
            &arb::fzf::Look::default(),
            &flags,
            (80, 24),
        );
        for absent in ["FZF_GHOST", "FZF_WRAP", "FZF_NTH", "FZF_WITH_NTH"] {
            assert!(
                !env.iter().any(|(k, _)| k == absent),
                "{absent} was exported despite the option never being passed"
            );
        }
        // The unconditional ones are still there.
        assert!(env.iter().any(|(k, _)| k == "FZF_PROMPT"));
    }

    #[test]
    fn a_nul_bearing_item_is_omitted_not_truncated() {
        // fzf(1): "FZF_CURRENT_ITEM is omitted when the item contains a NUL
        // byte, because exec(2) cannot pass it."
        let env = env_for_test("", "with\0nul", &["with\0nul"]);
        assert!(!env.iter().any(|(k, _)| k == "FZF_CURRENT_ITEM"));
    }

    #[test]
    fn the_preview_geometry_follows_preview_window() {
        // Default is fzf's right half: 50 columns less the box border, and the
        // body (terminal less prompt and info rows) less the border.
        let default = arb::fzf::PreviewWindow::default();
        assert_eq!(preview_geometry(&default, (100, 30)), (26, 48));

        // A different spec moves and resizes it, and the reported numbers move
        // with it — this is what was inert before arb honored the flag.
        let down = arb::fzf::PreviewWindow::parse("down,10");
        assert_eq!(preview_geometry(&down, (100, 30)), (8, 98));
        let left = arb::fzf::PreviewWindow::parse("left,25%");
        assert_eq!(preview_geometry(&left, (100, 30)), (26, 23));

        // Hidden still runs the command, and reports an empty window.
        let hidden = arb::fzf::PreviewWindow::parse("hidden");
        assert_eq!(preview_geometry(&hidden, (100, 30)), (0, 0));

        // A tiny terminal saturates instead of underflowing.
        assert_eq!(preview_geometry(&default, (2, 1)), (0, 0));
    }

    #[test]
    fn the_preview_corner_follows_the_position() {
        let env = |spec: &str| {
            let controls = Arc::new(Mutex::new(arb::tui::Controls::default()));
            let state = Arc::new(Mutex::new(StreamState::with_cap(usize::MAX)));
            let flags = PreviewFlags {
                prompt: "> ".into(),
                ghost: String::new(),
                wrap: String::new(),
                nth: String::new(),
                with_nth: String::new(),
                preview_label: String::new(),
                border_label: String::new(),
                list_label: String::new(),
                input_label: String::new(),
                header_label: String::new(),
                input_state: "enabled",
            };
            let look = arb::fzf::Look {
                preview_window: arb::fzf::PreviewWindow::parse(spec),
                ..arb::fzf::Look::default()
            };
            let env = child_env(&controls, &state, &look, &flags, (100, 30));
            let get = |k: &str| {
                env.iter()
                    .find(|(n, _)| n == k)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            (get("FZF_PREVIEW_TOP"), get("FZF_PREVIEW_LEFT"))
        };
        // right: the box starts where the list ends.
        assert_eq!(env("right,50%"), ("1".to_string(), "51".to_string()));
        // left: at the first column instead.
        assert_eq!(env("left,50%"), ("1".to_string(), "1".to_string()));
        // down: below the list.
        assert_eq!(env("down,10"), ("19".to_string(), "1".to_string()));
    }

    #[test]
    fn opts_file_and_opts_are_both_read() {
        // fzf(1) ENVIRONMENT VARIABLES: $FZF_DEFAULT_OPTS_FILE first, then
        // $FZF_DEFAULT_OPTS, then the command line. `arb::fzf::env_args` is the
        // reader; this asserts it is actually wired into the composed argv.
        let dir = std::env::temp_dir().join("zmax-fzf-optsfile-test");
        std::fs::write(&dir, "--prompt='file> '\n").expect("write opts file");
        // SAFETY: single-threaded test, and both vars are restored below.
        unsafe {
            std::env::set_var("FZF_DEFAULT_OPTS_FILE", &dir);
            std::env::set_var("FZF_DEFAULT_OPTS", "--pointer=>>");
        }
        let argv = compose_argv(&Request {
            candidates: &[],
            command: None,
            options: &[],
            default_opts: "",
            term_size: (80, 24),
        });
        unsafe {
            std::env::remove_var("FZF_DEFAULT_OPTS_FILE");
            std::env::remove_var("FZF_DEFAULT_OPTS");
        }
        let _ = std::fs::remove_file(&dir);
        assert!(
            argv.iter().any(|a| a == "--prompt=file> " || a == "file> "),
            "the opts FILE reached the argv: {argv:?}"
        );
        assert!(
            argv.iter().any(|a| a.contains(">>")),
            "$FZF_DEFAULT_OPTS reached the argv: {argv:?}"
        );
    }

    #[test]
    fn preview_substitution_quotes_the_line() {
        assert_eq!(shell_quote("a b.rs"), "'a b.rs'");
        assert_eq!(shell_quote("it's.rs"), r"'it'\''s.rs'");
    }
}
