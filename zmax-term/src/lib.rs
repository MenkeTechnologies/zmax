// Internal docs deliberately cross-link private items (rendered with
// `--document-private-items` in CI). These links are intentional and would only
// be "broken" in a public-docs build this binary crate never produces. Genuinely
// unresolved links (`broken_intra_doc_links`) remain denied.
#![allow(rustdoc::private_intra_doc_links)]
// The bundled games (`ui/*`) are grid-based and index their boards by row/column
// on purpose; `for i in 0..N { board[i] }` reads clearer there than an iterator
// adapter, so the range-loop style lint is allowed crate-wide.
#![allow(clippy::needless_range_loop)]

#[macro_use]
extern crate zmax_view;

pub mod ai;
pub mod appdata;
pub mod application;
pub mod args;
pub mod blame;
pub mod ci;
pub mod closed_files;
pub mod commands;
pub mod compositor;
pub mod config;
pub mod emacs_abbrev;
pub mod emacs_bookmark;
pub mod emacs_kill;
pub mod emacs_mark;
pub mod emacs_rect;
pub mod emacs_register;
pub mod embedded;
pub mod emmet;
pub mod events;
pub mod eww;
pub mod file_watcher;
pub mod harpoon;
pub mod irc;
pub mod health;
pub mod hi_lock;
pub mod job;
pub mod keymap;
pub mod local_history;
pub mod logging;
pub mod recent_files;
pub mod run_config;
pub mod snippet_store;
pub mod translate;
pub mod spell;
pub mod ui;
pub mod vim_autocmd;
pub mod vim_conceal;
pub mod vim_fold;
pub mod vim_modeline;
pub mod vim_regex;
pub mod vim_statusline;
pub mod vim_swap;
pub mod vim_undo;
pub mod zmaxinfo;
pub mod zwire;

#[cfg(not(windows))]
use std::env::var_os;

use std::path::Path;

use futures_util::Future;
mod handlers;

use ignore::DirEntry;
use zmax_stdx::Url;

#[cfg(windows)]
fn true_color() -> bool {
    true
}

#[cfg(not(windows))]
fn true_color() -> bool {
    if var_os("COLORTERM").is_some_and(|v| v == "truecolor" || v == "24bit")
        || var_os("WSL_DISTRO_NAME").is_some()
    {
        return true;
    }

    match termini::TermInfo::from_env() {
        Ok(t) => {
            t.extended_cap("RGB").is_some()
                || t.extended_cap("Tc").is_some()
                || (t.extended_cap("setrgbf").is_some() && t.extended_cap("setrgbb").is_some())
        }
        Err(_) => false,
    }
}

/// Heuristic "is this a binary (non-text) file?" check over a leading chunk of a
/// file. Replaces the `content_inspector` crate — we only need the binary/text
/// verdict, not its encoding classification.
///
/// A leading byte-order mark marks the content as text (UTF-16/32 text
/// legitimately contains NUL bytes, so it must be excluded before the NUL scan);
/// otherwise a NUL byte in the first kilobyte — or a known binary magic number —
/// means binary.
pub(crate) fn is_binary(buffer: &[u8]) -> bool {
    // UTF-32 BOMs must be checked before UTF-16 (their BOMs overlap).
    const BYTE_ORDER_MARKS: &[&[u8]] = &[
        &[0xEF, 0xBB, 0xBF],       // UTF-8
        &[0x00, 0x00, 0xFE, 0xFF], // UTF-32BE
        &[0xFF, 0xFE, 0x00, 0x00], // UTF-32LE
        &[0xFE, 0xFF],             // UTF-16BE
        &[0xFF, 0xFE],             // UTF-16LE
    ];

    if BYTE_ORDER_MARKS.iter().any(|bom| buffer.starts_with(bom)) {
        return false;
    }

    let scan = &buffer[..buffer.len().min(1024)];
    scan.contains(&0) || buffer.starts_with(b"%PDF") || buffer.starts_with(b"\x89PNG")
}

/// Function used for filtering dir entries in the various file pickers.
fn filter_picker_entry(entry: &DirEntry, root: &Path, dedup_symlinks: bool) -> bool {
    // We always want to ignore popular VCS directories, otherwise if
    // `ignore` is turned off, we end up with a lot of noise
    // in our picker.
    if matches!(
        entry.file_name().to_str(),
        Some(".git" | ".pijul" | ".jj" | ".hg" | ".svn")
    ) {
        return false;
    }

    // We also ignore symlinks that point inside the current directory
    // if `dedup_links` is enabled.
    if dedup_symlinks && entry.path_is_symlink() {
        return entry
            .path()
            .canonicalize()
            .ok()
            .is_some_and(|path| !path.starts_with(root));
    }

    true
}

/// Opens URL in external program.
fn open_external_url_callback(
    url: Url,
) -> impl Future<Output = Result<job::Callback, anyhow::Error>> + Send + 'static {
    let commands = open::commands(url.as_str());
    async {
        for cmd in commands {
            let mut command: tokio::process::Command = cmd.into();
            if command.status().await.is_ok() {
                return Ok(job::Callback::Editor(Box::new(|_| {})));
            }
        }
        Ok(job::Callback::Editor(Box::new(move |editor| {
            editor.set_error("Opening URL in external program failed")
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::is_binary;

    #[test]
    fn binary_detection() {
        assert!(!is_binary(b""));
        assert!(!is_binary(b"plain text\nsecond line"));
        // a NUL byte in the scanned range -> binary
        assert!(is_binary(b"text\0with nul"));
        // binary magic numbers with no NUL prefix
        assert!(is_binary(b"%PDF-1.7 ..."));
        assert!(is_binary(b"\x89PNG\r\n"));
        // a BOM marks the content as text even though it carries NUL bytes
        assert!(!is_binary(b"\xFF\xFEt\0e\0x\0t\0")); // UTF-16LE
        assert!(!is_binary(b"\x00\x00\xFE\xFFtext")); // UTF-32BE
        assert!(!is_binary(b"\xEF\xBB\xBFtext")); // UTF-8 BOM
    }
}
