//! vim `:mksession` — a port of nvim's `ex_session.c` (`ex_mkrc`, `makeopens`,
//! `ses_win_rec`, `ses_winsizes`, `put_view`, `ses_arglist`) and of the fold
//! writers it calls (`put_folds` in fold.c, `makefoldset` in option.c).
//!
//! The file it writes is the one nvim writes for the same layout, so a session
//! saved by zmax loads in vim and nvim, and theirs load in zmax. The editor is
//! read in two steps: [`snapshot`] visits each tab page and records what the
//! writer needs, then [`write`] produces the text from that record alone.

use std::path::{Path, PathBuf};

use zmax_view::tree::{Layout, TreeShape};
use zmax_view::{Editor, ViewId};

/// The `'sessionoptions'` flags the writer honours.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionOptions {
    pub blank: bool,
    pub buffers: bool,
    pub curdir: bool,
    pub sesdir: bool,
    pub folds: bool,
    pub help: bool,
    pub tabpages: bool,
    pub winsize: bool,
    pub terminal: bool,
}

impl SessionOptions {
    /// Parse a `'sessionoptions'` value (comma-separated flags).
    pub fn parse(value: &str) -> Self {
        let has = |flag: &str| value.split(',').any(|f| f.trim() == flag);
        SessionOptions {
            blank: has("blank"),
            buffers: has("buffers"),
            curdir: has("curdir"),
            sesdir: has("sesdir"),
            folds: has("folds"),
            help: has("help"),
            tabpages: has("tabpages"),
            winsize: has("winsize"),
            terminal: has("terminal"),
        }
    }
}

/// nvim's default `'sessionoptions'`.
pub const DEFAULT_SESSIONOPTIONS: &str = "blank,buffers,curdir,folds,help,tabpages,winsize,terminal";

/// A fold as `put_folds_recurse` walks it: its lines (1-based), whether it is
/// closed, and the folds nested in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldNode {
    pub start: usize,
    pub end: usize,
    pub closed: bool,
    pub nested: Vec<FoldNode>,
}

/// One window, as `put_view` and `ses_winsizes` read it.
#[derive(Debug, Clone)]
pub struct WinSnap {
    /// The buffer's file, `None` for a buffer with no file.
    pub path: Option<PathBuf>,
    /// The alternate buffer's file (`balt`).
    pub alt: Option<PathBuf>,
    /// Cursor line (1-based), and its screen column (0-based, `w_virtcol`).
    pub lnum: usize,
    pub col: usize,
    pub virtcol: usize,
    /// First line shown (1-based) and the text height (`w_view_height`).
    pub topline: usize,
    pub view_height: usize,
    /// `w_height` (text rows) and `w_width`, and whether the window spans the
    /// whole height / width of the tab page.
    pub height: usize,
    pub width: usize,
    pub full_height: bool,
    pub full_width: bool,
    /// Horizontal scroll and whether lines wrap (`w_leftcol`, `'wrap'`).
    pub leftcol: usize,
    pub wrap: bool,
    pub localdir: Option<PathBuf>,
    /// `makefoldset`: the window's fold options as `:setlocal` writes them.
    pub fold_opts: Vec<(&'static str, String)>,
    pub foldmethod_manual: bool,
    pub foldlevel: usize,
    pub folds: Vec<FoldNode>,
}

/// One tab page.
#[derive(Debug, Clone)]
pub struct TabSnap {
    pub shape: TreeShape,
    /// The windows in `shape`'s leaf order (vim's window order).
    pub windows: Vec<WinSnap>,
    /// The current window's number (1-based).
    pub curwin: usize,
    pub localdir: Option<PathBuf>,
}

/// Everything [`write`] reads.
#[derive(Debug, Clone)]
pub struct SessionSnap {
    pub tabs: Vec<TabSnap>,
    /// The current tab page's number (1-based).
    pub curtab: usize,
    /// `globaldir`, else the working directory.
    pub cwd: PathBuf,
    /// The listed buffers with a file, in buffer order, with the line `badd`
    /// enters each on.
    pub buffers: Vec<(PathBuf, usize)>,
    /// The global argument list and its index (`w_arg_idx`).
    pub args: Vec<String>,
    pub arg_idx: usize,
    /// `Rows` and `Columns`: the screen the sizes are fractions of.
    pub rows: usize,
    pub columns: usize,
    /// Options the header and footer restore.
    pub hlsearch: bool,
    pub showtabline: usize,
    pub winheight: usize,
    pub winwidth: usize,
}

/// The values the snapshot needs from zmax's option store.
pub struct OptionReader<'a> {
    /// A buffer's effective value of an option (its `:setlocal` value, else
    /// the global one, else the default), by full name.
    pub buffer_opt: &'a dyn Fn(&zmax_view::Document, &str) -> String,
    /// An option's global value (else its default), by full name.
    pub global_opt: &'a dyn Fn(&str) -> String,
}

/// Record the editor's tab pages and windows. Each tab page is visited in turn
/// (as vim reads `tp_firstwin`/`tp_topframe`) and the current one is restored.
pub fn snapshot(editor: &mut Editor, opts: &OptionReader, args: (Vec<String>, usize)) -> SessionSnap {
    let tab_count = editor.tabs.len().max(1);
    let curtab = editor.current_tab;
    let mut tabs = Vec::with_capacity(tab_count);
    for t in 0..tab_count {
        if tab_count > 1 {
            editor.switch_tab(t);
        }
        tabs.push(snapshot_tab(editor, opts));
    }
    if tab_count > 1 {
        editor.switch_tab(curtab);
    }

    let area = editor.tree.area();
    let mut buffers = Vec::new();
    for id in &editor.buffer_order {
        let Some(doc) = editor.document(*id) else { continue };
        let Some(path) = doc.path() else { continue };
        let text = doc.text().slice(..);
        let lnum = doc
            .selections()
            .values()
            .next()
            .map_or(1, |sel| text.char_to_line(sel.primary().cursor(text)) + 1);
        buffers.push((path.to_path_buf(), lnum));
    }
    SessionSnap {
        tabs,
        curtab: curtab + 1,
        cwd: editor.global_cwd(),
        buffers,
        args: args.0,
        arg_idx: args.1,
        // `&lines` counts the command line below the windows.
        rows: usize::from(area.height) + 1,
        columns: usize::from(area.width),
        hlsearch: (opts.global_opt)("hlsearch") == "on",
        showtabline: (opts.global_opt)("showtabline").parse().unwrap_or(1),
        winheight: (opts.global_opt)("winheight").parse().unwrap_or(1),
        winwidth: (opts.global_opt)("winwidth").parse().unwrap_or(20),
    }
}

/// vim `check_arg_idx`: a window's argument index is invalid when the list has
/// more than one entry and the window is not editing the current one.
fn arg_idx_invalid(snap: &SessionSnap, path: Option<&Path>) -> bool {
    if snap.args.len() <= 1 {
        return false;
    }
    let Some(arg) = snap.args.get(snap.arg_idx) else {
        return true;
    };
    let arg = zmax_stdx::path::expand_tilde(Path::new(arg));
    let arg = if arg.is_absolute() { arg.into_owned() } else { snap.cwd.join(arg) };
    path.is_none_or(|p| zmax_stdx::path::canonicalize(arg) != p)
}

/// The frame a tab page's layout starts from: zmax roots every tree in a
/// container, which vim's `topframe` does not have when it holds one child.
fn frame(shape: &TreeShape) -> &TreeShape {
    match shape {
        TreeShape::Split { children, .. } if children.len() == 1 => frame(&children[0].1),
        other => other,
    }
}

fn snapshot_tab(editor: &Editor, opts: &OptionReader) -> TabSnap {
    let area = editor.tree.area();
    let ids: Vec<ViewId> = editor.tree.traverse().map(|(id, _)| id).collect();
    let curwin = ids.iter().position(|id| *id == editor.tree.focus).map_or(1, |n| n + 1);
    let windows = ids
        .iter()
        .map(|&id| snapshot_win(editor, id, area, opts))
        .collect();
    TabSnap {
        shape: editor.tree.shape(),
        windows,
        curwin,
        localdir: editor.tab_localdir.clone(),
    }
}

fn snapshot_win(
    editor: &Editor,
    id: ViewId,
    area: zmax_view::graphics::Rect,
    opts: &OptionReader,
) -> WinSnap {
    let view = editor.tree.get(id);
    let doc = editor.document(view.doc).expect("a window's document");
    let text = doc.text().slice(..);
    let cursor = doc.selection(id).primary().cursor(text);
    let line = text.char_to_line(cursor);
    let col = cursor - text.line_to_char(line);
    let virtcol = virtcol(&text.line(line).to_string(), col, doc.tab_width());
    let offset = doc.view_offset(id);
    let topline = text.char_to_line(offset.anchor.min(text.len_chars())) + 1;
    let alt = view
        .docs_access_history
        .iter()
        .rev()
        .find(|d| **d != view.doc)
        .and_then(|d| editor.document(*d))
        .and_then(|d| d.path().map(Path::to_path_buf));
    let opt = |name: &str| (opts.buffer_opt)(doc, name);
    let foldmethod = opt("foldmethod");
    let fold_opts = vec![
        ("foldmethod", foldmethod.clone()),
        ("foldexpr", opt("foldexpr")),
        ("foldmarker", opt("foldmarker")),
        ("foldignore", opt("foldignore")),
        ("foldlevel", doc.folds().level().to_string()),
        ("foldminlines", opt("foldminlines")),
        ("foldnestmax", opt("foldnestmax")),
        ("foldenable", opt("foldenable")),
    ];
    let viewport = view.inner_area(doc);
    WinSnap {
        path: doc.path().map(Path::to_path_buf),
        alt,
        lnum: line + 1,
        col,
        virtcol,
        topline,
        view_height: view.inner_height(),
        height: view.inner_height(),
        width: usize::from(view.area.width),
        full_height: editor.tree.node_height(id) >= area.height,
        full_width: view.area.width >= area.width,
        leftcol: offset.horizontal_offset,
        wrap: doc.text_format(viewport.width, None, Some(id)).soft_wrap,
        localdir: view.localdir.clone(),
        fold_opts,
        foldmethod_manual: foldmethod == "manual",
        foldlevel: doc.folds().level(),
        folds: fold_tree(doc.folds().iter().map(|f| (f.start + 1, f.end + 1, f.closed))),
    }
}

/// vim `w_virtcol`: the screen column (0-based) of character `col` in `line`,
/// tabs expanded to `tab_width` stops and wide characters counted wide.
fn virtcol(line: &str, col: usize, tab_width: usize) -> usize {
    let mut vcol = 0;
    for c in line.chars().take(col) {
        vcol += if c == '\t' {
            tab_width - vcol % tab_width.max(1)
        } else {
            zmax_core::graphemes::grapheme_width(c.encode_utf8(&mut [0; 4]))
        };
    }
    vcol
}

/// Nest a flat, start-ordered fold list by containment (zmax keeps folds flat;
/// vim's `w_folds` is a tree).
pub fn fold_tree(folds: impl Iterator<Item = (usize, usize, bool)>) -> Vec<FoldNode> {
    let mut flat: Vec<(usize, usize, bool)> = folds.collect();
    flat.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    fn build(items: &[(usize, usize, bool)], i: &mut usize, end: usize) -> Vec<FoldNode> {
        let mut out = Vec::new();
        while *i < items.len() && items[*i].1 <= end {
            let (start, stop, closed) = items[*i];
            *i += 1;
            let nested = build(items, i, stop);
            out.push(FoldNode {
                start,
                end: stop,
                closed,
                nested,
            });
        }
        out
    }
    let mut i = 0;
    build(&flat, &mut i, usize::MAX)
}

/// vim `vim_strsave_fnameescape(…, VSE_NONE)`: backslash the characters
/// special on an Ex command line, and a leading `+`, `>` or a lone `-`.
fn fnameescape(name: &str) -> String {
    const SPECIAL: &str = " \t\n*?[{`$\\%#'\"|!<";
    let mut out = String::with_capacity(name.len());
    if name.starts_with('+') || name.starts_with('>') || name == "-" {
        out.push('\\');
    }
    for c in name.chars() {
        if SPECIAL.contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// vim `ses_escape_fname`: `~` for the home directory, then escaped.
fn escape_fname(path: &Path) -> String {
    fnameescape(&zmax_stdx::path::fold_home_dir(path).to_string_lossy())
}

/// The writer's running state (`did_lcd`, and the directory short names are
/// relative to).
struct Writer {
    out: String,
    ssop: SessionOptions,
    did_lcd: bool,
    base: Option<PathBuf>,
}

impl Writer {
    fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    /// vim `ses_get_fname`: the short name while the directory the session
    /// restores is known (`curdir`/`sesdir`, no `:lcd` written yet), else the
    /// full one.
    fn fname(&self, path: &Path) -> String {
        if let Some(base) = self.base.as_ref().filter(|_| !self.did_lcd) {
            if let Ok(rel) = path.strip_prefix(base) {
                return fnameescape(&rel.to_string_lossy());
            }
        }
        escape_fname(path)
    }

    fn do_win(&self, win: &WinSnap) -> bool {
        win.path.is_some() || self.ssop.blank
    }
}

/// vim `ses_win_rec`: commands that split the first window into `shape`.
/// Afterwards the last window of the frame is current.
fn ses_win_rec(w: &mut Writer, shape: &TreeShape) {
    let TreeShape::Split { layout, children } = frame(shape) else {
        return;
    };
    let split = match layout {
        Layout::Horizontal => "split",
        Layout::Vertical => "vsplit",
    };
    let count = children.len() - 1;
    for _ in 0..count {
        w.line("wincmd _ | wincmd |");
        w.line(split);
    }
    if count > 0 {
        let dir = if *layout == Layout::Horizontal { 'k' } else { 'h' };
        w.line(&format!("{count}wincmd {dir}"));
    }
    for (i, (_, child)) in children.iter().enumerate() {
        ses_win_rec(w, child);
        if i + 1 < children.len() {
            w.line("wincmd w");
        }
    }
}

/// vim `ses_winsizes`.
fn ses_winsizes(w: &mut Writer, snap: &SessionSnap, tab: &TabSnap) {
    if !w.ssop.winsize {
        w.line("wincmd =");
        return;
    }
    let mut n = 0;
    for win in &tab.windows {
        if !w.do_win(win) {
            continue;
        }
        n += 1;
        if !win.full_height {
            w.line(&format!(
                "exe '{n}resize ' . ((&lines * {} + {}) / {})",
                win.height,
                snap.rows / 2,
                snap.rows
            ));
        }
        if !win.full_width {
            w.line(&format!(
                "exe 'vert {n}resize ' . ((&columns * {} + {}) / {})",
                win.width,
                snap.columns / 2,
                snap.columns
            ));
        }
    }
}

/// vim `put_folds` (manual folds and the open/closed state of each).
fn put_folds(w: &mut Writer, win: &WinSnap) {
    if win.foldmethod_manual {
        w.line("silent! normal! zE");
        put_folds_recurse(w, &win.folds);
        w.line("let &fdl = &fdl");
    }
    put_foldopen_recurse(w, win, &win.folds, 1);
}

fn put_folds_recurse(w: &mut Writer, folds: &[FoldNode]) {
    for fold in folds {
        put_folds_recurse(w, &fold.nested);
        w.line(&format!("sil! {},{}fold", fold.start, fold.end));
    }
}

/// vim `put_foldopen_recurse`: open or close a fold whose state is not the
/// one 'foldlevel' gives it (zmax does not tell a fold set by hand from one set
/// by level, so every fold is written as vim writes a hand-set one).
fn put_foldopen_recurse(w: &mut Writer, win: &WinSnap, folds: &[FoldNode], level: usize) {
    for fold in folds {
        if fold.nested.is_empty() {
            if fold.closed != (level > win.foldlevel) {
                put_fold_open_close(w, fold);
            }
        } else {
            w.line(&fold.start.to_string());
            w.line("sil! normal! zo");
            put_foldopen_recurse(w, win, &fold.nested, level + 1);
            if fold.closed {
                put_fold_open_close(w, fold);
            }
        }
    }
}

fn put_fold_open_close(w: &mut Writer, fold: &FoldNode) {
    w.line(&fold.start.to_string());
    w.line(&format!("sil! normal! z{}", if fold.closed { 'c' } else { 'o' }));
}

/// vim `put_view`: everything that restores one window.
fn put_view(w: &mut Writer, snap: &SessionSnap, win: &WinSnap, add_edit: bool, current_arg_idx: usize) {
    w.line("argglobal");
    // Restore the argument index; `:argument` edits the file itself.
    let mut did_next = false;
    if snap.arg_idx != current_arg_idx && snap.arg_idx < snap.args.len() {
        w.line(&format!("{}argu", snap.arg_idx + 1));
        did_next = true;
    }
    let mut do_cursor = true;
    if add_edit && (!did_next || arg_idx_invalid(snap, win.path.as_deref())) {
        match &win.path {
            Some(path) => {
                let f = w.fname(path);
                w.line(&format!(
                    "if bufexists(fnamemodify(\"{f}\", \":p\")) | buffer {f} | else | edit {f} | endif"
                ));
                w.line("if &buftype ==# 'terminal'");
                w.line(&format!("  silent file {f}"));
                w.line("endif");
            }
            None => {
                w.line("enew");
                do_cursor = false;
            }
        }
    }
    if let Some(alt) = &win.alt {
        let f = w.fname(alt);
        w.line(&format!("balt {f}"));
    }
    if w.ssop.folds {
        for (name, value) in &win.fold_opts {
            match value.as_str() {
                "on" => w.line(&format!("setlocal {name}")),
                "off" => w.line(&format!("setlocal no{name}")),
                v => w.line(&format!("setlocal {name}={}", v.replace(' ', "\\ "))),
            }
        }
        if win.path.is_some() {
            put_folds(w, win);
        }
    }
    if do_cursor {
        if win.view_height == 0 {
            w.line(&format!("let s:l = {}", win.lnum));
        } else {
            w.line(&format!(
                "let s:l = {} - (({} * winheight(0) + {}) / {})",
                win.lnum,
                win.lnum.saturating_sub(win.topline),
                win.view_height / 2,
                win.view_height
            ));
        }
        w.line("if s:l < 1 | let s:l = 1 | endif");
        w.line("keepjumps exe s:l");
        w.line("normal! zt");
        w.line(&format!("keepjumps {}", win.lnum));
        if win.col == 0 {
            w.line("normal! 0");
        } else if !win.wrap && win.leftcol > 0 && win.width > 0 {
            w.line(&format!(
                "let s:c = {} - (({} * winwidth(0) + {}) / {})",
                win.virtcol + 1,
                win.virtcol.saturating_sub(win.leftcol),
                win.width / 2,
                win.width
            ));
            w.line("if s:c > 0");
            w.line(&format!("  exe 'normal! ' . s:c . '|zs' . {} . '|'", win.virtcol + 1));
            w.line("else");
            w.line(&format!("  normal! 0{}|", win.virtcol + 1));
            w.line("endif");
        } else {
            w.line(&format!("normal! 0{}|", win.virtcol + 1));
        }
    }
    if let Some(dir) = &win.localdir {
        let d = escape_fname(dir);
        w.line(&format!("lcd {d}"));
        w.did_lcd = true;
    }
}

/// vim `ex_mkrc` for `:mksession` with `makeopens`: the session file text.
/// `session_dir` is the directory the file is written to (for `sesdir`).
pub fn write(snap: &SessionSnap, ssop: SessionOptions, session_dir: &Path) -> String {
    let base = if ssop.sesdir {
        Some(session_dir.to_path_buf())
    } else if ssop.curdir {
        Some(snap.cwd.clone())
    } else {
        None
    };
    let mut w = Writer {
        out: String::new(),
        ssop,
        did_lcd: false,
        base,
    };
    w.line("let SessionLoad = 1");
    w.line("let s:so_save = &g:so | let s:siso_save = &g:siso | setg so=0 siso=0 | setl so=-1 siso=-1");

    // makeopens
    w.line("let v:this_session=expand(\"<sfile>:p\")");
    w.line("doautoall SessionLoadPre");
    w.line("silent only");
    if ssop.tabpages {
        w.line("silent tabonly");
    }
    if ssop.sesdir {
        w.line("exe \"cd \" . escape(expand(\"<sfile>:p:h\"), ' ')");
    } else if ssop.curdir {
        let d = escape_fname(&snap.cwd);
        w.line(&format!("cd {d}"));
    }
    w.line("if expand('%') == '' && !&modified && line('$') <= 1 && getline(1) == ''");
    w.line("  let s:wipebuf = bufnr('%')");
    w.line("endif");
    w.line("let s:shortmess_save = &shortmess");
    w.line("set shortmess+=aoO");
    let shown: Vec<&Path> = snap
        .tabs
        .iter()
        .flat_map(|t| t.windows.iter().filter_map(|w| w.path.as_deref()))
        .collect();
    for (path, lnum) in &snap.buffers {
        if ssop.buffers || shown.contains(&path.as_path()) {
            let f = w.fname(path);
            w.line(&format!("badd +{lnum} {f}"));
        }
    }
    // ses_arglist("argglobal", …)
    w.line("argglobal");
    w.line("%argdel");
    for arg in &snap.args {
        let f = if ssop.curdir {
            fnameescape(arg)
        } else {
            escape_fname(&snap.cwd.join(arg))
        };
        w.line(&format!("$argadd {f}"));
    }

    let tabs: &[TabSnap] = if ssop.tabpages {
        &snap.tabs
    } else {
        std::slice::from_ref(&snap.tabs[snap.curtab - 1])
    };
    let restore_stal = snap.showtabline == 1 && tabs.len() > 1;
    if restore_stal {
        w.line("set stal=2");
    }
    if ssop.tabpages {
        for _ in 1..tabs.len() {
            w.line("tabnew +setlocal\\ bufhidden=wipe");
        }
        if tabs.len() > 1 {
            w.line("tabrewind");
        }
    }

    let mut restore_height_width = false;
    let mut cur_arg_idx = 0;
    for (t, tab) in tabs.iter().enumerate() {
        let mut need_tabnext = ssop.tabpages && t > 0;
        // Before the layout, load one file, so an aborted load leaves no
        // useless windows.
        let mut edited_win = None;
        for (i, win) in tab.windows.iter().enumerate() {
            if let Some(path) = win.path.as_ref().filter(|_| w.do_win(win)) {
                if need_tabnext {
                    w.line("tabnext");
                    need_tabnext = false;
                }
                let f = w.fname(path);
                w.line(&format!("edit {f}"));
                if !arg_idx_invalid(snap, Some(path)) {
                    edited_win = Some(i);
                }
                break;
            }
        }
        if need_tabnext {
            w.line("tabnext");
        }
        if matches!(frame(&tab.shape), TreeShape::Split { .. }) {
            w.line("let s:save_splitbelow = &splitbelow");
            w.line("let s:save_splitright = &splitright");
            w.line("set splitbelow splitright");
            ses_win_rec(&mut w, &tab.shape);
            w.line("let &splitbelow = s:save_splitbelow");
            w.line("let &splitright = s:save_splitright");
        }
        let nr = tab.windows.iter().filter(|win| w.do_win(win)).count();
        let cnr = tab.windows[..tab.curwin.min(tab.windows.len())]
            .iter()
            .filter(|win| w.do_win(win))
            .count();
        if tab.windows.len() > 1 {
            w.line("wincmd t");
            if !restore_height_width {
                w.line("let s:save_winminheight = &winminheight");
                w.line("let s:save_winminwidth = &winminwidth");
            }
            w.line("set winminheight=0");
            w.line("set winheight=1");
            w.line("set winminwidth=0");
            w.line("set winwidth=1");
            restore_height_width = true;
        }
        if nr > 1 {
            ses_winsizes(&mut w, snap, tab);
        }
        if ssop.curdir {
            if let Some(dir) = &tab.localdir {
                let d = escape_fname(dir);
                w.line(&format!("tcd {d}"));
                w.did_lcd = true;
            }
        }
        for (i, win) in tab.windows.iter().enumerate() {
            if !w.do_win(win) {
                continue;
            }
            put_view(&mut w, snap, win, Some(i) != edited_win, cur_arg_idx);
            if nr > 1 {
                w.line("wincmd w");
            }
        }
        cur_arg_idx = snap.arg_idx;
        if cnr > 1 {
            w.line(&format!("{cnr}wincmd w"));
        }
        if nr > 1 {
            ses_winsizes(&mut w, snap, tab);
        }
    }

    if ssop.tabpages {
        w.line(&format!("tabnext {}", snap.curtab));
    }
    if restore_stal {
        w.line("set stal=1");
    }
    w.line("if exists('s:wipebuf') && len(win_findbuf(s:wipebuf)) == 0 && getbufvar(s:wipebuf, '&buftype') isnot# 'terminal'");
    w.line("  silent exe 'bwipe ' . s:wipebuf");
    w.line("endif");
    w.line("unlet! s:wipebuf");
    w.line(&format!("set winheight={} winwidth={}", snap.winheight, snap.winwidth));
    w.line("let &shortmess = s:shortmess_save");
    if restore_height_width {
        w.line("let &winminheight = s:save_winminheight");
        w.line("let &winminwidth = s:save_winminwidth");
    }
    w.line("let s:sx = expand(\"<sfile>:p:r\").\"x.vim\"");
    w.line("if filereadable(s:sx)");
    w.line("  exe \"source \" . fnameescape(s:sx)");
    w.line("endif");
    // ex_mkrc's footer
    w.line("let &g:so = s:so_save | let &g:siso = s:siso_save");
    if snap.hlsearch {
        w.line("set hlsearch");
    }
    w.line("doautoall SessionLoadPost");
    w.line("unlet SessionLoad");
    w.line("\" vim: set ft=vim :");
    w.out
}

#[cfg(test)]
mod tests {
    use super::*;
    use zmax_view::DocumentId;

    fn leaf() -> TreeShape {
        TreeShape::Leaf {
            doc: DocumentId::default(),
            focused: false,
        }
    }

    fn split(layout: Layout, children: Vec<TreeShape>) -> TreeShape {
        TreeShape::Split {
            layout,
            children: children.into_iter().map(|c| (1.0, c)).collect(),
        }
    }

    fn writer() -> Writer {
        Writer {
            out: String::new(),
            ssop: SessionOptions::parse(DEFAULT_SESSIONOPTIONS),
            did_lcd: false,
            base: Some(PathBuf::from("/w")),
        }
    }

    /// `ses_win_rec` for `row(b, col(c, a))`: the lines nvim 0.12 wrote for that
    /// layout (`tests/fixtures/sessions/nested.vim`). zmax's root container
    /// around a tab page's frame writes nothing of its own.
    #[test]
    fn window_layout_commands_match_nvims() {
        let shape = split(
            Layout::Vertical,
            vec![split(
                Layout::Vertical,
                vec![leaf(), split(Layout::Horizontal, vec![leaf(), leaf()])],
            )],
        );
        let mut w = writer();
        ses_win_rec(&mut w, &shape);
        assert_eq!(
            w.out,
            "wincmd _ | wincmd |\nvsplit\n1wincmd h\nwincmd w\n\
             wincmd _ | wincmd |\nsplit\n1wincmd k\nwincmd w\n"
        );
    }

    /// zmax keeps folds flat; the writer nests them as vim's `w_folds` does
    /// and writes the inner ones first (`put_folds_recurse`).
    #[test]
    fn folds_nest_and_inner_folds_are_written_first() {
        let tree = fold_tree([(10, 30, true), (12, 14, false), (40, 60, false)].into_iter());
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].nested.len(), 1);
        let mut w = writer();
        put_folds_recurse(&mut w, &tree);
        assert_eq!(w.out, "sil! 12,14fold\nsil! 10,30fold\nsil! 40,60fold\n");
    }

    /// vim `fnameescape`: Ex-special characters are backslashed, and a name
    /// starting with `+` or `>` too.
    #[test]
    fn file_names_are_escaped_as_fnameescape_does() {
        assert_eq!(fnameescape("my notes.txt"), "my\\ notes.txt");
        assert_eq!(fnameescape("a#b%c.txt"), "a\\#b\\%c.txt");
        assert_eq!(fnameescape("+x"), "\\+x");
        assert_eq!(fnameescape("plain/path.rs"), "plain/path.rs");
    }

    /// A short name is relative to the session's directory until an `:lcd`
    /// is written, then every name is in full.
    #[test]
    fn names_are_short_until_a_local_directory() {
        let mut w = writer();
        assert_eq!(w.fname(Path::new("/w/sub/a.txt")), "sub/a.txt");
        w.did_lcd = true;
        assert_eq!(w.fname(Path::new("/w/sub/a.txt")), "/w/sub/a.txt");
    }
}
