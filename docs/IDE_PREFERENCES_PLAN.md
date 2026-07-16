# Plan: JetBrains-style Preferences IDE in ratatui (zmax)

## Context

zmax is a ratatui TUI IDE that already has an IDE shell (project tree,
structure, problems, run window, git, minimap, toolbar) and — added this session —
a **Run/Debug Configurations** manager and a **Settings** page, both as ratatui
modal `Component`s with full mouse support. The goal is to grow this into a "full
blown JetBrains IDE": a unified **Preferences** window with a category sidebar
hosting **Appearance/Theme (full custom color-scheme editor)**, **Editor
settings**, **Keymap editor**, and **Run Configs**, all editable with mouse +
keyboard and applied live.

The linchpin that makes this real (not a toy): zmax already has a **runtime
config-reload path** — `:reload-config` → `ConfigEvent::Refresh` →
`Application::refresh_config()` (`application.rs:465`) re-applies theme +
keybindings + editor settings **without restart**. Every editor we build writes to
`~/.zmax/config.toml` (or `~/.zmax/themes/*.toml`) and triggers a live reload.

Decisions: **Unified Preferences window** (single modal, left category tree, right
pane swaps) and a **full custom theme editor** (per-scope color editing, live
preview, save to `~/.zmax/themes/<name>.toml`).

## Architecture

One modal `PreferencesPanel` Component (full-screen, pushed via `cx.push_layer`)
that owns a **left category list** + a **right content area**, delegating the
content area to one of several **pages**:

```rust
// zmax-term/src/ui/preferences/mod.rs
trait PrefPage {
    fn title(&self) -> &str;
    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context); // absolute rect
    fn handle_key(&mut self, key: KeyEvent) -> PageResult;
    fn handle_mouse(&mut self, col: u16, row: u16, kind: MouseEventKind) -> PageResult;
}
enum PageResult { Consumed, RunCallback(Callback), CloseRequested }
```

`PreferencesPanel` draws the bordered frame + sidebar, hit-tests the sidebar to
switch pages, and forwards all other events to the active page. Pages record their
own mouse hit-regions in **absolute** coordinates (the modal is full-screen, so the
page's `area` is a sub-rect — use `area.x/area.y` as origin, no offset translation).

**Reuse, do not reinvent:**
- Modal/ratatui/mouse pattern: `ui/run_config.rs`, `ui/settings.rs` (just built).
  Pull their bodies into pages. Render helpers: `crate::ui::rat::{render,
  render_stateful, to_rat_style}` — **`rat::render` takes `zmax_view::graphics::Rect`,
  not ratatui's**; do rect math in zmax `Rect`, no `Layout::split`/`Block::inner`.
- Mouse hit-testing MUST be column-aware `(row, x0, x1, idx)` (the "clicking Name
  changes the name" bug came from row-only testing). Check field hits before list.
- `Component`/`EventResult`/`Callback`/`compositor.{push,pop,find::<EditorView>}` —
  `compositor.rs`. `commands::Context` vs `compositor::Context` are distinct types.

**Entry points (toolbar + key + command):**
- New `ToolHit::Preferences` + a `⚙ Preferences` toolbar button (`ide.rs`
  `render_toolbar` ~1308, `ToolHit` ~109, click map ~464) →
  `IdeAction::Preferences` → `apply_ide_action` (`ui/editor.rs`) pushes the panel.
- Command `preferences` + keybinding `SPC ,` (JetBrains Cmd-,). Existing
  `settings_page` / `run_config_manager` commands open the panel on that page.

## Pages (build in this order)

### 1. Preferences shell + Editor-settings page + Run-Configs page
- New module `ui/preferences/` with `PreferencesPanel` + the `PrefPage` trait.
- **Editor page**: lift the `SETTINGS` schema + `toml::Value` get/set/save logic
  out of `ui/settings.rs` into `preferences/editor_page.rs` (Bool/Int/Str specs,
  `get_path`/`set_path`, persist to `~/.zmax/config.toml`). Add a couple of
  **enum dropdown** specs (`line-number` = absolute|relative, `bufferline` =
  always|multiple|never). **On save, trigger live reload** (see below).
- **Run Configs page**: wrap the existing `RunConfigPanel` body
  (`ui/run_config.rs`) as a page — its CRUD list/form/buttons already work.
- Keep `settings.rs`/`run_config.rs` types but route the standalone commands to
  open the unified panel on the right page.

### 2. Keymap editor page (`preferences/keymap_page.rs`)
- Enumerate current bindings with `KeyTrie::reverse_map()` (`keymap.rs:189`) over
  each mode (`config.keys`) → rows of `mode · chord · command`.
- Searchable, scrollable `List`. Select a row → "press new chord" capture mode
  (read a `KeyEvent`, render it via `input.rs` Display).
- **Rebind** = write `[keys.<mode>]` `"<chord>" = "<command>"` into
  `~/.zmax/config.toml` (`toml::Value`, preserving other keys), then live reload
  so `merge_keys` (`keymap.rs:371`) layers it over defaults.
- Show user overrides distinctly; allow reset (remove the `[keys.<mode>]` entry).

### 3. Appearance / custom color-scheme editor page (`preferences/theme_page.rs`)
- **Theme list** from `theme::Loader::read_names` over `~/.zmax/themes/` +
  `runtime/themes/` + built-ins (`commands/typed.rs:1142`).
- **Scope editor**: for the selected theme, list editable scopes (`ui.background`,
  `ui.text`, `keyword`, `function`, `string`, `comment`, `ui.selection`, …) with
  their current fg/bg swatches (from `Theme.styles`, `theme.rs:272`).
- **Color picker**: choose a scope → pick from the 16 named base colors + a hex
  input field; update the in-memory `Theme`.
- **Live preview**: reuse the existing **theme preview pane** (added in commit
  `ecd63dff48`) rendered inside the page with a sample snippet styled by the edited
  theme; also apply to the editor via `editor.set_theme` Preview (`editor.rs:1660`).
- **Save custom theme**: serialize the edited scope→style map to TOML and write
  `~/.zmax/themes/<name>.toml` (new `theme_io::save(name, &Theme)` — Theme isn't
  directly `Serialize`, build the `toml::Value` table from `Theme.styles`). Set
  `theme = "<name>"` in config.toml + live-apply via `set_theme`.

## Live-reload helper (shared by all pages)

```rust
// after writing config.toml
cx.editor.config_events.0.send(ConfigEvent::Refresh).ok(); // editor.rs:1432
```
For theme changes call `editor.set_theme(theme)` directly (already persists).
`refresh_config` re-reads `[keys.*]` + `[editor]` (per `application.rs:465`). Pages
reach the editor through the `Context` passed to render/event handling, or via a
`Callback` that gets `compositor::Context`.

## Files

**New:** `ui/preferences/mod.rs` (panel + `PrefPage` trait + sidebar),
`ui/preferences/editor_page.rs`, `ui/preferences/runconfig_page.rs`,
`ui/preferences/keymap_page.rs`, `ui/preferences/theme_page.rs`, `theme_io.rs`
(theme TOML serialize/save).

**Modify:** `ui/mod.rs` (module decl), `commands.rs` (`preferences` command +
register; repoint `settings_page`/`run_config_manager`), `keymap/vim.rs` (`SPC ,`
and a Preferences entry under `SPC S`), `ui/ide.rs` (`ToolHit::Preferences`,
toolbar button, click map), `ui/editor.rs` (`IdeAction::Preferences` dispatch).
Largely lift logic from existing `ui/settings.rs` + `ui/run_config.rs`.

## Verification

- `cargo build -p zmax-term` clean; `ZMAX_DISABLE_AUTO_GRAMMAR_BUILD=1 cargo
  test -p zmax-term --lib` green; `cargo install --path zmax-term --locked`.
- Open via `SPC ,` / `⚙ Preferences` button. Switch categories with mouse + j/k.
- Editor page: toggle soft-wrap, confirm `~/.zmax/config.toml` updates and applies
  live (no restart).
- Keymap page: rebind a key, confirm `[keys.normal]` written and the binding works
  immediately after reload.
- Theme page: edit a scope color, see live preview, save, confirm
  `~/.zmax/themes/<name>.toml` written and theme applied.
- pty smoke test (as used for run_config): open panel, click a sidebar category and
  a row, assert no panic and the backing file changed. Reuse the `/tmp/zemcrash`
  HOME-sandboxed pty harness from this session.
- Mouse: verify column-aware hit-testing (sidebar vs content vs form fields).

## Scope / phasing

Recommended sequence: **(1)** shell + Editor page + Run-Configs page (mostly lifting
existing code) → **(2)** Keymap editor → **(3)** custom Theme editor. Each phase
builds green, installs, and is independently usable. Phases 2–3 are the larger new
subsystems (key-capture + theme TOML serialization).
