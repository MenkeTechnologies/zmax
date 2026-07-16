//! Emacs Lisp binding: registers the uniform editor API ([`super`]) as elisp
//! subrs on the (thread-local) elisprs host, and marshals fusevm values.

use elisprs::host::ElispHost;
use elisprs::{with_host, Value};

/// Low-level editor lisp: the Emacs command/file/window functions that scripts
/// call, expressed on top of the `editor-*` subrs. Routing through
/// `editor-command` (b_command) means each one re-mirrors the live buffer after
/// running, so a command that switches buffers stays coherent. `message` reuses
/// elisprs's own `format`, so it must load after the prelude.
const EDITOR_LISP: &str = r#"
;; `message` writes to the echo area (the status line) like Emacs, reusing the
;; elisp formatter; a nil FORMAT clears it and returns nil.
(defun message (fmt &rest args)
  (if fmt
      (let ((s (apply #'format fmt args)))
        (editor-message s)
        s)
    (editor-message "")
    nil))

;; File / buffer / window commands → zmax typable commands.
(defun find-file (filename &rest _) (editor-command "open" filename) t)
(defalias 'find-file-existing #'find-file)
(defun save-buffer (&optional _arg) (editor-command "write") t)
(defun write-file (filename &rest _) (editor-command "write" filename) t)
(defun save-some-buffers (&rest _) (editor-command "write-all") t)
(defun revert-buffer (&rest _) (editor-command "reload") t)
(defun kill-buffer (&optional _b) (editor-command "buffer-close") t)
(defun kill-this-buffer (&rest _) (editor-command "buffer-close") t)
(defun next-buffer (&rest _) (editor-command "buffer-next") t)
(defun previous-buffer (&rest _) (editor-command "buffer-previous") t)
(defun switch-to-buffer (buffer &rest _) (editor-command "buffer" buffer) t)
(defun split-window-below (&rest _) (editor-command "hsplit") t)
(defun split-window-right (&rest _) (editor-command "vsplit") t)
(defun goto-line (line &rest _) (editor-command "goto" (number-to-string line)) t)
(defun kill-emacs (&rest _) (editor-command "quit-all"))
(defun save-buffers-kill-terminal (&rest _) (editor-command "write-quit-all"))
(defalias 'save-buffers-kill-emacs #'save-buffers-kill-terminal)
;; Sentinel so we can detect that this layer is loaded even after a host reset.
(defvar zmax--editor-lisp-loaded t)
"#;

/// Install the editor subrs into the elisp host if they are not already bound.
///
/// We probe the actual function slot rather than a thread-local flag: elisprs's
/// `eval_file` (and any `reset_host`) wipes the whole host, and a stale "already
/// installed" flag would leave the editor subrs permanently void. The probe is
/// one intern + slot check, so it is cheap to run on every eval.
pub(super) fn ensure_builtins() {
    let installed = with_host(|h| {
        let sym = h.intern("editor-command");
        h.is_fbound(&sym)
    });
    if installed {
        return;
    }
    with_host(|h| {
        // name, min args, max args (None = variadic), fn.
        //
        // Only editor-level operations (status line, command dispatch, buffer
        // identity) are bound here. Buffer-text builtins — point/insert/
        // goto-char/forward-line/search-forward/looking-at/… — are elisprs's own
        // subrs; they run against a mirror of the live buffer that `eval_elisp`
        // syncs in and out (see super::load_buffer_into_host /
        // flush_host_into_buffer), so we must NOT override them here or the two
        // would fight over point. Higher-level Emacs commands live in
        // `EDITOR_LISP` (they route through `editor-command`).
        h.defsubr("editor-message", 1, Some(1), b_message);
        h.defsubr("editor-error", 1, Some(1), b_error);
        h.defsubr("editor-command", 1, None, b_command);
        h.defsubr("buffer-file-name", 0, Some(1), b_buffer_file_name);
        h.defsubr("buffer-name", 0, Some(1), b_buffer_name);
        // Mark & region. Point lives in the mirrored EditBuffer; the mark is held
        // in `super` and flushed to the live selection anchor, so a script that
        // sets the mark and moves point ends up with a real editor selection.
        h.defsubr("set-mark", 1, Some(1), b_set_mark);
        h.defsubr("push-mark", 0, Some(3), b_push_mark);
        h.defsubr("mark", 0, Some(1), b_mark);
        h.defsubr("region-beginning", 0, Some(0), b_region_beginning);
        h.defsubr("region-end", 0, Some(0), b_region_end);
        h.defsubr("region-active-p", 0, Some(0), b_region_active_p);
        h.defsubr("use-region-p", 0, Some(0), b_region_active_p);
        h.defsubr("deactivate-mark", 0, Some(1), b_deactivate_mark);
        h.defsubr("mark-whole-buffer", 0, Some(0), b_mark_whole_buffer);
        // Kill ring & yank. Text is cut/pasted on the mirror; the ring lives in
        // `super` and each kill also lands in the editor's yank/clipboard registers.
        h.defsubr("kill-new", 1, Some(2), b_kill_new);
        h.defsubr("kill-region", 2, Some(3), b_kill_region);
        h.defsubr("copy-region-as-kill", 2, Some(2), b_copy_region_as_kill);
        h.defsubr("kill-ring-save", 2, Some(3), b_copy_region_as_kill);
        h.defsubr("yank", 0, Some(1), b_yank);
        h.defsubr("current-kill", 1, Some(2), b_current_kill);
        h.defsubr("kill-line", 0, Some(1), b_kill_line);
        h.defsubr("kill-whole-line", 0, Some(1), b_kill_whole_line);
    });
}

/// Evaluate the editor-lisp command layer if it is not already present. Must run
/// after [`ensure_builtins`] (needs the `editor-*` subrs) and can only run once
/// the prelude is available (it uses `format`/`apply`), which `eval_str`
/// guarantees. Probes the `zmax--editor-lisp-loaded` sentinel so it reloads
/// after a host reset rather than trusting a stale flag (see [`ensure_builtins`]).
pub(super) fn ensure_editor_lisp() {
    let loaded = with_host(|h| {
        let sym = h.intern("zmax--editor-lisp-loaded");
        h.is_bound(&sym)
    });
    if loaded {
        return;
    }
    if let Err(e) = elisprs::eval_str(EDITOR_LISP) {
        log::debug!("elisp editor layer failed to load: {e}");
    }
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

/// `buffer-file-name` → the live buffer's absolute path, or nil if unsaved.
/// The optional BUFFER argument is accepted (scripts pass it) but ignored:
/// elisp sees a single current buffer mirrored from the editor.
fn b_buffer_file_name(_h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    let path = super::api_buf_name()?;
    Ok(if path.is_empty() {
        nil()
    } else {
        Value::str(path)
    })
}

/// `buffer-name` → the live buffer's file name (final path component), or
/// "*scratch*" for an unnamed buffer, mirroring Emacs's default names.
fn b_buffer_name(_h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    let path = super::api_buf_name()?;
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "*scratch*".to_string());
    Ok(Value::str(name))
}

// ── mark & region ──
//
// Positions are 1-based points at the elisp boundary and 0-based char offsets in
// the mark state (`super::mark_*`). Current point is read from the mirrored
// `EditBuffer` so these stay consistent with elisp's own point-moving builtins.

/// A nil optional arg is `Value::Undef`; treat that (and a missing arg) as absent.
fn opt(args: &[Value], idx: usize) -> Option<&Value> {
    args.get(idx).filter(|v| !matches!(v, Value::Undef))
}

fn b_set_mark(_h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    let pos = args[0].to_int().max(1) as usize;
    super::mark_set(pos - 1);
    Ok(Value::Int(pos as i64))
}

fn b_push_mark(h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    let pos = opt(args, 0)
        .map(|v| v.to_int())
        .unwrap_or_else(|| h.cur_buf().point as i64)
        .max(1) as usize;
    super::mark_set(pos - 1);
    Ok(nil())
}

fn b_mark(_h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    let force = opt(args, 0).is_some();
    match super::mark_get() {
        Some(m) if force || super::mark_is_active() => Ok(Value::Int(m as i64 + 1)),
        _ => Ok(nil()),
    }
}

fn b_region_beginning(h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    let p = h.cur_buf().point as i64;
    match super::mark_get() {
        Some(m) => Ok(Value::Int(p.min(m as i64 + 1))),
        None => Err("The mark is not set now, so there is no region".to_string()),
    }
}

fn b_region_end(h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    let p = h.cur_buf().point as i64;
    match super::mark_get() {
        Some(m) => Ok(Value::Int(p.max(m as i64 + 1))),
        None => Err("The mark is not set now, so there is no region".to_string()),
    }
}

fn b_region_active_p(h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    Ok(if super::mark_is_active() { t(h) } else { nil() })
}

fn b_deactivate_mark(_h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    super::mark_deactivate();
    Ok(nil())
}

fn b_mark_whole_buffer(h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    let len = h.cur_buf().text.len();
    super::mark_set(0);
    h.cur_buf().point = len + 1;
    Ok(nil())
}

// ── kill ring & yank ──
//
// Text lives in the mirrored `EditBuffer` (`h.cur_buf().text`, a `Vec<char>`, and
// a 1-based `point`); the kill ring itself lives in `super`. A 1-based [beg,end)
// point pair is clamped to the buffer and returned as a 0-based char range.
fn clamp_range(h: &mut ElispHost, a: i64, b: i64) -> (usize, usize) {
    let n = h.cur_buf().text.len();
    let lo = (a.min(b).max(1) as usize - 1).min(n);
    let hi = (a.max(b).max(1) as usize - 1).min(n);
    (lo, hi)
}

fn b_kill_new(h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    let s = as_string(h, &args[0]);
    super::kill_push(s.clone());
    Ok(Value::str(s))
}

fn b_kill_region(h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    let (lo, hi) = clamp_range(h, args[0].to_int(), args[1].to_int());
    let text: String = h.cur_buf().text[lo..hi].iter().collect();
    // Delete through the host so `zv`/`begv`/markers stay in sync with the
    // shortened text (a raw `text.drain` leaves `point-max` past the end, which
    // later panics `buffer-string`). `cur_delete` takes 1-based `[from, to)`.
    h.cur_delete(lo + 1, hi + 1);
    h.cur_buf().point = lo + 1;
    super::kill_push(text);
    super::mark_deactivate();
    Ok(nil())
}

fn b_copy_region_as_kill(h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    let (lo, hi) = clamp_range(h, args[0].to_int(), args[1].to_int());
    let text: String = h.cur_buf().text[lo..hi].iter().collect();
    super::kill_push(text);
    super::mark_deactivate();
    Ok(nil())
}

fn b_yank(h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    let s = super::kill_current(0).unwrap_or_default();
    let chars: Vec<char> = s.chars().collect();
    let at = {
        let buf = h.cur_buf();
        buf.point.saturating_sub(1).min(buf.text.len())
    };
    // Insert through the host (1-based point) so `zv`/`begv`/markers advance with
    // the inserted text; `cur_insert(_, true)` leaves point after the insertion.
    h.cur_buf().point = at + 1;
    h.cur_insert(chars, true);
    // Emacs leaves point after the insertion and the mark (inactive) at its start.
    super::mark_set(at);
    super::mark_deactivate();
    Ok(nil())
}

fn b_current_kill(_h: &mut ElispHost, args: &[Value]) -> Result<Value, String> {
    match super::kill_current(args[0].to_int()) {
        Some(s) => Ok(Value::str(s)),
        None => Ok(nil()),
    }
}

/// End (0-based) of the line containing 0-based `p`: the next '\n', or buffer end.
fn line_end(text: &[char], p: usize) -> usize {
    let mut e = p;
    while e < text.len() && text[e] != '\n' {
        e += 1;
    }
    e
}

fn b_kill_line(h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    let buf = h.cur_buf();
    let p = buf.point.saturating_sub(1).min(buf.text.len());
    let eol = line_end(&buf.text, p);
    // At end-of-line, kill the newline itself; otherwise kill to end-of-line.
    let end = if eol == p && eol < buf.text.len() {
        eol + 1
    } else {
        eol
    };
    let text: String = buf.text[p..end].iter().collect();
    buf.text.drain(p..end);
    buf.point = p + 1;
    super::kill_push(text);
    Ok(nil())
}

fn b_kill_whole_line(h: &mut ElispHost, _args: &[Value]) -> Result<Value, String> {
    let buf = h.cur_buf();
    let p = buf.point.saturating_sub(1).min(buf.text.len());
    // Start of the current line: just past the previous newline.
    let mut bol = p;
    while bol > 0 && buf.text[bol - 1] != '\n' {
        bol -= 1;
    }
    let eol = line_end(&buf.text, p);
    let end = (eol + 1).min(buf.text.len()); // include the trailing newline
    let text: String = buf.text[bol..end].iter().collect();
    buf.text.drain(bol..end);
    buf.point = bol + 1;
    super::kill_push(text);
    Ok(nil())
}
