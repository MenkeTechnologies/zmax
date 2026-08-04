//! stryke binding over the embedded strykelang interpreter.
//!
//! stryke source runs with persistent state (globals and subs survive across
//! calls via a thread-local `VMHelper`). `print`/`say`/`printf` output is
//! captured in-process through the VM's own sink, so it is *shown* rather than
//! discarded — until strykelang grew that sink the only safe option was
//! `suppress_stdout`, which threw the output away and left `:stryke` able to
//! report a program's last value and nothing it printed.
//!
//! stryke still has no native host-function registration (only source subs /
//! `rust{}` blocks), so the editor `api_*` surface is not exposed to it; that
//! remains upstream VM work. Unix-only (stryke depends on zshrs). The crate is
//! referenced as `::stryke` to avoid shadowing by this module's own name.

#[cfg(unix)]
use std::cell::RefCell;

#[cfg(unix)]
thread_local! {
    /// Persistent interpreter so `$x` / `sub f {…}` survive across `:stryke` calls.
    static VM: RefCell<Option<::stryke::vm_helper::VMHelper>> = const { RefCell::new(None) };
}

/// Evaluate stryke source and return what it printed, falling back to the
/// rendered value of the program's last expression when it printed nothing.
#[cfg(unix)]
pub(super) fn eval(code: &str) -> Result<String, String> {
    run(code, None)
}

/// Run `code` as a pipeline stage with `input` bound to `$stdin`. The binding is
/// a real scalar declared on the VM, so nothing is escaped: the input is data,
/// never syntax.
#[cfg(unix)]
pub(super) fn filter(code: &str, input: &str) -> Result<String, String> {
    run(code, Some(input))
}

#[cfg(unix)]
fn run(code: &str, input: Option<&str>) -> Result<String, String> {
    VM.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let vm = borrow.get_or_insert_with(::stryke::vm_helper::VMHelper::new);
        if let Some(input) = input {
            vm.bind_scalar("stdin", input);
        }
        vm.begin_capture();
        let result = ::stryke::parse_and_run_string(code, vm);
        let output = vm.end_capture();
        match result {
            Ok(value) => Ok(super::pick_output(&output, &value.to_string())),
            Err(e) => Err(super::join_output(&output, &e.to_string())),
        }
    })
}

#[cfg(not(unix))]
pub(super) fn eval(_code: &str) -> Result<String, String> {
    Err("embedded stryke is only supported on unix".into())
}

#[cfg(not(unix))]
pub(super) fn filter(_code: &str, _input: &str) -> Result<String, String> {
    Err("embedded stryke is only supported on unix".into())
}
