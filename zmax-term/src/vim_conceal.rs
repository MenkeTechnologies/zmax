//! vim `conceallevel` concealment. Computes the character indices of syntax
//! markers that should be hidden when rendering (currently Markdown emphasis,
//! inline code, links and strikethrough), turns them into empty-grapheme
//! `Overlay`s on the `Document`, and refreshes them on open/change. The render
//! (`View::text_annotations`) draws those overlays, hiding the markers.

use std::sync::OnceLock;

use regex::Regex;
use zmax_core::text_annotations::Overlay;
use zmax_view::{DocumentId, Editor};

/// Markdown conceal patterns: each captures the *visible* text in group 1; every
/// character of the match outside that group (the markers) is concealed.
fn markdown_patterns() -> &'static [Regex] {
    static PATS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATS.get_or_init(|| {
        [
            r"\*\*(.+?)\*\*",               // **bold**
            r"__(.+?)__",                   // __bold__
            r"~~(.+?)~~",                   // ~~strike~~
            r"`([^`\n]+?)`",                // `code`
            r"\[([^\]\n]+?)\]\([^)\n]*?\)", // [text](url)
        ]
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect()
    })
}

/// Character indices to conceal in Markdown `text`: the marker characters of
/// each matched span (everything in the match except the visible capture group).
pub fn markdown_conceal_char_indices(text: &str) -> Vec<usize> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut mask = vec![false; text.len()];
    for re in markdown_patterns() {
        for cap in re.captures_iter(text) {
            let (Some(m), Some(g)) = (cap.get(0), cap.get(1)) else {
                continue;
            };
            for b in m.start()..g.start() {
                mask[b] = true;
            }
            for b in g.end()..m.end() {
                mask[b] = true;
            }
        }
    }
    let mut out = Vec::new();
    for (char_idx, (byte_idx, _)) in text.char_indices().enumerate() {
        if mask[byte_idx] {
            out.push(char_idx);
        }
    }
    out
}

/// Most-recent `conceallevel`, cached so the `DocumentDidChange` hook (which only
/// receives the document, not the editor config) can recompute at the right level.
static CONCEAL_LEVEL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Recompute a document's conceal overlays for the current `conceallevel`.
pub fn recompute_conceal(editor: &mut Editor, doc_id: DocumentId) {
    let level = editor.config().conceallevel;
    CONCEAL_LEVEL.store(level, std::sync::atomic::Ordering::Relaxed);
    if let Some(doc) = editor.document_mut(doc_id) {
        recompute_conceal_doc(doc, level);
    }
}

/// Refresh conceal overlays when documents open or change, or `conceallevel`
/// changes.
pub fn register_hooks() {
    use zmax_event::register_hook;
    use zmax_view::events::{ConfigDidChange, DocumentDidChange, DocumentDidOpen};
    register_hook!(move |event: &mut DocumentDidOpen<'_>| {
        recompute_conceal(event.editor, event.doc);
        Ok(())
    });
    register_hook!(move |event: &mut DocumentDidChange<'_>| {
        let level = CONCEAL_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
        if level > 0 || !event.doc.conceal_overlays().is_empty() {
            recompute_conceal_doc(event.doc, level);
        }
        Ok(())
    });
    register_hook!(move |event: &mut ConfigDidChange<'_>| {
        let level = event.new.conceallevel;
        CONCEAL_LEVEL.store(level, std::sync::atomic::Ordering::Relaxed);
        let ids: Vec<DocumentId> = event.editor.documents().map(|d| d.id()).collect();
        for id in ids {
            if let Some(doc) = event.editor.document_mut(id) {
                recompute_conceal_doc(doc, level);
            }
        }
        Ok(())
    });
}

/// Recompute one document's conceal overlays at `level`.
fn recompute_conceal_doc(doc: &mut zmax_view::Document, level: usize) {
    let is_markdown = doc
        .language_name()
        .map(|l| matches!(l, "markdown" | "md"))
        .unwrap_or(false);
    if level == 0 || !is_markdown {
        if !doc.conceal_overlays().is_empty() {
            doc.set_conceal_overlays(Vec::new());
        }
        return;
    }
    let text: String = doc.text().slice(..).chars().collect();
    let overlays: Vec<Overlay> = markdown_conceal_char_indices(&text)
        .into_iter()
        .map(|idx| Overlay::new(idx, ""))
        .collect();
    doc.set_conceal_overlays(overlays);
}

#[cfg(test)]
mod test {
    use super::*;

    fn concealed(text: &str) -> String {
        let hide: std::collections::HashSet<usize> =
            markdown_conceal_char_indices(text).into_iter().collect();
        text.chars()
            .enumerate()
            .filter(|(i, _)| !hide.contains(i))
            .map(|(_, c)| c)
            .collect()
    }

    #[test]
    fn conceals_markdown_markers() {
        assert_eq!(concealed("a **bold** b"), "a bold b");
        assert_eq!(concealed("use `code` here"), "use code here");
        assert_eq!(concealed("a ~~no~~ b"), "a no b");
        assert_eq!(concealed("see [text](http://x) end"), "see text end");
        assert_eq!(concealed("plain text"), "plain text");
    }
}
