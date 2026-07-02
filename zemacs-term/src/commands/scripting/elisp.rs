//! Emacs Lisp binding: registers the uniform editor API ([`super`]) as elisp
//! subrs on the (thread-local) elisprs host, and marshals fusevm values.

use std::cell::Cell;

use elisprs::host::ElispHost;
use elisprs::{with_host, Value};

thread_local! {
    static BUILTINS_READY: Cell<bool> = const { Cell::new(false) };
}

/// Install the editor subrs into the elisp host exactly once per thread.
pub(super) fn ensure_builtins() {
    if BUILTINS_READY.with(|c| c.get()) {
        return;
    }
    BUILTINS_READY.with(|c| c.set(true));
    with_host(|h| {
        // name, min args, max args (None = variadic), fn.
        //
        // Only editor-level operations (status line, command dispatch, files)
        // are bound here. Buffer-text builtins — point/insert/goto-char/
        // forward-line/search-forward/looking-at/… — are elisprs's own subrs;
        // they run against a mirror of the live buffer that `eval_elisp` syncs
        // in and out (see super::load_buffer_into_host / flush_host_into_buffer),
        // so we must NOT override them here or the two would fight over point.
        h.defsubr("editor-message", 1, Some(1), b_message);
        h.defsubr("editor-error", 1, Some(1), b_error);
        h.defsubr("editor-command", 1, None, b_command);
        h.defsubr("find-file", 1, Some(1), b_find_file);
        h.defsubr("save-buffer", 0, Some(0), b_save_buffer);
    });
}

// ── marshalling ──

/// Coerce an elisp value to a Rust string (strings verbatim, symbols by name,
/// everything else via `prin1`-free printing).
fn as_string(h: &ElispHost, v: &Value) -> String {
    match v {
        Value::Str(s) => s.as_str().to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        _ => h.sym_name(v).unwrap_or_else(|| h.print(v, false)),
    }
}

/// Elisp truth: `nil` is `Value::Undef`, true is the interned symbol `t`.
fn t(h: &mut ElispHost) -> Value {
    h.intern("t")
}

fn nil() -> Value {
    Value::Undef
}

// ── subr implementations (thin marshallers over super::api_*) ──

fn b_message(h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    let s = as_string(h, &args[0]);
    super::api_message(&s)?;
    Ok(Value::str(s))
}

fn b_error(h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    let s = as_string(h, &args[0]);
    super::api_error(&s)?;
    // elisp `error` signals; here we surface it and return nil.
    Ok(nil())
}

fn b_command(h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    let name = as_string(h, &args[0]);
    let rest: Vec<String> = args[1..].iter().map(|v| as_string(h, v)).collect();
    super::api_command(&name, &rest)?;
    // The command may have switched the current buffer (`:open`, buffer motions,
    // …). Reload elisp's mirror from the now-current live buffer so later buffer
    // ops in this eval — and the final flush — target the right buffer.
    super::load_buffer_into_host(h);
    Ok(t(h))
}

fn b_find_file(h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    let path = as_string(h, &args[0]);
    super::api_command("open", &[path])?;
    // `find-file` switches buffers; re-mirror the newly opened one (see b_command).
    super::load_buffer_into_host(h);
    Ok(t(h))
}

fn b_save_buffer(h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    super::api_command("write", &[])?;
    Ok(t(h))
}
