//! Tcl binding over the embedded tclrs interpreter.
//!
//! tclrs is a fusevm frontend that captures its own output (`Interp::capturing`
//! + `take_output`), so — like phplang — it needs no process-fd redirect. What
//! it does need is **stack**: a nested `eval`/`uplevel`/proc call runs on a VM
//! of its own, so tclrs's own driver runs scripts on a thread with
//! [`tclrs::runtime::RECOMMENDED_STACK`] (`tclrs/src/main.rs:65`), which the
//! default recursion limit of 1000 levels is sized against. The editor thread
//! has the ordinary 8 MiB, so a deep proc would overflow it — a signal, not a
//! catchable error.
//!
//! One tclrs limitation reaches the user directly: its compiler requires braced
//! expressions, so `expr {$a + 1}` compiles and the unbraced `expr $a + 1` is
//! refused with "expression must be a literal in this phase"
//! (`tclrs/src/compiler.rs:963`). The braced form is idiomatic Tcl, so the
//! binding passes source through untouched rather than rewriting it.
//!
//! So the interpreter lives on a dedicated worker thread with that stack, and
//! `:tcl` sends it source and blocks for the answer. The worker also gives Tcl
//! the persistence its REPL wants: `set`/`proc` survive across calls, the same
//! contract zsh and stryke have. A panicking script kills the worker; the next
//! call notices the dead channel and spawns a fresh one.

#[cfg(unix)]
use std::cell::RefCell;
#[cfg(unix)]
use std::sync::mpsc::{channel, Receiver, Sender};

#[cfg(unix)]
thread_local! {
    /// The worker owning the persistent interpreter, spawned on first use.
    static WORKER: RefCell<Option<Worker>> = const { RefCell::new(None) };
}

/// Channel pair to the interpreter thread: source in, rendered result out.
#[cfg(unix)]
struct Worker {
    tx: Sender<String>,
    rx: Receiver<Result<String, String>>,
}

#[cfg(unix)]
impl Worker {
    /// Spawn the interpreter thread. The interpreter is built *on* that thread,
    /// so nothing about it has to be `Send`.
    fn spawn() -> Result<Worker, String> {
        let (tx, src_rx) = channel::<String>();
        let (res_tx, rx) = channel::<Result<String, String>>();

        std::thread::Builder::new()
            .name("zmax-tcl".into())
            .stack_size(tclrs::runtime::RECOMMENDED_STACK)
            .spawn(move || {
                let mut interp = tclrs::Interp::capturing();
                // `src_rx` ends when the editor thread drops the worker.
                for src in src_rx {
                    let value = interp.eval(&src).map_err(|e| e.to_string());
                    let output = interp.take_output();
                    let reply = match value {
                        Ok(value) => Ok(super::pick_output(&output, &value)),
                        Err(e) => Err(super::join_output(&output, &e)),
                    };
                    if res_tx.send(reply).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("could not start the tcl interpreter thread: {e}"))?;

        Ok(Worker { tx, rx })
    }

    /// Run one script, blocking until the worker answers. `None` means the
    /// worker is gone (a panicking script took the thread with it) — the script
    /// result and a dead channel are different failures, and only the second
    /// one invalidates the worker.
    fn call(&self, code: &str) -> Option<Result<String, String>> {
        self.tx.send(code.to_string()).ok()?;
        self.rx.recv().ok()
    }
}

/// Evaluate Tcl source and return what it printed, falling back to the value of
/// its last command when it printed nothing (the `tclsh` convention). State
/// persists across calls.
#[cfg(unix)]
pub(super) fn eval(code: &str) -> Result<String, String> {
    WORKER.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if borrow.is_none() {
            *borrow = Some(Worker::spawn()?);
        }
        match borrow.as_ref().expect("worker was just installed").call(code) {
            Some(result) => result,
            None => {
                // A dead worker is not reusable: drop it so the next call respawns.
                *borrow = None;
                Err("the tcl interpreter thread died running that script".into())
            }
        }
    })
}

#[cfg(not(unix))]
pub(super) fn eval(_code: &str) -> Result<String, String> {
    Err("embedded tcl is only supported on unix".into())
}
