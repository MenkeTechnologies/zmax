//! AWK binding over the embedded awkrs interpreter.
//!
//! AWK's natural editor role is a **text filter**: run a program over the
//! selection (or whole buffer) and replace it with the program's output. This
//! is stateless (a fresh awk runtime per call) and never touches process stdio
//! — output is captured via `awkrs::run_program`. The orchestration that reads
//! the region and writes the result lives in [`super::run_awk_filter`].

/// Run `program` over `input` and return its captured `print`/`printf` output.
#[cfg(unix)]
pub(super) fn run(program: &str, input: &str) -> Result<String, String> {
    awkrs::run_program(program, input).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
pub(super) fn run(_program: &str, _input: &str) -> Result<String, String> {
    Err("embedded awk is only supported on unix".into())
}
