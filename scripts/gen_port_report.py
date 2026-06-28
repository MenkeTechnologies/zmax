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
    "spacemacs": {"keybinding"},
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
    for cm in re.finditer(r'name:\s*"([a-z0-9!_-]+)"', src):
        names.add(cm.group(1))
    # aliases: aliases: &["..","..']
    for am in re.finditer(r"aliases:\s*&\[([^\]]*)\]", src):
        for a in re.finditer(r'"([a-z0-9!_-]+)"', am.group(1)):
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
        if sid in by_id and sid not in broken_ids:
            status = by_id[sid][0]
        agg[(src, cat)]["total"] += 1
        src_agg[src]["total"] += 1
        if status in ("ported", "partial"):
            agg[(src, cat)][status] += 1
            src_agg[src][status] += 1
        rows.append((src, cat, it["name"], status, it.get("doc_ref", "")))

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
            f'<div class="bar"><div class="fill" style="width:{p:.1f}%"></div>'
            f'<span>{p:.1f}%</span></div>'
        )

    h = []
    h.append("<!doctype html><html lang=en><head><meta charset=\"utf-8\">")
    h.append("<meta name=\"viewport\" content='width=device-width,initial-scale=1'>")
    h.append("<title>zemacs port report</title>")
    h.append(
        "<style>"
        "body{background:#0b0e14;color:#c5c8c6;font:14px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace;margin:0;padding:2rem;max-width:1100px;margin:auto}"
        "h1{color:#00e5ff;letter-spacing:.05em}h2{color:#ff2e88;margin-top:2rem;border-bottom:1px solid #1c2230;padding-bottom:.3rem}"
        "a{color:#7af}.muted{color:#6b7280}"
        "table{border-collapse:collapse;width:100%;margin:.5rem 0}"
        "th,td{border:1px solid #1c2230;padding:.35rem .6rem;text-align:left}"
        "th{background:#11151f;color:#9fb3c8}td.n{text-align:right}"
        ".bar{position:relative;background:#11151f;border:1px solid #1c2230;height:18px;min-width:120px}"
        ".fill{position:absolute;top:0;left:0;height:100%;background:linear-gradient(90deg,#00e5ff,#ff2e88)}"
        ".bar span{position:relative;padding-left:.4rem;font-size:12px;color:#e6e6e6}"
        ".ported{color:#3fb950}.partial{color:#d29922}.absent{color:#6b7280}.broken{color:#f85149;font-weight:bold}"
        ".kpi{display:flex;gap:1rem;flex-wrap:wrap;margin:1rem 0}"
        ".card{background:#11151f;border:1px solid #1c2230;padding:.8rem 1.2rem;border-radius:6px}"
        ".card b{display:block;font-size:1.6rem;color:#00e5ff}"
        "details{margin:.4rem 0}summary{cursor:pointer;color:#9fb3c8}"
        "</style></head><body>"
    )
    h.append("<h1>zemacs port report</h1>")
    h.append(
        "<p class=muted>Auto-generated by <code>scripts/gen_port_report.py</code>. "
        "Numerator re-derived from zemacs source on every run; denominator is the "
        "cited inventories under <code>port/data/</code> (Vim/Neovim runtime docs, "
        "GNU Emacs manual indexes, Spacemacs documentation). Headline coverage counts "
        "<b>ported</b> only.</p>"
    )
    h.append("<div class=kpi>")
    h.append(f"<div class=card><b>{stats['total']}</b>feature items (denominator)</div>")
    h.append(
        f"<div class=card><b class=ported>{stats['ported']}</b>ported "
        f"({pct(stats['ported'], stats['total']):.1f}%)</div>"
    )
    h.append(
        f"<div class=card><b class=partial>{stats['partial']}</b>partial "
        f"({pct(stats['partial'], stats['total']):.1f}%)</div>"
    )
    h.append(f"<div class=card><b class=absent>{stats['absent']}</b>absent</div>")
    bc = "broken" if stats["broken"] else "ported"
    h.append(f"<div class=card><b class={bc}>{stats['broken']}</b>broken mappings</div>")
    h.append("</div>")
    h.append(
        "<p class=muted>zemacs surface: "
        f"{stats['static_cmds']} static commands · {stats['typable_cmds']} typable "
        f"commands · {stats['keybindings']} default keybindings</p>"
    )

    if broken:
        h.append("<h2 class=broken>⚠ Broken mappings</h2>")
        h.append(
            "<p>These mapping entries point at zemacs code that does not exist. "
            "They are counted as <b>absent</b>. Fix the evidence or remove the entry.</p>"
        )
        h.append("<table><tr><th>spec_id</th><th>problem</th><th>evidence</th></tr>")
        for sid, why, ev in broken:
            h.append(
                f"<tr><td>{sid}</td><td class=broken>{why}</td><td>{ev}</td></tr>"
            )
        h.append("</table>")

    h.append("<h2>Coverage by source</h2><table>")
    h.append("<tr><th>Source</th><th>Total</th><th>Ported</th><th>Partial</th><th>Progress</th></tr>")
    for src in sorted(src_agg):
        a = src_agg[src]
        h.append(
            f"<tr><td>{src}</td><td class=n>{a['total']}</td>"
            f"<td class='n ported'>{a['ported']}</td>"
            f"<td class='n partial'>{a['partial']}</td>"
            f"<td>{bar(a['ported'], a['total'])}</td></tr>"
        )
    h.append("</table>")

    h.append("<h2>Coverage by source / category</h2><table>")
    h.append("<tr><th>Source</th><th>Category</th><th>Total</th><th>Ported</th><th>Partial</th><th>Progress</th></tr>")
    for (src, cat) in sorted(agg):
        a = agg[(src, cat)]
        h.append(
            f"<tr><td>{src}</td><td>{cat}</td><td class=n>{a['total']}</td>"
            f"<td class='n ported'>{a['ported']}</td>"
            f"<td class='n partial'>{a['partial']}</td>"
            f"<td>{bar(a['ported'], a['total'])}</td></tr>"
        )
    h.append("</table>")

    # Per-category item detail (collapsible) — ported/partial first.
    h.append("<h2>Item detail</h2>")
    cats = defaultdict(list)
    for src, cat, name, status, ref in rows:
        cats[(src, cat)].append((name, status, ref))
    order = {"ported": 0, "partial": 1, "absent": 2}
    for (src, cat) in sorted(cats):
        lst = cats[(src, cat)]
        nported = sum(1 for x in lst if x[1] == "ported")
        npart = sum(1 for x in lst if x[1] == "partial")
        h.append(
            f"<details><summary>{src} / {cat} "
            f"<span class=muted>({len(lst)} items · "
            f"<span class=ported>{nported} ported</span> · "
            f"<span class=partial>{npart} partial</span>)</span></summary>"
        )
        h.append("<table><tr><th>Feature</th><th>Status</th><th>Source ref</th></tr>")
        for name, status, ref in sorted(lst, key=lambda x: (order[x[1]], x[0])):
            esc = (
                name.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            )
            h.append(
                f"<tr><td>{esc}</td><td class={status}>{status}</td>"
                f"<td class=muted>{ref}</td></tr>"
            )
        h.append("</table></details>")

    h.append("</body></html>")
    open(OUT_HTML, "w", encoding="utf-8").write("\n".join(h))


def write_keybinding_report(rows):
    """A focused report on keybinding coverage only (the key-press surface of
    vim/neovim, emacs and spacemacs), derived from the same resolved `rows`."""
    kb = [
        (src, cat, name, status, ref)
        for (src, cat, name, status, ref) in rows
        if src in KEYBIND_CATS and cat in KEYBIND_CATS[src]
    ]
    src_agg = defaultdict(lambda: {"total": 0, "ported": 0, "partial": 0})
    cat_agg = defaultdict(lambda: {"total": 0, "ported": 0, "partial": 0})
    for src, cat, _name, status, _ref in kb:
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
        "Key Index, and the Spacemacs `SPC` tree); ex-commands, options, "
        "functions, layers and `M-x` commands are excluded. `ported` means the "
        "same key, pressed in zemacs, does the equivalent thing. Do not edit by "
        "hand.\n"
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
            f'<div class="bar"><div class="fill" style="width:{p:.1f}%"></div>'
            f'<span>{p:.1f}%</span></div>'
        )

    h = ["<!doctype html><html lang=en><head><meta charset=\"utf-8\">"]
    h.append("<meta name=\"viewport\" content='width=device-width,initial-scale=1'>")
    h.append("<title>zemacs keybinding coverage</title>")
    h.append(
        "<style>"
        "body{background:#0b0e14;color:#c5c8c6;font:14px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace;margin:auto;padding:2rem;max-width:1100px}"
        "h1{color:#00e5ff;letter-spacing:.05em}h2{color:#ff2e88;margin-top:2rem;border-bottom:1px solid #1c2230;padding-bottom:.3rem}"
        ".muted{color:#6b7280}a{color:#7af}"
        "table{border-collapse:collapse;width:100%;margin:.5rem 0}"
        "th,td{border:1px solid #1c2230;padding:.35rem .6rem;text-align:left}"
        "th{background:#11151f;color:#9fb3c8}td.n{text-align:right}"
        ".bar{position:relative;background:#11151f;border:1px solid #1c2230;height:18px;min-width:120px}"
        ".fill{position:absolute;top:0;left:0;height:100%;background:linear-gradient(90deg,#00e5ff,#ff2e88)}"
        ".bar span{position:relative;padding-left:.4rem;font-size:12px;color:#e6e6e6}"
        ".ported{color:#3fb950}.partial{color:#d29922}.absent{color:#6b7280}"
        ".kpi{display:flex;gap:1rem;flex-wrap:wrap;margin:1rem 0}"
        ".card{background:#11151f;border:1px solid #1c2230;padding:.8rem 1.2rem;border-radius:6px}"
        ".card b{display:block;font-size:1.6rem;color:#00e5ff}"
        "details{margin:.4rem 0}summary{cursor:pointer;color:#9fb3c8}"
        "</style></head><body>"
    )
    h.append("<h1>zemacs keybinding coverage</h1>")
    h.append(
        "<p class=muted>The keybinding subset of the "
        "<a href='port_report.html'>port report</a>. Counts only the key-press "
        "surface — vim/neovim normal/visual/insert/cmdline keys, the GNU Emacs "
        "Key Index, and the Spacemacs <code>SPC</code> tree. <b>ported</b> = the "
        "same key does the equivalent thing in zemacs.</p>"
    )
    h.append("<div class=kpi>")
    h.append(f"<div class=card><b>{total}</b>cited keybindings</div>")
    h.append(f"<div class=card><b class=ported>{ported}</b>ported ({pct(ported, total):.1f}%)</div>")
    h.append(f"<div class=card><b class=partial>{partial}</b>partial ({pct(partial, total):.1f}%)</div>")
    h.append(f"<div class=card><b class=absent>{total - ported - partial}</b>absent</div>")
    h.append("</div>")
    h.append("<h2>By source</h2><table>")
    h.append("<tr><th>Source</th><th>Keybindings</th><th>Ported</th><th>Partial</th><th>Progress</th></tr>")
    for src in sorted(src_agg):
        a = src_agg[src]
        h.append(
            f"<tr><td>{src}</td><td class=n>{a['total']}</td>"
            f"<td class='n ported'>{a['ported']}</td><td class='n partial'>{a['partial']}</td>"
            f"<td>{bar(a['ported'], a['total'])}</td></tr>"
        )
    h.append("</table>")
    h.append("<h2>By source / category</h2><table>")
    h.append("<tr><th>Source</th><th>Category</th><th>Keybindings</th><th>Ported</th><th>Partial</th><th>Progress</th></tr>")
    for (src, cat) in sorted(cat_agg):
        a = cat_agg[(src, cat)]
        h.append(
            f"<tr><td>{src}</td><td>{cat}</td><td class=n>{a['total']}</td>"
            f"<td class='n ported'>{a['ported']}</td><td class='n partial'>{a['partial']}</td>"
            f"<td>{bar(a['ported'], a['total'])}</td></tr>"
        )
    h.append("</table>")
    h.append(
        "<p class=muted>Emacs is 0% because zemacs is a modal (vim) editor — "
        "Emacs default chords (<code>C-x C-f</code>, …) are intentionally not "
        "bound; the commands they invoke are tracked under the port report's "
        "emacs <code>command</code> category.</p>"
    )
    # per-category item detail
    h.append("<h2>Keybinding detail</h2>")
    cats = defaultdict(list)
    for src, cat, name, status, ref in kb:
        cats[(src, cat)].append((name, status, ref))
    order = {"ported": 0, "partial": 1, "absent": 2}
    for (src, cat) in sorted(cats):
        lst = cats[(src, cat)]
        np_ = sum(1 for x in lst if x[1] == "ported")
        pa_ = sum(1 for x in lst if x[1] == "partial")
        h.append(
            f"<details><summary>{src} / {cat} <span class=muted>({len(lst)} keys · "
            f"<span class=ported>{np_} ported</span> · <span class=partial>{pa_} partial</span>)"
            "</span></summary>"
        )
        h.append("<table><tr><th>Key</th><th>Status</th><th>Source ref</th></tr>")
        for name, status, ref in sorted(lst, key=lambda x: (order[x[1]], x[0])):
            esc = name.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            h.append(
                f"<tr><td>{esc}</td><td class={status}>{status}</td>"
                f"<td class=muted>{ref}</td></tr>"
            )
        h.append("</table></details>")
    h.append("</body></html>")
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
