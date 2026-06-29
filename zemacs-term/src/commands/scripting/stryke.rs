//! stryke binding over the embedded strykelang interpreter.
//!
//! This is the eval half: stryke source runs with persistent state (globals and
//! subs survive across calls via a thread-local `VMHelper`), and the program's
//! return value is shown. stryke routes `print` to a stdout fd our redirect
//! can't intercept (its embedded-zsh fds layer), so `print` is suppressed to
//! keep the TUI clean — capturing it needs an upstream output-sink hook. stryke
//! also has no native host-function registration yet (only source subs /
//! `rust{}` blocks), so the editor `api_*` surface isn't exposed to stryke;
//! both are upstream VM work, the next stryke milestone. Unix-only (stryke
//! depends on zshrs). The crate is referenced as `::stryke` to avoid shadowing
//! by this module's own name.

#[cfg(unix)]
use std::cell::RefCell;

#[cfg(unix)]
thread_local! {
    /// Persistent interpreter so `$x` / `sub f {…}` survive across `:stryke` calls.
    static VM: RefCell<Option<::stryke::vm_helper::VMHelper>> = const { RefCell::new(None) };
}

/// Evaluate stryke source and return the rendered value of the program's last
/// expression. `print` output is suppressed (see the module note). stderr
/// (e.g. `warn`) is contained by the fd-capture wrapper rather than leaking.
#[cfg(unix)]
pub(super) fn eval(code: &str) -> Result<String, String> {
    let (result, _contained) = super::capture::with_captured_fds(|| {
        VM.with(|cell| {
            let mut borrow = cell.borrow_mut();
            let vm = borrow.get_or_insert_with(::stryke::vm_helper::VMHelper::new);
            vm.suppress_stdout = true;
            ::stryke::parse_and_run_string(code, vm)
        })
    })?;

    match result {
        Ok(value) => Ok(value.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(not(unix))]
pub(super) fn eval(_code: &str) -> Result<String, String> {
    Err("embedded stryke is only supported on unix".into())
}
