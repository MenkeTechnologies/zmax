#!/usr/bin/env python3
"""Generate the zemacs port report.

Measures how much of the exhaustive Vim/Neovim + Emacs + Spacemacs feature
surface (the *denominator*, cited inventories under ``port/data/*.json``) is
implemented by zemacs (the *numerator*, re-derived from source on every run).

Honesty contract (mirrors the zshrs ``gen_port_report.py`` precedent):

* The numerator is parsed from the actual zemacs source every run. It is never
  read from a cache or a hand-maintained number, so it cannot go stale.
* The only curated artifact is ``port/mapping.json``: spec-id -> evidence.
* Every evidence token MUST resolve to a real, parsed zemacs command / keymap
  binding. A mapping that points at a non-existent command is reported as a
  BROKEN MAPPING, counted as *absent*, and listed loudly at the top of the
  report. You cannot inflate the number by whitelisting; you can only inflate
  it by adding real code and pointing the mapping at it.
* ``ported`` and ``partial`` are reported separately. Headline coverage is
  ``ported`` only.

Run: ``python3 scripts/gen_port_report.py``
Outputs: ``docs/port_report.html`` and ``docs/port_report.md``.
"""

import json
import os
import re
import sys
from collections import defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DATA = os.path.join(ROOT, "port", "data")
MAPPING = os.path.join(ROOT, "port", "mapping.json")
ZEMACS_TERM = os.path.join(ROOT, "zemacs-term", "src")
OUT_HTML = os.path.join(ROOT, "docs", "port_report.html")
OUT_MD = os.path.join(ROOT, "docs", "port_report.md")
# Also emitted as an mdBook chapter so it publishes to gh-pages.
OUT_BOOK = os.path.join(ROOT, "book", "src", "generated", "port-report.md")

# Keybinding-only report (subset of the port report: just the key-press surface
# of each editor, excluding ex-commands, options, functions, layers, M-x).
KB_HTML = os.path.join(ROOT, "docs", "keybinding_report.html")
KB_MD = os.path.join(ROOT, "docs", "keybinding_report.md")
KB_BOOK = os.path.join(ROOT, "book", "src", "generated", "keybinding-report.md")
KEYBIND_CATS = {
    "neovim": {"normal-mode", "visual-mode", "insert-mode", "cmdline-editing"},
    "emacs": {"keybinding"},
    "spacemacs": {"keybinding", "emacs-prefix"},
    # The entire JetBrains default keymap is key-press surface (every action is a
    # shortcut), so include all of its categories via the "*" wildcard rather
    # than freezing a list that goes stale as new categories are cited.
    "jetbrains": {"*"},
}


# --------------------------------------------------------------------------
# Numerator: parse the real zemacs source.
# --------------------------------------------------------------------------
def _match_delim(src, i, open_ch, close_ch):
    """From index `i` (just past an opening `open_ch`, nesting depth 1), return the
    index just past its matching `close_ch`. Skips Rust string literals and `//`
    line comments so delimiter characters that appear as *keys* (e.g. the `"}"` /
    `"{"` / `")"` keys in the keymap) or inside comments don't miscount. This is
    what lets the parser handle real keymap content like vim's `zf}` fold key."""
    n = len(src)
    depth = 1
    while i < n and depth:
        ch = src[i]
        if ch == '"':
            i += 1
            while i < n and src[i] != '"':
                if src[i] == "\\":
                    i += 1
                i += 1
            i += 1
            continue
        if ch == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                i += 1
            continue
        if ch == open_ch:
            depth += 1
        elif ch == close_ch:
            depth -= 1
        i += 1
    return i


def parse_static_commands():
    """Return {name: doc} for every entry in the static_commands! invocation."""
    path = os.path.join(ZEMACS_TERM, "commands.rs")
    src = open(path, encoding="utf-8").read()
    # Locate the macro INVOCATION (not the macro_rules! definition).
    m = re.search(r"\n\s*static_commands!\(", src)
    if not m:
        sys.exit("FATAL: static_commands! invocation not found in commands.rs")
    start = m.end()
    # Walk to the matching close paren (string/comment-aware).
    i = _match_delim(src, start, "(", ")")
    block = src[start : i - 1]
    cmds = {}
    for cm in re.finditer(r'^\s*([a-z][a-z0-9_]+)\s*,\s*"([^"]*)"', block, re.M):
        cmds[cm.group(1)] = cm.group(2)
    return cmds


def parse_typable_commands():
    """Return {name} for every typable (:) command, including aliases."""
    path = os.path.join(ZEMACS_TERM, "commands", "typed.rs")
    src = open(path, encoding="utf-8").read()
    names = set()
    # Command names may be capitalized (fzf.vim ports: :Files, :GFiles, :Rg, …).
    for cm in re.finditer(r'name:\s*"([A-Za-z0-9!:/_-]+)"', src):
        names.add(cm.group(1))
    # aliases: aliases: &["..","..']
    for am in re.finditer(r"aliases:\s*&\[([^\]]*)\]", src):
        for a in re.finditer(r'"([A-Za-z0-9!:/_-]+)"', am.group(1)):
            names.add(a.group(1))
    # macro-generated list entries: `ex_modifier_entry!("name", &["alias"], ...)`
    # and `vim_map_command!("name", ...)` — same effect as a literal entry.
    for mm in re.finditer(
        r'(?:ex_modifier_entry|vim_map_command)!\(\s*"([A-Za-z0-9!:/_-]+)"\s*(?:,\s*&\[([^\]]*)\])?',
        src,
    ):
        names.add(mm.group(1))
        if mm.group(2):
            for a in re.finditer(r'"([A-Za-z0-9!:/_-]+)"', mm.group(2)):
                names.add(a.group(1))
    return names


def parse_keymap():
    """Parse keymap/default.rs into {mode: {chord: command}}.

    chord is a space-joined key sequence, e.g. "g g". Aliases (``"x" | "y"``)
    each get an entry. Submaps (``"g" => { ... }``) recurse with a key prefix.
    """
    # zemacs ships the vim keymap as the default (keymap/vim.rs), so the report
    # measures the keymap users actually get.
    path = os.path.join(ZEMACS_TERM, "keymap", "vim.rs")
    src = open(path, encoding="utf-8").read()
    result = defaultdict(dict)

    def brace_body(open_idx):
        """open_idx points just past the opening `{`; return (body, end_idx)."""
        i = _match_delim(src, open_idx, "{", "}")
        return src[open_idx : i - 1], i

    # Process keymap constructs in source order so clone() sees its source map
    # and merge_nodes() layers on top — mirrors how default.rs builds the modes:
    #   let normal = keymap!({ ... });
    #   let mut select = normal.clone();
    #   select.merge_nodes(keymap!({ ... }));
    pat = re.compile(
        r"let\s+(?:mut\s+)?(\w+)\s*=\s*keymap!\(\{"      # 1: base def
        r"|let\s+mut\s+(\w+)\s*=\s*(\w+)\.clone\(\)"      # 2,3: clone
        r"|(\w+)\.merge_nodes\(keymap!\(\{"               # 4: merge
    )
    for m in pat.finditer(src):
        if m.group(1):  # base keymap! definition
            body, _ = brace_body(m.end())
            _walk_keymap(body, [], result[m.group(1)])
        elif m.group(2):  # clone: copy source map
            result[m.group(2)] = dict(result.get(m.group(3), {}))
        elif m.group(4):  # merge_nodes: layer onto existing map
            body, _ = brace_body(m.end())
            _walk_keymap(body, [], result[m.group(4)])

    # Bindings inserted programmatically after macro construction are declared in
    # parseable tables; `add_command` accepts either a typable `:cmd` or a bare
    # static command name (`cmd.parse::<MappableCommand>()`):
    #   ("space f s", "Files", ":write"),  ("space g f l", "Git", "git_file_log_picker"),
    # Match the chord (first string) of any (chord, label, cmd) tuple.
    for tm in re.finditer(
        r'\(\s*"([A-Za-z][^"]*)"\s*,\s*"[^"]*"\s*,\s*"(:?[A-Za-z][^"]*)"\s*\)', src
    ):
        chord, cmd = tm.group(1), tm.group(2).lstrip(":")
        result["normal"][chord] = cmd

    # The shipped default preset is spacemacs = the vim base + the Emacs
    # C-x / C-c / C-h prefixes overlaid on normal/select/insert (keymap/
    # spacemacs.rs, applied by spacemacs::default()). Parse each `fn *_prefix()`
    # keymap! body and the CX_TYPABLE graft table, then layer them onto all three
    # modes so evidence like `key:insert:C-x C-f` resolves against what users get.
    sm_path = os.path.join(ZEMACS_TERM, "keymap", "spacemacs.rs")
    try:
        sm = open(sm_path, encoding="utf-8").read()
    except OSError:
        sm = ""
    # Applied in the same precedence order as spacemacs::default(): the generated
    # CXCH_FULL fallbacks first, then the curated `*_prefix()` maps override them
    # on collision (curated real bindings win; non-colliding fallbacks survive).
    overlay = {}
    # 1. CXCH_FULL: (full-chord, submap-label, command) — the exhaustive map.
    tbl2 = re.search(r"CXCH_FULL[^=]*=\s*&\[(.*?)\];", sm, re.S)
    if tbl2:
        for chord, cmd in re.findall(
            r'\(\s*"([^"]+)"\s*,\s*"[^"]*"\s*,\s*"([^"]+)"\s*\)', tbl2.group(1)
        ):
            overlay[chord] = cmd
    # 2. Curated C-x/C-c/C-h prefix maps override the fallbacks.
    for fm in re.finditer(r"fn\s+c[xch]_prefix\(\)", sm):
        km = sm.find("keymap!({", fm.end())
        if km == -1:
            continue
        open_idx = km + len("keymap!({")
        end = _match_delim(sm, open_idx, "{", "}")
        _walk_keymap(sm[open_idx : end - 1], [], overlay)
    # 3. CX_TYPABLE grafts under C-x.
    tbl = re.search(r"CX_TYPABLE[^=]*=\s*&\[(.*?)\];", sm, re.S)
    if tbl:
        for k2, cmd in re.findall(
            r'\(\s*"([^"]+)"\s*,\s*"[^"]*"\s*,\s*"(:?[^"]+)"\s*\)', tbl.group(1)
        ):
            overlay["C-x " + k2] = cmd.lstrip(":")
    for mode in ("normal", "select", "insert"):
        result[mode].update(overlay)

    # `.` dot-repeat is handled specially in EditorView (ui/editor.rs), not via a
    # keymap binding or a command, so detect that hardcoded handler directly.
    editor_view = os.path.join(ZEMACS_TERM, "ui", "editor.rs")
    try:
        ev = open(editor_view, encoding="utf-8").read()
        if "key!('.')" in ev and "last_insert" in ev:
            result["normal"]["."] = "repeat_last_insert (EditorView)"
    except OSError:
        pass

    # Command-line editing keys live in the `:` prompt, not the keymap macro:
    # ui/prompt.rs handles them with a hardcoded `match event { ... }`. Parse
    # that handler so the cmdline-editing surface is measured from real code.
    result["command"] = parse_prompt_keymap()
    # Dired is a modal Component; expose its keys as a `dired` mode.
    result["dired"] = parse_dired_keymap()
    # Calendar is a modal Component too.
    result["calendar"] = _parse_component_keymap("calendar.rs", "calendar")
    return result


# Named keys as written in the ctrl!/alt!/key!/shift! macros, normalised to the
# same chord vocabulary the keymap macro uses (see zemacs-view/src/input.rs).
_NAMED_KEYS = {
    "Esc": "esc", "Enter": "ret", "Left": "left", "Right": "right",
    "Up": "up", "Down": "down", "Home": "home", "End": "end",
    "Backspace": "backspace", "Delete": "del", "Tab": "tab",
    "PageUp": "pageup", "PageDown": "pagedown", "Insert": "ins",
}


def parse_prompt_keymap():
    """Extract the keys the `:` prompt handles into {chord: "prompt"}.

    Parses the `match event { ... }` arms in ui/prompt.rs, recognising the
    ctrl!/alt!/key!/shift! key macros. This is the command-line editing surface;
    like the EditorView dot-repeat handler above, it is hardcoded rather than
    expressed in the keymap macro, so it is read straight from source.
    """
    path = os.path.join(ZEMACS_TERM, "ui", "prompt.rs")
    try:
        src = open(path, encoding="utf-8").read()
    except OSError:
        return {}
    # There are two `match event` blocks: one unwraps the Event, the other (the
    # one we want) handles keys. Pick the balanced block that contains key macros.
    body = ""
    for m in re.finditer(r"\bmatch\s+event\s*\{", src):
        i = _match_delim(src, m.end(), "{", "}")
        candidate = src[m.end() : i - 1]
        if "ctrl!(" in candidate or "key!(" in candidate:
            body = candidate
            break
    if not body:
        return {}

    out = {}
    for mm in re.finditer(r"\b(ctrl|alt|shift|key)!\(\s*('?)(\w+)\2\s*\)", body):
        macro, _q, arg = mm.group(1), mm.group(2), mm.group(3)
        key = _NAMED_KEYS.get(arg, arg)  # named key -> canonical, else literal char
        prefix = {"ctrl": "C-", "alt": "A-", "shift": "S-", "key": ""}[macro]
        out[f"{prefix}{key}"] = "prompt"
    return out


def parse_dired_keymap():
    """Extract the keys the Dired mode handles into {chord: "dired"}.

    Parses the `match key { ... }` arms in ui/dired.rs (ctrl!/alt!/key! macros,
    both `'c'` char literals and named keys). Dired is a modal Component whose
    keys live in its `handle_event`, not the keymap macro, so — like the `:`
    prompt (parse_prompt_keymap) — they are read straight from source, exposing a
    `dired` mode so `key:dired:m` etc. resolve for the Emacs Dired keybindings.
    """
    return _parse_component_keymap("dired.rs", "dired")


def _parse_component_keymap(filename, mode):
    """Parse the `match key { ... }` handler of a modal ui Component into
    {chord: mode}, recognising ctrl!/alt!/shift!/key! with `'c'` char literals or
    named keys. Shared by Dired, Calendar and any future major-mode overlay."""
    path = os.path.join(ZEMACS_TERM, "ui", filename)
    try:
        src = open(path, encoding="utf-8").read()
    except OSError:
        return {}
    # These Component files only invoke the key macros inside `handle_event`, so
    # scan the whole file: a brace-matcher on the `match key { .. }` block would
    # be fooled by `alt!('}')` / `key!('{')` char literals (not delimiter-aware).
    out = {}
    for mm in re.finditer(r"\b(ctrl|alt|shift|key)!\(\s*(?:'(.)'|(\w+))\s*\)", src):
        macro, ch, named = mm.group(1), mm.group(2), mm.group(3)
        key = ch if ch is not None else _NAMED_KEYS.get(named, named)
        prefix = {"ctrl": "C-", "alt": "A-", "shift": "S-", "key": ""}[macro]
        out[f"{prefix}{key}"] = mode
    return out


def _split_keys(keyspec):
    """`"g" | "down"` -> ['g', 'down']; strips quotes, unescapes `\\"`/`\\\\`."""
    raw = re.findall(r'"((?:[^"\\]|\\.)*)"', keyspec)
    return [k.replace('\\"', '"').replace("\\\\", "\\") for k in raw]


def _walk_keymap(body, prefix, out):
    """Recursively extract bindings. ``out`` is mutated: {chord: command}."""
    i = 0
    n = len(body)
    while i < n:
        # Find next quoted-key spec followed by =>.
        m = re.compile(
            r'((?:"(?:[^"\\]|\\.)*"\s*\|\s*)*"(?:[^"\\]|\\.)*")\s*=>'
        ).search(body, i)
        if not m:
            break
        keys = _split_keys(m.group(1))
        after = m.end()
        # Skip whitespace.
        j = after
        while j < n and body[j].isspace():
            j += 1
        if j < n and body[j] == "{":
            # Submap: find matching brace (string/comment-aware), recurse.
            k = _match_delim(body, j + 1, "{", "}")
            inner = body[j + 1 : k - 1]
            # First token inside is the label string "Label"; the walker's
            # regex needs a `=>` so the bare label is naturally ignored.
            for key in keys:
                _walk_keymap(inner, prefix + [key], out)
            i = k
        else:
            # Leaf: command name, or [cmd, cmd] sequence, until , or newline.
            seg = re.match(r"\s*([^,\n]+)", body[after:])
            target = seg.group(1).strip() if seg else ""
            target = target.rstrip(",").strip()
            # Normalise list form [a, b] -> first command (evidence only needs presence)
            cmd = target.strip("[] ").split(",")[0].strip()
            cmd = re.sub(r"\(.*$", "", cmd).strip()  # drop any (args)
            for key in keys:
                chord = " ".join(prefix + [key])
                if cmd:
                    out[chord] = cmd
            i = after + (seg.end() if seg else 0)


# --------------------------------------------------------------------------
# Inventories + mapping.
# --------------------------------------------------------------------------
def load_inventories():
    items = []
    for f in sorted(os.listdir(DATA)):
        if not f.endswith(".json") or f.startswith("_"):
            continue
        arr = json.load(open(os.path.join(DATA, f), encoding="utf-8"))
        items.extend(arr)
    return items


def load_mapping():
    if not os.path.exists(MAPPING):
        return []
    return json.load(open(MAPPING, encoding="utf-8"))


# --------------------------------------------------------------------------
# Resolution.
# --------------------------------------------------------------------------
def parse_builtins():
    """Engine-level vim behaviours that are real but not expressible as a keymap
    entry (so they cannot use `key:` evidence). Each is verified against actual
    source so the catalogue stays honest-by-construction."""
    builtins = set()
    ed = open(os.path.join(ROOT, "zemacs-term/src/ui/editor.rs"), encoding="utf-8").read()
    if "is_count_key" in ed:
        # numeric count prefix (1-9 …) consumed before a command, and {count}<Del>
        # editing it back, are handled by the count machinery in EditorView.
        builtins.add("count")
    # Vimscript builtins implemented by the embedded vimlrs interpreter
    # (callable via `:vim`/`:viml`). Each `pub fn f_<name>` is a real impl,
    # re-parsed from source; catalogued as `viml:<name>` so a neovim `function`
    # item resolves via `builtin:viml:<name>`.
    import os as _os, re as _re
    _viml=_os.path.join(ROOT,"vendor","vimlrs","src")
    if _os.path.isdir(_viml):
        for dp,_d,fs in _os.walk(_viml):
            for fn in fs:
                if fn.endswith(".rs"):
                    try: t=open(_os.path.join(dp,fn),encoding="utf-8").read()
                    except OSError: continue
                    for m in _re.finditer(r"\bpub fn f_([A-Za-z0-9_]+)",t): builtins.add("viml:"+m.group(1))
    # vimlrs ex-statements (`:let`,`:if`,`:for`,`:try`,`:echon` …) it parses/runs,
    # callable via `:vim`; catalogued as viml:ex:<name>.
    _ast=_os.path.join(ROOT,"vendor","vimlrs","src","viml_ast.rs")
    if _os.path.isfile(_ast):
        _t=open(_ast,encoding="utf-8").read()
        for _kw in ["let","if","elseif","else","endif","for","endfor","while","endwhile","function","endfunction","return","call","execute","echo","echon","echomsg","echoerr","try","catch","finally","endtry","throw","break","continue","unlet","const"]:
            if _re.search(r"\b"+_kw.capitalize()+r"\b",_t): builtins.add("viml:ex:"+_kw)
    return builtins


def resolve(zemacs):
    """zemacs = dict of parsed source sets. Returns per-item status + broken list."""
    statics = zemacs["statics"]
    typables = zemacs["typables"]
    keymap = zemacs["keymap"]
    builtins = zemacs.get("builtins", parse_builtins())

    def evidence_ok(tok):
        kind, _, rest = tok.partition(":")
        if kind == "static":
            return rest in statics
        if kind == "typable":
            return rest in typables
        if kind == "key":
            mode, _, chord = rest.partition(":")
            return chord in keymap.get(mode, {})
        if kind == "builtin":
            return rest in builtins
        return False

    mapping = load_mapping()
    by_id = {}
    broken = []
    seen_ids = set()
    for entry in mapping:
        sid = entry["spec_id"]
        if sid in seen_ids:
            broken.append((sid, "duplicate mapping entry", entry.get("evidence", [])))
            continue
        seen_ids.add(sid)
        ev = entry.get("evidence", [])
        bad = [t for t in ev if not evidence_ok(t)]
        if not ev or bad:
            broken.append((sid, "no/unresolved evidence: %s" % (bad or "empty"), ev))
            by_id[sid] = ("broken", ev, entry.get("note", ""))
        else:
            st = entry.get("status", "ported")
            if st not in ("ported", "partial"):
                st = "ported"
            by_id[sid] = (st, ev, entry.get("note", ""))
    return by_id, broken


# --------------------------------------------------------------------------
# Report assembly.
# --------------------------------------------------------------------------
def build():
    zemacs = {
        "statics": set(parse_static_commands().keys()),
        "typables": parse_typable_commands(),
        "keymap": parse_keymap(),
        "builtins": parse_builtins(),
    }
    items = load_inventories()
    by_id, broken = resolve(zemacs)
    broken_ids = {b[0] for b in broken}

    # Aggregate by (source, category).
    agg = defaultdict(lambda: {"total": 0, "ported": 0, "partial": 0})
    src_agg = defaultdict(lambda: {"total": 0, "ported": 0, "partial": 0})
    rows = []
    for it in items:
        sid = it["id"]
        src = it["source"]
        cat = it["category"]
        status = "absent"
        desc = it.get("desc", "")   # what the key does in the upstream editor
        zmap = ""                    # what zemacs maps it to (mapping note / evidence)
        if sid in by_id and sid not in broken_ids:
            status, ev, note = by_id[sid]
            zmap = note or ", ".join(ev)
        agg[(src, cat)]["total"] += 1
        src_agg[src]["total"] += 1
        if status in ("ported", "partial"):
            agg[(src, cat)][status] += 1
            src_agg[src][status] += 1
        rows.append((src, cat, it["name"], status, it.get("doc_ref", ""), desc, zmap))

    total = len(items)
    ported = sum(1 for r in rows if r[3] == "ported")
    partial = sum(1 for r in rows if r[3] == "partial")
    keybind_count = sum(len(v) for v in zemacs["keymap"].values())
    stats = {
        "total": total,
        "ported": ported,
        "partial": partial,
        "absent": total - ported - partial,
        "broken": len(broken),
        "static_cmds": len(zemacs["statics"]),
        "typable_cmds": len(zemacs["typables"]),
        "keybindings": keybind_count,
    }
    return stats, src_agg, agg, broken, rows


def pct(n, d):
    return (100.0 * n / d) if d else 0.0


# --------------------------------------------------------------------------
# strykelang HUD chrome (shared by both HTML reports).
# --------------------------------------------------------------------------
# Inline component <style> — these classes (stat-grid, file-table, bar-wrap,
# feature-grid, …) are NOT in hud-static.css/tutorial.css, so every page must
# embed this block verbatim.
STRYKE_STYLE = """  <style>
    .tutorial-main { max-width: 76rem; }
    .bar-wrap { background:var(--bg-primary);border:1px solid var(--border);border-radius:2px;height:18px;position:relative;overflow:hidden; }
    .bar-fill { height:100%;border-radius:1px;transition:width 1.2s cubic-bezier(.22,1,.36,1); }
    .bar-fill.green  { background:linear-gradient(90deg,#39ff14,#20c00a);box-shadow:0 0 8px rgba(57,255,20,.4); }
    .bar-fill.cyan   { background:linear-gradient(90deg,#05d9e8,#0891b2);box-shadow:0 0 8px rgba(5,217,232,.4); }
    .bar-fill.yellow { background:linear-gradient(90deg,#ffb800,#e8a000);box-shadow:0 0 8px rgba(255,184,0,.35); }
    .bar-fill.magenta{ background:linear-gradient(90deg,#d300c5,#a000a0);box-shadow:0 0 8px rgba(211,0,197,.35); }
    .bar-pct { position:absolute;right:6px;top:0;line-height:18px;font-size:10px;font-weight:700;color:#fff;text-shadow:0 0 4px #000;font-family:'Orbitron',sans-serif; }

    .file-table { width:100%;border-collapse:collapse;margin:0.6rem 0;font-size:12px; }
    .file-table th { background:var(--bg-secondary);color:var(--cyan);font-family:'Orbitron',sans-serif;font-size:10px;font-weight:700;letter-spacing:1.2px;text-transform:uppercase;text-align:left;padding:7px 10px;border:1px solid var(--border); }
    .file-table td { padding:6px 10px;border:1px solid var(--border);color:var(--text-dim);vertical-align:middle; }
    .file-table tr:hover td { background:var(--bg-hover); }
    .file-table td:first-child { font-family:'Share Tech Mono',monospace;color:var(--accent-light);font-weight:600;white-space:nowrap; }
    .file-table .num { text-align:right;font-family:'Share Tech Mono',monospace; }
    .file-table .total-row td { background:var(--bg-secondary);font-weight:700;color:var(--text);border-top:2px solid var(--cyan); }
    .file-table code { font-size:11px;color:var(--accent-light);background:var(--bg-primary);padding:1px 4px;border-radius:2px; }

    .stat-grid { display:grid;grid-template-columns:repeat(auto-fill,minmax(14rem,1fr));gap:0.75rem;margin:1.2rem 0; }
    .stat-card { border:1px solid var(--border);border-top:3px solid var(--cyan);background:var(--bg-card);padding:1rem 1.2rem;border-radius:2px;text-align:center; }
    .stat-card .stat-val { font-family:'Orbitron',sans-serif;font-size:28px;font-weight:900;color:var(--cyan);line-height:1.1;text-shadow:0 0 20px var(--cyan-glow); }
    .stat-card .stat-val.accent { color:var(--accent);text-shadow:0 0 20px var(--accent-glow); }
    .stat-card .stat-val.green  { color:var(--green);text-shadow:0 0 20px rgba(57,255,20,.3); }
    .stat-card .stat-label { font-family:'Orbitron',sans-serif;font-size:9px;font-weight:700;letter-spacing:2px;text-transform:uppercase;color:var(--text-muted);margin-top:0.5rem; }
    @keyframes glow-pulse { 0%,100%{text-shadow:0 0 20px var(--cyan-glow)}50%{text-shadow:0 0 40px var(--cyan-glow),0 0 80px var(--cyan-dim)} }
    .stat-card .stat-val { animation:glow-pulse 3s ease-in-out infinite; }

    .mapping-grid { display:grid;grid-template-columns:repeat(auto-fill,minmax(20rem,1fr));gap:0.65rem;margin:0.8rem 0; }
    .mapping-card { border:1px solid var(--border);border-left:3px solid var(--magenta);background:var(--bg-card);padding:0.6rem 0.9rem;border-radius:2px; }
    .mapping-card h4 { font-family:'Orbitron',sans-serif;font-size:10px;font-weight:700;letter-spacing:1.5px;text-transform:uppercase;color:var(--magenta);margin:0 0 0.3rem; }
    .mapping-card p { margin:0;font-size:11px;color:var(--text-dim);line-height:1.5; }
    .mapping-card code { font-size:10.5px;color:var(--accent-light);background:var(--bg-primary);padding:1px 4px;border-radius:2px; }

    .section-rule { border:none;border-top:1px dashed var(--border);margin:2rem 0; }

    .feature-grid { display:grid;grid-template-columns:repeat(auto-fill,minmax(22rem,1fr));gap:0.65rem;margin:0.8rem 0; }
    .feature-card { border:1px solid var(--border);border-left:3px solid var(--cyan);background:var(--bg-card);padding:0.7rem 1rem;border-radius:2px; }
    .feature-card h4 { font-family:'Orbitron',sans-serif;font-size:10px;font-weight:700;letter-spacing:1.5px;text-transform:uppercase;color:var(--cyan);margin:0 0 0.3rem; }
    .feature-card p { margin:0;font-size:11px;color:var(--text-dim);line-height:1.55; }
    .feature-card code { font-size:10.5px;color:var(--accent-light);background:var(--bg-primary);padding:1px 4px;border-radius:2px; }
    .feature-card ul { margin:0.3rem 0 0;padding-left:1.2rem;font-size:11px;color:var(--text-dim);line-height:1.6; }
    .feature-card li code { font-size:10px; }
  </style>"""

# hud-theme.js owns the Theme / CRT / Neon toggles AND the 8 color schemes
# (cyberpunk, midnight, matrix, ember, arctic, crimson, toxic, vapor) — it wires
# #btnTheme/#btnCrt/#btnNeon and auto-injects the color-scheme strip after the
# header. We only keep the page-specific bar-fill grow animation inline.
STRYKE_SCRIPT = """  <script src="hud-theme.js"></script>
  <script>
    document.addEventListener('DOMContentLoaded', () => {
      document.querySelectorAll('.bar-fill').forEach(bar => {
        const w = bar.style.width;
        bar.style.width = '0';
        requestAnimationFrame(() => { requestAnimationFrame(() => { bar.style.width = w; }); });
      });
    });
  </script>"""


def stryke_head(title, desc):
    """Full <!DOCTYPE>…</head> with the strykelang head + inline component style."""
    return (
        '<!DOCTYPE html>\n<html lang="en">\n<head>\n'
        '  <meta charset="utf-8">\n'
        '  <meta name="viewport" content="width=device-width, initial-scale=1">\n'
        '  <meta name="color-scheme" content="dark light">\n'
        f'  <meta name="description" content="{desc}">\n'
        f'  <title>{title}</title>\n'
        '  <link rel="preconnect" href="https://fonts.googleapis.com">\n'
        '  <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>\n'
        '  <link href="https://fonts.googleapis.com/css2?family=Orbitron:wght@400;600;700;900&family=Share+Tech+Mono&display=swap" rel="stylesheet">\n'
        '  <link rel="stylesheet" href="hud-static.css">\n'
        '  <link rel="stylesheet" href="tutorial.css">\n'
        + STRYKE_STYLE + '\n</head>'
    )


def stryke_header(brand, current, crumbs, subtitle):
    """<body> opening through <main>: tutorial-header chrome + breadcrumbs + toolbar.

    crumbs is a list of (label, href); http(s) hrefs open in a new tab.
    """
    nav = [f'<span class="current">{current}</span>']
    for label, href in crumbs:
        nav.append('<span class="sep">/</span>')
        if href.startswith("http"):
            nav.append(
                f'<a href="{href}" target="_blank" rel="noopener noreferrer">{label}</a>'
            )
        else:
            nav.append(f'<a href="{href}">{label}</a>')
    nav_html = "\n            ".join(nav)
    return (
        '<body>\n'
        '  <div class="app tutorial-app" id="reportApp">\n'
        '    <div class="crt-scanline" id="crtH" aria-hidden="true"></div>\n'
        '    <div class="crt-scanline-v" id="crtV" aria-hidden="true"></div>\n'
        '    <header class="tutorial-header">\n'
        '      <div class="tutorial-header-inner">\n'
        '        <div>\n'
        f'          <h1 class="tutorial-brand">{brand}</h1>\n'
        '          <nav class="tutorial-crumbs" aria-label="Breadcrumb">\n'
        f'            {nav_html}\n'
        '          </nav>\n'
        '          <p style="margin:0.35rem 0 0;font-family:\'Share Tech Mono\',monospace;font-size:11px;color:var(--text-dim);letter-spacing:0.03em;opacity:0.75;">\n'
        f'            {subtitle}\n'
        '          </p>\n'
        '        </div>\n'
        '        <div class="tutorial-toolbar">\n'
        '          <button type="button" class="btn btn-secondary" id="btnTheme" title="Toggle light/dark">Theme</button>\n'
        '          <button type="button" class="btn btn-secondary active" id="btnCrt" title="CRT scanline overlay">CRT</button>\n'
        '          <button type="button" class="btn btn-secondary active" id="btnNeon" title="Neon border pulse">Neon</button>\n'
        '        </div>\n'
        '      </div>\n'
        '    </header>\n'
        '    <main class="tutorial-main">'
    )


def stryke_footer(text):
    """</main> + footer + theme/CRT/neon + bar-animation script, closing the doc."""
    return (
        '    </main>\n'
        '    <footer style="text-align:center;padding:2rem;font-size:10px;color:var(--text-muted);font-family:\'Orbitron\',sans-serif;letter-spacing:2px;">\n'
        f'      {text}\n'
        '    </footer>\n'
        '  </div>\n\n'
        + STRYKE_SCRIPT + '\n</body>\n</html>'
    )


# Status -> inline color, used in table cells / detail summaries.
STATUS_COLOR = {
    "ported": "var(--green)",
    "partial": "var(--accent)",
    "absent": "var(--text-muted)",
}


def write_md(stats, src_agg, agg, broken):
    L = []
    L.append("# zemacs Port Report\n")
    L.append(
        "Auto-generated by `scripts/gen_port_report.py`. Numerator is re-derived "
        "from zemacs source on every run; denominator is the cited inventories "
        "under `port/data/`. Do not edit by hand.\n"
    )
    L.append("## Headline\n")
    L.append(f"- Denominator (cited feature items): **{stats['total']}**")
    L.append(
        f"- Ported: **{stats['ported']}** ({pct(stats['ported'], stats['total']):.1f}%)"
    )
    L.append(
        f"- Partial: **{stats['partial']}** ({pct(stats['partial'], stats['total']):.1f}%)"
    )
    L.append(f"- Absent: **{stats['absent']}**")
    L.append(f"- Broken mappings (counted absent): **{stats['broken']}**")
    L.append(
        f"- zemacs source surface: {stats['static_cmds']} static commands, "
        f"{stats['typable_cmds']} typable `:` commands, "
        f"{stats['keybindings']} default keybindings\n"
    )
    if broken:
        L.append("## ⚠ BROKEN MAPPINGS\n")
        L.append("These point at zemacs code that does not exist. Fix or remove.\n")
        for sid, why, ev in broken:
            L.append(f"- `{sid}` — {why} — evidence: `{ev}`")
        L.append("")
    if "functionality" in src_agg:
        f = src_agg["functionality"]
        fa = f["total"] - f["ported"] - f["partial"]
        L.append("## Functionality coverage (capabilities, deduplicated)\n")
        L.append(
            "The primary measure: distinct editor *capabilities*, one row each, "
            "regardless of how many ancestor editors expose the same feature. It "
            "answers \"what can zemacs do\" without counting `go-to-line` four "
            "times across Vim, Emacs, Spacemacs and JetBrains.\n"
        )
        L.append(f"- Capabilities tracked: **{f['total']}**")
        L.append(f"- Implemented (ported): **{f['ported']}** ({pct(f['ported'], f['total']):.1f}%)")
        L.append(f"- Partial / different model: **{f['partial']}**")
        L.append(f"- Absent (genuine gaps): **{fa}**\n")
        L.append("| Area | Total | Ported | Partial | % |")
        L.append("|---|--:|--:|--:|--:|")
        for (src, cat) in sorted(agg):
            if src != "functionality":
                continue
            a = agg[(src, cat)]
            L.append(
                f"| {cat} | {a['total']} | {a['ported']} | {a['partial']} | "
                f"{pct(a['ported'], a['total']):.1f}% |"
            )
        L.append("")
        L.append(
            "> The per-source tables below measure *muscle-memory compatibility* "
            "with each ancestor editor instead. They overlap heavily (the same "
            "capability is counted once per source) and the Emacs denominator is "
            "the entire GNU Emacs manual — games, Dired, Gnus, Calc, TeX-mode and "
            "all — so a low per-source percentage reflects scope and duplication, "
            "not missing functionality. Read functionality coverage above for the "
            "honest capability picture.\n"
        )
    L.append("## Coverage by source\n")
    L.append("| Source | Total | Ported | Partial | Ported % |")
    L.append("|---|--:|--:|--:|--:|")
    for src in sorted(src_agg):
        a = src_agg[src]
        L.append(
            f"| {src} | {a['total']} | {a['ported']} | {a['partial']} | "
            f"{pct(a['ported'], a['total']):.1f}% |"
        )
    L.append("")
    L.append("## Coverage by source / category\n")
    L.append("| Source | Category | Total | Ported | Partial | Ported % |")
    L.append("|---|---|--:|--:|--:|--:|")
    for (src, cat) in sorted(agg):
        a = agg[(src, cat)]
        L.append(
            f"| {src} | {cat} | {a['total']} | {a['ported']} | {a['partial']} | "
            f"{pct(a['ported'], a['total']):.1f}% |"
        )
    L.append("")
    text = "\n".join(L)
    open(OUT_MD, "w", encoding="utf-8").write(text)
    os.makedirs(os.path.dirname(OUT_BOOK), exist_ok=True)
    open(OUT_BOOK, "w", encoding="utf-8").write(text)


def write_html(stats, src_agg, agg, broken, rows):
    def bar(n, d):
        p = pct(n, d)
        return (
            f'<div class="bar-wrap"><div class="bar-fill cyan" style="width:{p:.1f}%"></div>'
            f'<span class="bar-pct">{p:.1f}%</span></div>'
        )

    h = [stryke_head(
        "zemacs &mdash; Port Report",
        "zemacs port report — coverage of the Vim/Neovim + Emacs + Spacemacs "
        "feature surface, with a numerator re-derived from zemacs source on every run.",
    )]
    h.append(stryke_header(
        "// ZEMACS &mdash; PORT REPORT",
        "Port Report",
        [
            ("Home", "index.html"),
            ("Engineering Report", "report.html"),
            ("Keybinding Coverage", "keybinding_report.html"),
            ("GitHub", "https://github.com/MenkeTechnologies/zemacs"),
        ],
        "Coverage of the Vim/Neovim + Emacs + Spacemacs feature surface; "
        "numerator re-derived from source every run.",
    ))

    h.append('      <h2 class="tutorial-title"><span class="step-hash">&gt;_</span>HEADLINE</h2>')
    h.append(
        '      <p class="tutorial-subtitle">Auto-generated by <code>scripts/gen_port_report.py</code>. '
        'Numerator re-derived from zemacs source on every run; denominator is the cited inventories under '
        '<code>port/data/</code> (Vim/Neovim runtime docs, GNU Emacs manual indexes, Spacemacs documentation). '
        'Headline coverage counts <strong>ported</strong> only.</p>'
    )
    h.append('      <div class="stat-grid">')
    h.append(f'        <div class="stat-card"><div class="stat-val">{stats["total"]}</div><div class="stat-label">Feature Items (Denominator)</div></div>')
    h.append(f'        <div class="stat-card"><div class="stat-val green">{stats["ported"]}</div><div class="stat-label">Ported ({pct(stats["ported"], stats["total"]):.1f}%)</div></div>')
    h.append(f'        <div class="stat-card"><div class="stat-val accent">{stats["partial"]}</div><div class="stat-label">Partial ({pct(stats["partial"], stats["total"]):.1f}%)</div></div>')
    h.append(f'        <div class="stat-card"><div class="stat-val">{stats["absent"]}</div><div class="stat-label">Absent</div></div>')
    bcls = "accent" if stats["broken"] else "green"
    h.append(f'        <div class="stat-card"><div class="stat-val {bcls}">{stats["broken"]}</div><div class="stat-label">Broken Mappings</div></div>')
    h.append(f'        <div class="stat-card"><div class="stat-val">{stats["static_cmds"]}</div><div class="stat-label">Static Commands</div></div>')
    h.append(f'        <div class="stat-card"><div class="stat-val">{stats["typable_cmds"]}</div><div class="stat-label">Typable Commands</div></div>')
    h.append(f'        <div class="stat-card"><div class="stat-val">{stats["keybindings"]}</div><div class="stat-label">Default Keybindings</div></div>')
    h.append('      </div>')

    if "functionality" in src_agg:
        f = src_agg["functionality"]
        fa = f["total"] - f["ported"] - f["partial"]
        h.append('      <hr class="section-rule">')
        h.append('      <h2 class="tutorial-title"><span class="step-hash">&gt;_</span>FUNCTIONALITY COVERAGE</h2>')
        h.append(
            '      <p class="tutorial-subtitle">The primary measure: distinct editor '
            '<strong>capabilities</strong>, one row each, regardless of how many ancestor '
            'editors expose the same feature &mdash; what zemacs can do, without counting '
            '<code>go-to-line</code> four times across Vim, Emacs, Spacemacs and JetBrains.</p>'
        )
        h.append('      <div class="stat-grid">')
        h.append(f'        <div class="stat-card"><div class="stat-val">{f["total"]}</div><div class="stat-label">Capabilities</div></div>')
        h.append(f'        <div class="stat-card"><div class="stat-val green">{f["ported"]}</div><div class="stat-label">Ported ({pct(f["ported"], f["total"]):.1f}%)</div></div>')
        h.append(f'        <div class="stat-card"><div class="stat-val accent">{f["partial"]}</div><div class="stat-label">Partial</div></div>')
        h.append(f'        <div class="stat-card"><div class="stat-val">{fa}</div><div class="stat-label">Absent (genuine gaps)</div></div>')
        h.append('      </div>')
        h.append('      <table class="file-table">')
        h.append('        <thead><tr><th>Area</th><th class="num">Total</th><th class="num">Ported</th><th class="num">Partial</th><th>Progress</th></tr></thead>')
        h.append('        <tbody>')
        for (src, cat) in sorted(agg):
            if src != "functionality":
                continue
            a = agg[(src, cat)]
            h.append(
                f'          <tr><td>{cat}</td><td class="num">{a["total"]}</td>'
                f'<td class="num" style="color:var(--green);">{a["ported"]}</td>'
                f'<td class="num" style="color:var(--accent);">{a["partial"]}</td>'
                f'<td>{bar(a["ported"], a["total"])}</td></tr>'
            )
        h.append('        </tbody>')
        h.append('      </table>')
        h.append(
            '      <p class="tutorial-subtitle">The per-source tables below measure '
            '<em>muscle-memory compatibility</em> with each ancestor editor instead. They '
            'overlap heavily (the same capability counted once per source), and the Emacs '
            'denominator is the entire GNU Emacs manual &mdash; games, Dired, Gnus, Calc, '
            'TeX-mode and all &mdash; so a low per-source percentage reflects scope and '
            'duplication, not missing functionality.</p>'
        )

    if broken:
        h.append('      <hr class="section-rule">')
        h.append('      <h2 class="tutorial-title"><span class="step-hash">~</span>BROKEN MAPPINGS</h2>')
        h.append(
            '      <p class="tutorial-subtitle">These mapping entries point at zemacs code that does not exist. '
            'They are counted as <strong>absent</strong>. Fix the evidence or remove the entry.</p>'
        )
        h.append('      <table class="file-table">')
        h.append('        <thead><tr><th>spec_id</th><th>problem</th><th>evidence</th></tr></thead>')
        h.append('        <tbody>')
        for sid, why, ev in broken:
            h.append(
                f'          <tr><td>{sid}</td>'
                f'<td style="color:#f85149;font-weight:700;">{why}</td><td>{ev}</td></tr>'
            )
        h.append('        </tbody>')
        h.append('      </table>')

    h.append('      <hr class="section-rule">')
    h.append('      <h2 class="tutorial-title"><span class="step-hash">~</span>COVERAGE BY SOURCE</h2>')
    h.append('      <p class="tutorial-subtitle">Ported and partial counts per upstream editor, against that editor\'s cited feature inventory.</p>')
    h.append('      <table class="file-table">')
    h.append('        <thead><tr><th>Source</th><th class="num">Total</th><th class="num">Ported</th><th class="num">Partial</th><th>Progress</th></tr></thead>')
    h.append('        <tbody>')
    for src in sorted(src_agg):
        a = src_agg[src]
        h.append(
            f'          <tr><td>{src}</td><td class="num">{a["total"]}</td>'
            f'<td class="num" style="color:var(--green);">{a["ported"]}</td>'
            f'<td class="num" style="color:var(--accent);">{a["partial"]}</td>'
            f'<td>{bar(a["ported"], a["total"])}</td></tr>'
        )
    h.append('        </tbody>')
    h.append('      </table>')

    h.append('      <hr class="section-rule">')
    h.append('      <h2 class="tutorial-title"><span class="step-hash">~</span>COVERAGE BY SOURCE / CATEGORY</h2>')
    h.append('      <p class="tutorial-subtitle">The same coverage, broken down by the category each feature belongs to.</p>')
    h.append('      <table class="file-table">')
    h.append('        <thead><tr><th>Source</th><th>Category</th><th class="num">Total</th><th class="num">Ported</th><th class="num">Partial</th><th>Progress</th></tr></thead>')
    h.append('        <tbody>')
    for (src, cat) in sorted(agg):
        a = agg[(src, cat)]
        h.append(
            f'          <tr><td>{src}</td><td>{cat}</td><td class="num">{a["total"]}</td>'
            f'<td class="num" style="color:var(--green);">{a["ported"]}</td>'
            f'<td class="num" style="color:var(--accent);">{a["partial"]}</td>'
            f'<td>{bar(a["ported"], a["total"])}</td></tr>'
        )
    h.append('        </tbody>')
    h.append('      </table>')

    # Per-category item detail (collapsible) — ported/partial first.
    h.append('      <hr class="section-rule">')
    h.append('      <h2 class="tutorial-title"><span class="step-hash">~</span>ITEM DETAIL</h2>')
    h.append('      <p class="tutorial-subtitle">Per-category breakdown; ported and partial items first. Expand a category to see every item and its source reference.</p>')
    cats = defaultdict(list)
    for src, cat, name, status, ref, _desc, _zmap in rows:
        cats[(src, cat)].append((name, status, ref))
    order = {"ported": 0, "partial": 1, "absent": 2}
    for (src, cat) in sorted(cats):
        lst = cats[(src, cat)]
        nported = sum(1 for x in lst if x[1] == "ported")
        npart = sum(1 for x in lst if x[1] == "partial")
        h.append(
            f'      <details><summary>{src} / {cat} '
            f'<span style="color:var(--text-muted);">({len(lst)} items · '
            f'<span style="color:var(--green);">{nported} ported</span> · '
            f'<span style="color:var(--accent);">{npart} partial</span>)</span></summary>'
        )
        h.append('      <table class="file-table">')
        h.append('        <thead><tr><th>Feature</th><th>Status</th><th>Source ref</th></tr></thead>')
        h.append('        <tbody>')
        for name, status, ref in sorted(lst, key=lambda x: (order[x[1]], x[0])):
            esc = (
                name.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            )
            h.append(
                f'          <tr><td>{esc}</td>'
                f'<td style="color:{STATUS_COLOR[status]};">{status}</td>'
                f'<td style="color:var(--text-muted);">{ref}</td></tr>'
            )
        h.append('        </tbody>')
        h.append('      </table></details>')

    h.append(stryke_footer("ZEMACS &middot; PORT REPORT &middot; MENKETECHNOLOGIES"))
    open(OUT_HTML, "w", encoding="utf-8").write("\n".join(h))


def write_keybinding_report(rows):
    """A focused report on keybinding coverage only (the key-press surface of
    vim/neovim, emacs, spacemacs and the JetBrains default keymap), derived from
    the same resolved `rows`."""
    kb = [
        (src, cat, name, status, ref, desc, zmap)
        for (src, cat, name, status, ref, desc, zmap) in rows
        if src in KEYBIND_CATS
        and ("*" in KEYBIND_CATS[src] or cat in KEYBIND_CATS[src])
    ]
    src_agg = defaultdict(lambda: {"total": 0, "ported": 0, "partial": 0})
    cat_agg = defaultdict(lambda: {"total": 0, "ported": 0, "partial": 0})
    for src, cat, _name, status, _ref, _desc, _zmap in kb:
        for bucket in (src_agg[src], cat_agg[(src, cat)]):
            bucket["total"] += 1
            if status in ("ported", "partial"):
                bucket[status] += 1
    total = len(kb)
    ported = sum(1 for r in kb if r[3] == "ported")
    partial = sum(1 for r in kb if r[3] == "partial")

    # --- markdown ---
    L = ["# zemacs Keybinding Coverage\n"]
    L.append(
        "Auto-generated by `scripts/gen_port_report.py` — the keybinding subset "
        "of the [port report](port_report.md). Counts only the **key-press "
        "surface** (vim/neovim normal/visual/insert/cmdline keys, the GNU Emacs "
        "Key Index, the Spacemacs `SPC` tree, and the JetBrains default keymap); "
        "ex-commands, options, functions, layers and `M-x` commands are excluded. "
        "`ported` means the same key, pressed in zemacs, does the equivalent "
        "thing. Do not edit by hand.\n"
    )
    L.append("## Headline\n")
    L.append(f"- Cited keybindings (denominator): **{total}**")
    L.append(f"- Ported: **{ported}** ({pct(ported, total):.1f}%)")
    L.append(f"- Partial: **{partial}** ({pct(partial, total):.1f}%)")
    L.append(f"- Absent: **{total - ported - partial}**\n")
    L.append("## By source\n")
    L.append("| Source | Keybindings | Ported | Partial | Ported % |")
    L.append("|---|--:|--:|--:|--:|")
    for src in sorted(src_agg):
        a = src_agg[src]
        L.append(
            f"| {src} | {a['total']} | {a['ported']} | {a['partial']} | "
            f"{pct(a['ported'], a['total']):.1f}% |"
        )
    L.append("")
    L.append("## By source / category\n")
    L.append("| Source | Category | Keybindings | Ported | Partial | Ported % |")
    L.append("|---|---|--:|--:|--:|--:|")
    for (src, cat) in sorted(cat_agg):
        a = cat_agg[(src, cat)]
        L.append(
            f"| {src} | {cat} | {a['total']} | {a['ported']} | {a['partial']} | "
            f"{pct(a['ported'], a['total']):.1f}% |"
        )
    L.append("")
    L.append(
        "Emacs coverage is low because zemacs is a modal (vim) editor — most Emacs "
        "chord bindings (`C-x C-f`, …) are intentionally not bound. What *is* counted "
        "are the global readline-style editing keys that genuinely work here (e.g. "
        "`C-a`/`C-e`/`C-k`, `M-f`/`M-b`/`M-d`, `M-x`, `C-s`); the remaining Emacs "
        "*commands* are tracked under the port report's emacs `command` category.\n"
    )
    text = "\n".join(L)
    open(KB_MD, "w", encoding="utf-8").write(text)
    os.makedirs(os.path.dirname(KB_BOOK), exist_ok=True)
    open(KB_BOOK, "w", encoding="utf-8").write(text)

    # --- html ---
    def bar(n, d):
        p = pct(n, d)
        return (
            f'<div class="bar-wrap"><div class="bar-fill cyan" style="width:{p:.1f}%"></div>'
            f'<span class="bar-pct">{p:.1f}%</span></div>'
        )

    h = [stryke_head(
        "zemacs &mdash; Keybinding Coverage",
        "zemacs keybinding coverage — the key-press subset of the port report: "
        "vim/neovim, the GNU Emacs Key Index, and the Spacemacs SPC tree.",
    )]
    h.append(stryke_header(
        "// ZEMACS &mdash; KEYBINDING COVERAGE",
        "Keybinding Coverage",
        [
            ("Home", "index.html"),
            ("Engineering Report", "report.html"),
            ("Port Report", "port_report.html"),
            ("GitHub", "https://github.com/MenkeTechnologies/zemacs"),
        ],
        "The key-press subset of the port report: which bindings are ported, "
        "partial, or pending.",
    ))

    h.append('      <h2 class="tutorial-title"><span class="step-hash">&gt;_</span>HEADLINE</h2>')
    h.append(
        '      <p class="tutorial-subtitle">The keybinding subset of the '
        '<a href="port_report.html" style="color:var(--accent-light);">port report</a>. Counts only the key-press '
        'surface — vim/neovim normal/visual/insert/cmdline keys, the GNU Emacs Key Index, and the Spacemacs '
        '<code>SPC</code> tree. <strong>ported</strong> = the same key does the equivalent thing in zemacs.</p>'
    )
    h.append('      <div class="stat-grid">')
    h.append(f'        <div class="stat-card"><div class="stat-val">{total}</div><div class="stat-label">Cited Keybindings</div></div>')
    h.append(f'        <div class="stat-card"><div class="stat-val green">{ported}</div><div class="stat-label">Ported ({pct(ported, total):.1f}%)</div></div>')
    h.append(f'        <div class="stat-card"><div class="stat-val accent">{partial}</div><div class="stat-label">Partial ({pct(partial, total):.1f}%)</div></div>')
    h.append(f'        <div class="stat-card"><div class="stat-val">{total - ported - partial}</div><div class="stat-label">Absent</div></div>')
    h.append('      </div>')

    h.append('      <hr class="section-rule">')
    h.append('      <h2 class="tutorial-title"><span class="step-hash">~</span>BY SOURCE</h2>')
    h.append('      <p class="tutorial-subtitle">Key-press coverage per upstream editor.</p>')
    h.append('      <table class="file-table">')
    h.append('        <thead><tr><th>Source</th><th class="num">Keybindings</th><th class="num">Ported</th><th class="num">Partial</th><th>Progress</th></tr></thead>')
    h.append('        <tbody>')
    for src in sorted(src_agg):
        a = src_agg[src]
        h.append(
            f'          <tr><td>{src}</td><td class="num">{a["total"]}</td>'
            f'<td class="num" style="color:var(--green);">{a["ported"]}</td>'
            f'<td class="num" style="color:var(--accent);">{a["partial"]}</td>'
            f'<td>{bar(a["ported"], a["total"])}</td></tr>'
        )
    h.append('        </tbody>')
    h.append('      </table>')

    h.append('      <hr class="section-rule">')
    h.append('      <h2 class="tutorial-title"><span class="step-hash">~</span>BY SOURCE / CATEGORY</h2>')
    h.append('      <p class="tutorial-subtitle">Broken down by the key mode / index each binding belongs to.</p>')
    h.append('      <table class="file-table">')
    h.append('        <thead><tr><th>Source</th><th>Category</th><th class="num">Keybindings</th><th class="num">Ported</th><th class="num">Partial</th><th>Progress</th></tr></thead>')
    h.append('        <tbody>')
    for (src, cat) in sorted(cat_agg):
        a = cat_agg[(src, cat)]
        h.append(
            f'          <tr><td>{src}</td><td>{cat}</td><td class="num">{a["total"]}</td>'
            f'<td class="num" style="color:var(--green);">{a["ported"]}</td>'
            f'<td class="num" style="color:var(--accent);">{a["partial"]}</td>'
            f'<td>{bar(a["ported"], a["total"])}</td></tr>'
        )
    h.append('        </tbody>')
    h.append('      </table>')
    h.append(
        '      <p class="tutorial-subtitle">Emacs is 0% because zemacs is a modal (vim) editor — '
        'Emacs default chords (<code>C-x C-f</code>, …) are intentionally not bound; the commands they invoke '
        'are tracked under the port report\'s emacs <code>command</code> category.</p>'
    )

    # per-category item detail
    h.append('      <hr class="section-rule">')
    h.append('      <h2 class="tutorial-title"><span class="step-hash">~</span>KEYBINDING DETAIL</h2>')
    h.append('      <p class="tutorial-subtitle">Every cited key, grouped by source and category; ported and partial first.</p>')
    def esc_html(s):
        return (s or "").replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

    cats = defaultdict(list)
    for src, cat, name, status, ref, desc, zmap in kb:
        cats[(src, cat)].append((name, status, ref, desc, zmap))
    order = {"ported": 0, "partial": 1, "absent": 2}
    for (src, cat) in sorted(cats):
        lst = cats[(src, cat)]
        np_ = sum(1 for x in lst if x[1] == "ported")
        pa_ = sum(1 for x in lst if x[1] == "partial")
        h.append(
            f'      <details><summary>{src} / {cat} '
            f'<span style="color:var(--text-muted);">({len(lst)} keys · '
            f'<span style="color:var(--green);">{np_} ported</span> · '
            f'<span style="color:var(--accent);">{pa_} partial</span>)</span></summary>'
        )
        h.append('      <table class="file-table">')
        h.append('        <thead><tr><th>Key</th><th>Action (in editor)</th><th>&rarr; zemacs</th><th>Status</th><th>Source ref</th></tr></thead>')
        h.append('        <tbody>')
        for name, status, ref, desc, zmap in sorted(lst, key=lambda x: (order[x[1]], x[0])):
            zcell = esc_html(zmap) if zmap else '<span style="color:var(--text-muted);">&mdash;</span>'
            h.append(
                f'          <tr><td>{esc_html(name)}</td>'
                f'<td style="color:var(--text-dim);">{esc_html(desc)}</td>'
                f'<td style="color:var(--accent-light);">{zcell}</td>'
                f'<td style="color:{STATUS_COLOR[status]};">{status}</td>'
                f'<td style="color:var(--text-muted);">{ref}</td></tr>'
            )
        h.append('        </tbody>')
        h.append('      </table></details>')
    h.append(stryke_footer("ZEMACS &middot; KEYBINDING COVERAGE &middot; MENKETECHNOLOGIES"))
    open(KB_HTML, "w", encoding="utf-8").write("\n".join(h))
    return total, ported, partial


def main():
    os.makedirs(os.path.dirname(OUT_HTML), exist_ok=True)
    stats, src_agg, agg, broken, rows = build()
    write_md(stats, src_agg, agg, broken)
    write_html(stats, src_agg, agg, broken, rows)
    kb_total, kb_ported, kb_partial = write_keybinding_report(rows)
    print(
        f"denominator={stats['total']} ported={stats['ported']} "
        f"partial={stats['partial']} absent={stats['absent']} "
        f"broken={stats['broken']}"
    )
    print(
        f"zemacs surface: {stats['static_cmds']} static, "
        f"{stats['typable_cmds']} typable, {stats['keybindings']} keybindings"
    )
    print(
        f"keybindings: {kb_total} total, {kb_ported} ported "
        f"({pct(kb_ported, kb_total):.1f}%), {kb_partial} partial"
    )
    if broken:
        print("WARNING: %d broken mapping(s) — see report." % len(broken))
    print(f"wrote {OUT_MD}")
    print(f"wrote {OUT_HTML}")
    print(f"wrote {KB_MD}")
    print(f"wrote {KB_HTML}")


if __name__ == "__main__":
    main()
