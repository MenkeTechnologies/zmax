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
        // name, min args, max args (None = variadic), fn
        h.defsubr("editor-message", 1, Some(1), b_message);
        h.defsubr("editor-error", 1, Some(1), b_error);
        h.defsubr("editor-command", 1, None, b_command);
        h.defsubr("insert", 0, None, b_insert);
        h.defsubr("buffer-string", 0, Some(0), b_buffer_string);
        h.defsubr("point", 0, Some(0), b_point);
        h.defsubr("point-min", 0, Some(0), b_point_min);
        h.defsubr("point-max", 0, Some(0), b_point_max);
        h.defsubr("goto-char", 1, Some(1), b_goto_char);
        h.defsubr("buffer-substring", 2, Some(2), b_buffer_substring);
        h.defsubr("delete-region", 2, Some(2), b_delete_region);
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
    Ok(t(h))
}

fn b_insert(h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    let text: String = args.iter().map(|v| as_string(h, v)).collect();
    super::api_insert(&text)?;
    Ok(nil())
}

fn b_buffer_string(_h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    Ok(Value::str(super::api_buffer_string()?))
}

fn b_point(_h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    Ok(Value::Int(super::api_point()?))
}

fn b_point_min(_h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    Ok(Value::Int(super::api_point_min()?))
}

fn b_point_max(_h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    Ok(Value::Int(super::api_point_max()?))
}

fn b_goto_char(_h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    super::api_goto_char(args[0].to_int())?;
    Ok(nil())
}

fn b_buffer_substring(_h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    let s = super::api_buffer_substring(args[0].to_int(), args[1].to_int())?;
    Ok(Value::str(s))
}

fn b_delete_region(_h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    super::api_delete_region(args[0].to_int(), args[1].to_int())?;
    Ok(nil())
}

fn b_find_file(h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    let path = as_string(h, &args[0]);
    super::api_command("open", &[path])?;
    Ok(t(h))
}

fn b_save_buffer(h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    super::api_command("write", &[])?;
    Ok(t(h))
}
