//! arb binding over the embedded arblang stream/query language.
//!
//! arb is line-oriented — its natural editor role, like awk, is a **text
//! filter**: an arb spec's `out { … }` pipeline is compiled once and run over the
//! buffer (or selection) lines, and the buffer is replaced with the result. This
//! is stateless (a fresh parse/build per call) and never touches process stdio.
//! Only the `out` pipeline is evaluated; arb's TUI/widget/actor surfaces are not
//! meaningful as an in-editor filter. Unix-only (matches its siblings). The crate
//! is imported as `arb` (its lib name); the package is `arblang`.

/// Run an arb spec's `out { }` pipeline over `input`'s lines and return the
/// produced text. Errors carry arb's parse/build message.
#[cfg(unix)]
pub(super) fn run(program: &str, input: &str) -> Result<String, String> {
    let cmds = arb::parser::parse(program).map_err(|e| e.to_string())?;
    let spec = arb::spec::build(&cmds).map_err(|e| e.to_string())?;
    let ops = spec
        .out
        .ok_or_else(|| "spec has no `out { }` pipeline to run as a filter".to_string())?;

    let lines: Vec<String> = input.lines().map(str::to_string).collect();
    match arb::query::eval(&ops, &lines, 0.0) {
        arb::query::QueryResult::Lines(ls) => Ok(ls.join("\n")),
        arb::query::QueryResult::Scalar(n) => Ok(fmt_scalar(n)),
        // A group-by/count pipeline yields `key<TAB>count` rows, one per line.
        arb::query::QueryResult::Pairs(ps) => Ok(ps
            .iter()
            .map(|(k, n)| format!("{k}\t{n}"))
            .collect::<Vec<_>>()
            .join("\n")),
    }
}

/// Render an arb scalar the way its CLI does: integers without a trailing `.0`.
#[cfg(unix)]
fn fmt_scalar(n: f64) -> String {
    if n.fract() == 0.0 && n.is_finite() {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

#[cfg(not(unix))]
pub(super) fn run(_program: &str, _input: &str) -> Result<String, String> {
    Err("embedded arb is only supported on unix".into())
}
