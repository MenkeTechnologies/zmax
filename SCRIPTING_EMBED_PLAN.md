# Embedding 5 interpreters into zmax — implementation plan

Embed **elisprs** (Emacs Lisp), **zmax-viml/vimlrs** (Vimscript), **strykelang**
(Perl 5), **awkrs** (AWK) and **zshrs** (zsh) into the editor so all five can drive
zmax through one **uniform full IDE API**.

Decisions taken: dependencies wired as **git submodules / path deps** (editable, since
4 of 5 need source changes); **all five built in parallel**; every language gets the
**complete editor API** (commands, buffer mutation, keymaps, hooks, registers, options).

---

## 1. Current state (what the audit found)

### Editor seams (zmax-term / zmax-view) — ready

- **Command registry**: `TypableCommand { name, fun: fn(&mut compositor::Context, Args,
  PromptEvent) -> Result } ` in `commands/typed.rs`, indexed by `TYPABLE_COMMAND_MAP`;
  dispatched via `execute_command()` / `execute_command_line()`. Static motions are
  `MappableCommand::Static { fun: fn(&mut Context) }` in `commands.rs`.
- **The bridge object** every handler gets: `Context<'a> { editor: &mut Editor, jobs:
  &mut Jobs, callback, ... }` (`commands.rs:104`) — and `compositor::Context` which also
  carries the compositor/UI. This is the one handle a script needs.
- **State**: `Editor { documents: BTreeMap<DocumentId, Document>, registers, theme,
  config: Arc<dyn DynAccess<Config>>, .. }` (`zmax-view/editor.rs`); `Document { text:
  Rope, selections, history, .. }`. Mutation is via `Transaction` → `doc.apply()` (undoable).
- **Config**: `~/.zmax/config.toml` + workspace `.zmax/config.toml`, loaded in
  `main.rs` before `app.run()` — the natural init hook. `KeyTrie` keymaps live in `Config.keys`
  per `Mode`, runtime-swappable through `Arc<ArcSwap<Config>>`; `keymap::merge_keys`.
- **Precedent**: `run-shell-command` / `!` / `sh` already shells out (template for zsh).
- **No scripting exists yet** — clean slate. `WorkspaceTrust` already gates workspace config.

### Interpreter embedding surfaces

| Crate | ver | Entry API | Persists? | Host-callback (editor ops) | fusevm | Gap to close |
|---|---|---|---|---|---|---|
| **elisprs** | 0.1.1 | `eval_str`/`eval_file`, `with_host`, `defsubr(name,min,max,SubrFn)`, `reset_host`; `Value`=`fusevm::Value` | thread-local host ✅ | **clean** (`defsubr`) | 0.14.3 | none |
| **vimlrs** (`zmax-viml`, unpublished) | 0.1.1 | `eval_expr`/`eval_source` → `typval_T`, `capture_begin/take`, `eval_file` | thread-local globals/funcs ✅ | **none**; `:map`/`:command`/`:set`/`:autocmd` are TODO stubs (E1147) | 0.14.2 | wire ex-cmds + builtins → host (largest) |
| **strykelang** (`stryke`) | 0.17.33 | `VMHelper::new()`, `parse_and_run_string(code,&mut vm)` → `StrykeValue` | owned `&mut VMHelper` ✅ | only via `rust{}` rustc-compiled blocks | 0.14.0 (opt) | add native host-fn registration |
| **awkrs** | 0.4.14 | mostly private; CLI-shaped; `Value`, `Runtime::new` | per-run ✅ | none | 0.14.2 | add `run_program(prog,input)->String` + var get/set |
| **zshrs** (`zsh`) | 0.12.5 | `ShellExecutor::new()`, `execute_script(&str)->i32`, scalar/array/assoc get/set | persistent ✅ | writes **real fds**, no capture | 0.14.2 | add output capture / PTY route |

**Cross-cutting risk**: all five ride **fusevm**, on three different minor versions
(0.14.0/0.14.2/0.14.3). They must be unified on one version that also lives in the
zmax workspace, or Cargo will pull incompatible duplicates.

---

## 2. Central architecture

### 2.1 The thread-local editor bridge (the forced design)

Every interpreter exposes host callbacks as **bare `fn` pointers** and keeps its
host/state **thread-local**. A `fn` can't capture `&mut Editor`, so the editor is reached
through a thread-local raw pointer installed by an RAII guard for the duration of one
synchronous eval — the exact pattern awkrs (`RuntimeGuard`/`CURRENT_RT`), zshrs
(`CURRENT_EXECUTOR`) and elisprs (`HOST`) already use internally.

```rust
// zmax-term/src/scripting/bridge.rs
struct ScriptCx<'a> { cx: &'a mut compositor::Context<'a> } // editor + jobs + compositor
thread_local! { static CUR: Cell<*mut ()> = Cell::new(ptr::null_mut()); }
pub struct Guard(*mut ());                  // installs on new, clears on Drop, supports nesting (stack)
pub fn with_cx<R>(f: impl FnOnce(&mut compositor::Context) -> R) -> R; // used by every api primitive
```

Consequences (must be honored):
- **Eval runs on the editor thread**, synchronously, from inside a command handler that
  already holds `&mut compositor::Context`. No background eval of editor-mutating scripts.
- The raw pointer must never escape the guard scope; documented `unsafe`, asserted non-null.
- Nesting (a scripted command that evals again) handled by a guard stack.
- Long scripts block the UI → add a step budget / cooperative timeout; route blocking work
  (zsh fork/exec, perl heavy loops) to `Jobs`/async or a PTY.

### 2.2 Scripting host lives **in zmax-term** (not a new crate)

The uniform API needs `TYPABLE_COMMAND_MAP` + `Context` + `Editor`, all owned by
zmax-term. A separate crate would be circular. New module tree:

```
zmax-term/src/scripting/
  mod.rs      ScriptLang enum, ScriptEngine trait, registry, dispatch
  bridge.rs   ScriptCx + Guard + with_cx (§2.1)
  value.rs    ScriptValue + per-language From/Into
  api.rs      the uniform editor API primitives (§3) — defined once
  hooks.rs    event subscription + per-language callback dispatch (§3, item I)
  init.rs     locate + load init files in config order
  elisp.rs    bind api + marshal fusevm::Value  (upstream: ready)
  viml.rs     bind api + marshal typval_T        (upstream: ex-cmd/builtin host hooks)
  perl.rs     bind api + marshal StrykeValue      (upstream: host-fn registration)
  awk.rs      region filter + run_program         (upstream: run_program/capture)
  zsh.rs      embedded shell + capture            (upstream: output capture/PTY)
```

`ScriptEngine` trait: `eval_str(&str) -> Result<ScriptValue>`, `eval_file(&Path)`,
`call(fn_handle, &[ScriptValue])`, `reset()`. `ScriptValue` = nil/bool/int/float/string/
list/map/func-handle; each language module provides the marshalling.

---

## 3. The uniform editor API (the "host ABI")

One flat set of Rust primitives in `scripting/api.rs`, each operating through `with_cx`.
Every language module registers all of them under idiomatic names. Categories:

- **A. Command dispatch** — `cmd(name, args…)`: look up `TYPABLE_COMMAND_MAP` (and static
  commands) and run with the current cx. This alone exposes ~all editor features.
- **B. Buffer / text** — `buffer-string`, `buffer-substring(a,b)`, `insert(s)`,
  `delete-region(a,b)`, `replace-region(a,b,s)`, `goto(pos)`, `point`, `point-max`,
  `line-count`, `current-line`, `line-text(n)`. Mutations build one `Transaction` →
  `doc.apply()` so undo/redo and multi-cursor stay correct.
- **C. Point / mark / selection** — get/set primary selection + all selections, extend, set mode.
- **D. Buffers / files** — `find-file`, `current-buffer`, `buffer-list`, `switch-buffer`, `save-buffer`, `kill-buffer`.
- **E. Windows / views** — split, focus, close.
- **F. Registers** — get/set named registers.
- **G. Options / config** — `get-option`/`set-option` over `editor::Config` (and theme).
- **H. Keymap** — `define-key(mode, "keys", target)` where target is a command name *or* a
  script function handle; writes a **runtime keymap overlay** (§4, item 6).
- **I. Hooks / autocmds** — `add-hook(event, fn)`; events fire from a new `hooks.rs`
  subscribed to `zmax-event`: buffer-open, before/after-save, mode-change,
  selection-change, quit, etc. Each language maps its native form (`add-hook`,
  `:autocmd`, Perl/awk callbacks) onto it.
- **J. UI / messaging** — `message`, `error`, minibuffer `read-string`/`prompt`, set status.
- **K. Eval interop** — re-enter same engine; optionally call across languages by name.

The same names are wired per language (e.g. elisp `insert`, viml `setline()`/`feedkeys()`,
perl `Editor::insert`, awk via field/region semantics, zsh via builtins).

---

## 4. Editor-side integration tasks (zmax-term)

1. **Dependency wiring** — add the five crates as **git submodules under `zmax/vendor/`**
   (or path deps to the meta siblings) + path deps in `zmax-term/Cargo.toml`. Each crate
   declares its own standalone `[workspace]`; add them to the zmax workspace
   `exclude` list (or strip their `[workspace]`) so cargo doesn't auto-absorb them.
2. **fusevm unification** — pin one fusevm version across all five and add `fusevm` to the
   zmax `[workspace.dependencies]`. Bump the laggards (awkrs/vimlrs/zshrs 0.14.2,
   stryke 0.14.0) to elisprs's line; reconcile any API drift. **Gate-zero: nothing else
   compiles until this is clean.**
3. **`scripting/` module** — build §2.2 / §3.
4. **Commands** (`typed.rs`): `:eval`/`:elisp`/`:eval-expression`, `:viml`/`:vim`,
   `:perl`/`:stryke`, `:awk`, `:zsh` (+ route `!`); `:source <file>` dispatched by
   extension; region filters `:awk!`, `:perl!`, `:!` (pipe selection through a program).
5. **Init loading** — `Application::load_init_scripts()` called in `main.rs` after
   `Application::new()`, before `app.run()`. Loads, in config order, `~/.zmax/init.el`,
   `init.vim`, `init.pl`, `init.awk`, and sources `init.zsh` for shell env. New
   `[scripting]` config table: enabled langs, file paths, order, sandbox flags. Workspace
   init scripts gated behind existing `WorkspaceTrust`.
6. **Runtime keymap overlay** — add `editor.runtime_keymap` consulted before `Config.keys`;
   a new `MappableCommand::Script { lang, handle }` lets a key invoke a script callback.
   `define-key` (API item H) mutates this overlay.
7. **Hooks/events** — enumerate hookable events, fire them from editor lifecycle via
   `zmax-event`, dispatch to registered callbacks (`hooks.rs`).
8. **Undo grouping** — script-driven edits coalesce into one undo transaction per eval.

---

## 5. Per-language upstream work (in the submodules)

- **elisprs** — none required. Consume `defsubr` for all of §3; optionally add a batch
  register helper. Reference implementation that proves the design.
- **vimlrs** — *largest*. Implement the stubbed ex-command handlers (`:map`→define-key,
  `:set`→option, `:command`→typable, `:autocmd`→hook, `:normal`→feedkeys) to call host
  hooks; add a public host-callback registration so editor-touching builtins
  (`getline`/`setline`/`feedkeys`/`execute`) reach the API. Add `set_ex_host(impl ExHost)`.
- **strykelang** — add native host-fn registration: `vm.register_native(name, fn(&mut
  VMHelper, &[StrykeValue]) -> StrykeResult<StrykeValue>)` surfaced as a Perl sub (e.g. an
  `Editor::` package). Keep `rust{}` rustc-FFI **disabled** in the editor (arbitrary
  compile/exec) — use the host-fn path instead.
- **awkrs** — expose a clean embedding API: `Program::compile(src)` +
  `run_over_str(input)->String` (capture `print` via the existing `print_buf`), plus
  `set_var`/`get_var`. Powers region filters; optional host-fn hook for full API parity.
- **zshrs** — add `execute_script_capture(&mut self, src)->(i32,String,String)` (dup fds to
  pipes) and/or a PTY route, so output lands in a buffer instead of the real terminal.
  Reuse for `M-x shell` and `!`.

---

## 6. Risks / hard parts

- **fusevm version drift** (do first; blocks all).
- **UI-thread blocking**: thread-local hosts force on-thread eval; add budgets/timeouts;
  push fork/exec + heavy loops to Jobs/PTY.
- **zsh real-fd I/O inside a TUI** can corrupt the terminal — must capture/PTY.
- **vimlrs stubs** are substantial porting, not glue.
- **Arbitrary code execution** (zsh, perl, stryke FFI) — gate workspace init behind
  `WorkspaceTrust`; disable stryke `rust{}`.
- **Unsafe raw-pointer bridge** — guarantee no pointer escape; nesting via guard stack.
- **Borrow discipline** — mutating `documents` while a script iterates; batch into Transactions.

---

## 7. Sequencing (parallel tracks, with a forced order inside)

- **Phase 0 — Foundation (gate):** submodules + Cargo wiring (§4.1), fusevm unification (§4.2).
- **Phase 1 — Harness:** `scripting/` core — bridge, ScriptValue, `api.rs` primitives,
  ScriptEngine trait, init loader, `[scripting]` config, command skeletons. Wire **elisp**
  end-to-end (ready) as the reference proving the bridge + API.
- **Phase 2 — Five language tracks in parallel** (each = its §5 upstream gap + its
  `scripting/<lang>.rs` binding + value marshalling): elisp (API coverage), viml, perl,
  awk, zsh.
- **Phase 3 — Cross-cutting API:** runtime keymap overlay + `define-key` (H), hooks/events
  (I), undo grouping, M-x/minibuffer eval, region filters.
- **Phase 4 — Polish:** docs in `book/`, entries in the inline Help browser (`ui/help.rs`),
  per-language integration tests, sample `init.*` files, port-report update.

---

## 8. Acceptance checks

- `:elisp (insert "hi")` inserts text; `~/.zmax/init.el` loads at startup and its
  `defun`s persist for later `M-x`/`:eval`.
- `:vim :set number` and a `:command`/`:map`/`:autocmd` in `init.vim` actually take effect.
- `:perl Editor::insert("x")` and a Perl region filter (`:perl!`) transform the selection.
- `:awk!` filters the selection through an awk program with captured output.
- `:zsh ls` and `M-x shell` run via embedded zshrs with output in a buffer; cwd/env persist.
- A key bound from any language via `define-key` triggers its callback.
- All five share one fusevm version; `cargo build` is clean; per-language tests pass.
