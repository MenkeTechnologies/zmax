//! DocView — the zmax port of GNU Emacs `doc-view-mode`, the PDF/PS/DVI/EPUB
//! viewer.
//!
//! The page itself is drawn straight to the terminal by
//! `commands::display_doc_page_in_terminal` (terminal graphics, not a
//! `Surface`), and the current page/resolution live in the `DOCVIEW` state that
//! the `doc-view-*` typable commands already read and write. This [`Component`]
//! exists to own the *keymap*: without it those commands were reachable only by
//! name (`:doc-view-next-page`), so none of Emacs's DocView keys worked. Every
//! key here dispatches into the same `docview_step` / `docview_zoom` helpers the
//! typables use, so the two paths cannot drift apart.
//!
//! Keys (parsed into a `docview` keymap mode by `scripts/gen_port_report.py`, so
//! each maps to its Emacs DocView counterpart in the port tracker). The bindings
//! follow the Emacs manual's DocView Navigation node verbatim:
//!   n / PageDown / next / C-x ] — next page (`doc-view-next-page`)
//!   p / PageUp / prior / C-x [  — previous page (`doc-view-previous-page`)
//!   SPC — scroll or advance (`doc-view-scroll-up-or-next-page`)
//!   DEL — scroll or retreat (`doc-view-scroll-down-or-previous-page`)
//!   M-< — first page (`doc-view-first-page`)
//!   M-> — last page (`doc-view-last-page`)
//!   `+` — enlarge (`doc-view-enlarge`)
//!   `-` — shrink (`doc-view-shrink`)
//!   q / Esc — leave the viewer
//!
//! `SPC`/`DEL` advance a whole page rather than scrolling within one: the page is
//! rendered as a single terminal image, so there is nothing to scroll inside it.
//! They therefore share the next/previous handlers.
//!
//! `c m` is `doc-view-set-slice-using-mouse`; see [`slice_pick_script`] for how
//! the two clicks are read when the page is on screen but zmax's TUI is not.
//! `c r` resets the slice, matching Emacs's `c`-prefixed slice map.

use std::path::{Path, PathBuf};

use tui::buffer::Buffer as Surface;
use zmax_core::command_line::Args;
use zmax_view::graphics::Rect;
use zmax_view::input::KeyEvent;
use zmax_view::keyboard::KeyCode;

use crate::commands::typed::{docview_step, docview_zoom, DocPage};
use crate::ui::PromptEvent;
use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// One `+`/`-` press, matching `doc-view-enlarge` / `doc-view-shrink`.
const ZOOM_STEP: i32 = 25;

/// The viewer overlay. Holds no page state of its own — `DOCVIEW` is the single
/// source of truth, so a `:doc-view-goto-page` typed while the overlay is up
/// stays in sync.
#[derive(Default)]
pub struct DocView {
    /// `C-x` was typed and the next key decides whether it is `C-x [` or `C-x ]`.
    pending_ctrl_x: bool,
    /// `c` was typed and the next key names a slice command (`c m` / `c r`).
    pending_c: bool,
    /// `C-c` was typed and the next key names `C-c C-c` or `C-c C-t`.
    pending_ctrl_c: bool,
}

impl DocView {
    pub fn new() -> Self {
        Self::default()
    }

    /// `C-x` then a key: Emacs's `C-x [` / `C-x ]` page pair. Kept in its own fn
    /// so the chords read as the two-key sequences they are.
    fn dispatch_ctrl_x_key(&mut self, cx: &mut Context, key: KeyEvent) -> anyhow::Result<()> {
        match key {
            key!('[') => docview_step(cx, DocPage::Prev),
            key!(']') => docview_step(cx, DocPage::Next),
            _ => Ok(()),
        }
    }

    /// `c` then a key: Emacs's slice map (`c m` pick with the mouse, `c r`
    /// reset). Matched on the raw [`KeyCode`] rather than the `key!` macro so the
    /// port report's component-keymap scanner does not read `m`/`r` as unprefixed
    /// DocView chords — they are only reachable behind `c`.
    fn dispatch_c_key(&mut self, cx: &mut Context, key: KeyEvent) -> anyhow::Result<()> {
        match key.code {
            KeyCode::Char('m') => set_slice_using_mouse(cx),
            KeyCode::Char('r') => crate::commands::typed::docview_reset_slice(cx),
            _ => Ok(()),
        }
    }

    /// `C-c` then a key: `C-c C-c` is `doc-view-toggle-display` — leave the
    /// rendered page and show the document's own buffer text again (`:doc-view`
    /// renders it back) — and `C-c C-t` is `doc-view-open-text`, the text in a
    /// buffer of its own. Returns whether the overlay should close, since only
    /// the toggle does. Matched on the raw [`KeyCode`] for the same reason
    /// [`Self::dispatch_c_key`] is: these are only reachable behind `C-c`.
    fn dispatch_ctrl_c_key(
        &mut self,
        cx: &mut Context,
        key: KeyEvent,
    ) -> (bool, anyhow::Result<()>) {
        let ctrl = key
            .modifiers
            .contains(zmax_view::keyboard::KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => {
                cx.editor.set_status("doc-view: text display");
                (true, Ok(()))
            }
            KeyCode::Char('t') if ctrl => (false, crate::commands::typed::docview_open_text(cx)),
            _ => (false, Ok(())),
        }
    }
}

impl Component for DocView {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        // `C-x [` / `C-x ]` are two-key chords; a Component has no keymap trie, so
        // the prefix is tracked by hand as the prompt does it.
        if std::mem::take(&mut self.pending_ctrl_x) {
            let stepped = self.dispatch_ctrl_x_key(cx, key);
            report(cx, stepped);
            return EventResult::Consumed(None);
        }
        if std::mem::take(&mut self.pending_c) {
            let sliced = self.dispatch_c_key(cx, key);
            report(cx, sliced);
            return EventResult::Consumed(None);
        }
        if std::mem::take(&mut self.pending_ctrl_c) {
            let (leave, done) = self.dispatch_ctrl_c_key(cx, key);
            report(cx, done);
            if leave {
                return EventResult::Consumed(Some(close));
            }
            return EventResult::Consumed(None);
        }

        let done = match key {
            key!('q') | key!(Esc) => return EventResult::Consumed(Some(close)),
            ctrl!('x') => {
                self.pending_ctrl_x = true;
                Ok(())
            }
            ctrl!('c') => {
                self.pending_ctrl_c = true;
                Ok(())
            }
            key!('c') => {
                self.pending_c = true;
                Ok(())
            }
            // Next page. SPC is `doc-view-scroll-up-or-next-page`: the page is one
            // image, so there is nothing to scroll within and it advances.
            key!('n') | key!(PageDown) | key!(' ') => docview_step(cx, DocPage::Next),
            // Previous page; DEL is `doc-view-scroll-down-or-previous-page`.
            key!('p') | key!(PageUp) | key!(Backspace) => docview_step(cx, DocPage::Prev),
            alt!('<') => docview_step(cx, DocPage::First),
            alt!('>') => docview_step(cx, DocPage::Last),
            key!('+') => docview_zoom(cx, ZOOM_STEP),
            key!('-') => docview_zoom(cx, -ZOOM_STEP),
            _ => return EventResult::Ignored(None),
        };
        report(cx, done);
        EventResult::Consumed(None)
    }

    /// The page is painted straight to the terminal by the step/zoom commands, so
    /// there is nothing to draw onto the `Surface` — and clearing it would erase
    /// the image the terminal is already holding.
    fn render(&mut self, _area: Rect, _surface: &mut Surface, _ctx: &mut Context) {}

    fn id(&self) -> Option<&'static str> {
        Some("docview")
    }
}

/// The helpers fail when the buffer stops being a document; say so on the status
/// line rather than dropping it, which is what the typable path does.
fn report(cx: &mut Context, result: anyhow::Result<()>) {
    if let Err(e) = result {
        cx.editor.set_error(e.to_string());
    }
}

// ---------------------------------------------------------------------------
// doc-view-set-slice-using-mouse
// ---------------------------------------------------------------------------

/// The file the picking script hands the chosen slice back through.
///
/// The page is drawn by an external viewer while zmax's terminal is released
/// (`Editor::pending_tty_command`), so zmax is not reading input at the moment
/// the clicks happen and cannot receive them as `Event::Mouse`. The script
/// therefore reads the two presses itself — SGR mouse reporting, raw mode — and
/// leaves `X Y W H` here for [`take_picked_slice`] to fold into the doc-view
/// slice state.
pub fn slice_pick_file() -> PathBuf {
    std::env::temp_dir().join(format!("zmax-docview-slice-{}", std::process::id()))
}

/// Read and consume the slice the picking script left behind, if there is one.
pub fn take_picked_slice() -> Option<(u32, u32, u32, u32)> {
    let path = slice_pick_file();
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let n: Vec<u32> = text
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    match n.as_slice() {
        [x, y, w, h] if *w > 0 && *h > 0 => Some((*x, *y, *w, *h)),
        _ => None,
    }
}

/// The picking script, with the page render, the viewer chain and the handback
/// file substituted in.
///
/// `chafa --stretch --size=COLSxROWS` maps the page onto the cell grid exactly
/// (aspect deliberately ignored for this pass, so a clicked cell corresponds to
/// a known pixel box), the two presses are decoded from SGR mouse reports, and
/// the resulting region is written out, cropped and redisplayed — so the slice
/// is visible immediately as well as remembered.
const SLICE_PICK: &str = r#"
exec </dev/tty
[ -s "$i" ] || { echo 'doc-view: could not render the page'; exit 1; }
command -v chafa >/dev/null 2>&1 || {
  echo 'doc-view-set-slice-using-mouse needs chafa (it is the viewer whose output size is exact)'
  rm -f "$i"; exit 1; }
size=$(magick identify -format '%w %h' "$i" 2>/dev/null || identify -format '%w %h' "$i" 2>/dev/null)
iw=${size%% *}; ih=${size##* }
[ -n "$iw" ] && [ -n "$ih" ] || {
  echo 'doc-view-set-slice-using-mouse needs ImageMagick identify to map clicks to page pixels'
  rm -f "$i"; exit 1; }
set -- $(stty size)
rows=$(($1 - 2)); cols=$2
printf '\033[2J\033[H'
chafa --stretch --size=${cols}x${rows} "$i"
printf 'Press mouse-1 at the top-left corner and again at the bottom-right corner (q aborts).'
printf '\033[?1000h\033[?1006h'
stty raw -echo
readclick() {
  st=0; seq=''
  while :; do
    c=$(dd bs=1 count=1 2>/dev/null)
    if [ "$c" = '<' ]; then st=1; seq=''; continue; fi
    if [ "$st" = 0 ]; then
      [ "$c" = q ] && return 1
      continue
    fi
    case "$c" in
      M) printf '%s' "$seq"; return 0 ;;
      m) st=0; seq='' ;;
      *) seq="$seq$c" ;;
    esac
  done
}
c1=$(readclick) && c2=$(readclick)
rc=$?
stty sane
printf '\033[?1006l\033[?1000l'
[ "$rc" = 0 ] || { echo; echo 'doc-view-set-slice-using-mouse: aborted'; rm -f "$i"; exit 0; }
r=${c1#*;}; x1=${r%%;*}; y1=${r#*;}
r=${c2#*;}; x2=${r%%;*}; y2=${r#*;}
px1=$(( (x1 - 1) * iw / cols )); py1=$(( (y1 - 1) * ih / rows ))
px2=$(( x2 * iw / cols ));       py2=$(( y2 * ih / rows ))
if [ "$px1" -gt "$px2" ]; then t=$px1; px1=$px2; px2=$t; fi
if [ "$py1" -gt "$py2" ]; then t=$py1; py1=$py2; py2=$t; fi
w=$((px2 - px1)); h=$((py2 - py1))
if [ "$w" -lt 1 ] || [ "$h" -lt 1 ]; then
  echo; echo 'doc-view-set-slice-using-mouse: the two clicks name an empty region'
  rm -f "$i"; exit 0
fi
printf '%s %s %s %s\n' "$px1" "$py1" "$w" "$h" > @OUT@
if command -v magick >/dev/null 2>&1; then
  magick "$i" -crop ${w}x${h}+${px1}+${py1} +repage "$i"
elif command -v convert >/dev/null 2>&1; then
  convert "$i" -crop ${w}x${h}+${px1}+${py1} +repage "$i"
fi
printf '\033[2J\033[H'
{ @VIEWERS@; } 2>/dev/null
printf '\n-- slice %sx%s+%s+%s  (Enter) --' "$w" "$h" "$px1" "$py1"
read -r _ </dev/tty
rm -f "$i"
"#;

/// Build the full picking script for page `page` of `doc` at `dpi`.
pub fn slice_pick_script(doc: &Path, page: u32, dpi: u32) -> String {
    let render = crate::commands::doc_page_render_script(doc, page, dpi);
    let body = SLICE_PICK
        .replace(
            "@OUT@",
            &crate::commands::img_shell_quote(&slice_pick_file().to_string_lossy()),
        )
        .replace("@VIEWERS@", crate::commands::IMG_VIEWER_CHAIN);
    format!("{render}{body}")
}

/// emacs `doc-view-set-slice-using-mouse` (`c m`): pick the slice by pressing
/// mouse-1 at its top-left corner and again at its bottom-right corner.
pub fn set_slice_using_mouse(cx: &mut Context) -> anyhow::Result<()> {
    crate::commands::typed::docview_pick_slice(cx)
}

/// `:doc-view-set-slice-using-mouse` — the typable behind `c m`.
pub fn ex_set_slice_using_mouse(
    cx: &mut Context,
    _args: Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    set_slice_using_mouse(cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_script_carries_the_render_and_the_handback_file() {
        let s = slice_pick_script(Path::new("/tmp/a b.pdf"), 3, 150);
        // The page render comes first and leaves the PNG in `$i`.
        assert!(s.contains("page=3"));
        assert!(s.contains("dpi=150"));
        assert!(s.contains("'/tmp/a b.pdf'"));
        // Mouse reporting is turned on and back off again.
        assert!(s.contains("\\033[?1000h"));
        assert!(s.contains("\\033[?1006l"));
        // The chosen region is written where `take_picked_slice` looks for it.
        assert!(s.contains(&slice_pick_file().display().to_string()));
        assert!(!s.contains("@OUT@") && !s.contains("@VIEWERS@"));
    }

    #[test]
    fn picked_slice_round_trips_and_rejects_empty_regions() {
        let path = slice_pick_file();
        std::fs::write(&path, "10 20 300 400\n").unwrap();
        assert_eq!(take_picked_slice(), Some((10, 20, 300, 400)));
        // Consumed: a second read finds nothing.
        assert_eq!(take_picked_slice(), None);
        std::fs::write(&path, "10 20 0 400\n").unwrap();
        assert_eq!(take_picked_slice(), None);
        let _ = std::fs::remove_file(&path);
    }
}
