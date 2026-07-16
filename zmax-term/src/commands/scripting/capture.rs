//! Host-side stdout/stderr capture for interpreters that write the real process
//! fds (embedded zsh, perl/stryke) instead of an in-process buffer.
//!
//! Redirects fds 0/1/2 to a temp file (+ `/dev/null` for stdin) around `f`,
//! restores them, and returns whatever was written. A temp file (not a pipe)
//! avoids the 64 KiB pipe-buffer deadlock when output is large. Eval runs
//! synchronously on the editor thread, so the redirect window is brief and the
//! TUI's fds are restored before the next render.

/// Serializes the (process-global) fd redirect/restore window so two concurrent
/// captures can't clobber each other's fds. Eval is single-threaded on the
/// editor thread at runtime; this matters mainly for parallel tests.
#[cfg(unix)]
static CAPTURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Run `f` with stdout/stderr redirected to a temp file; return its result and
/// the captured output.
#[cfg(unix)]
pub(super) fn with_captured_fds<R>(f: impl FnOnce() -> R) -> Result<(R, String), String> {
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::unix::io::AsRawFd;

    // Hold the process-wide capture lock for the entire redirect window.
    let _capture_guard = CAPTURE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();

    let mut tmp = tempfile::tempfile().map_err(|e| format!("tempfile: {e}"))?;
    let devnull = std::fs::File::open("/dev/null").map_err(|e| format!("/dev/null: {e}"))?;
    let tmpfd = tmp.as_raw_fd();
    let nullfd = devnull.as_raw_fd();

    // Save the real fds so we can restore them no matter how `f` returns.
    let (saved_in, saved_out, saved_err) = unsafe { (libc::dup(0), libc::dup(1), libc::dup(2)) };
    if saved_in < 0 || saved_out < 0 || saved_err < 0 {
        return Err("failed to save stdio fds".into());
    }
    unsafe {
        libc::dup2(nullfd, 0);
        libc::dup2(tmpfd, 1);
        libc::dup2(tmpfd, 2);
    }

    let result = f();

    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    unsafe {
        libc::dup2(saved_in, 0);
        libc::dup2(saved_out, 1);
        libc::dup2(saved_err, 2);
        libc::close(saved_in);
        libc::close(saved_out);
        libc::close(saved_err);
    }

    let mut buf = Vec::new();
    tmp.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    tmp.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok((result, String::from_utf8_lossy(&buf).into_owned()))
}

#[cfg(not(unix))]
pub(super) fn with_captured_fds<R>(f: impl FnOnce() -> R) -> Result<(R, String), String> {
    Ok((f(), String::new()))
}
