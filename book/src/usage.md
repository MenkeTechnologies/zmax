# Using Zmax

For a full interactive introduction to Zmax, refer to the
[tutor](https://github.com/MenkeTechnologies/zmax/blob/master/runtime/tutor) which
can be accessed via the command `zmax --tutor` or `:tutor`.

> 💡 Currently, not all functionality is fully documented, please refer to the
> [key mappings](./keymap.md) list.

## Modes

In the modal keymap presets (`spacemacs`, `vim`, `helix`), Zmax has different modes for different tasks. The `emacs` preset is modeless: it launches directly in insert mode, has no normal mode, and uses Emacs chords (`Ctrl-x`, `Ctrl-c`, `Meta`) instead. The main modes of the modal presets are:

* [Normal mode](./keymap.md#normal-mode): For navigation and editing commands. This is the mode the modal presets launch in.
* [Insert mode](./keymap.md#insert-mode): For typing text directly into the document. Access by typing `i` in normal mode.
* [Select/extend mode](./keymap.md#select--extend-mode): For making selections and performing operations on them. Access by typing `v` in normal mode.

## Buffers

Buffers are in-memory representations of files. You can have multiple buffers open at once. Use [pickers](./pickers.md) or commands like `:buffer-next` and `:buffer-previous` to open buffers or switch between them.

## Editing model

Zmax ships several [keymap presets](./configuration.md#keymap-presets). The default (`spacemacs`) and the `vim` preset use Vim's **operator → motion** model: you press an action such as `d`, `c` or `y` and then the motion or text object it applies to (`dw`, `ciw`, `yy`). If you come from Vim, see [Migrating from Vim](./from-vim.md).

The `helix` preset instead uses the Kakoune-inspired **selection → action** model: whatever you are going to act on (a word, a paragraph, a line, etc.) is selected first and the action (delete, change, yank, …) comes second. A cursor is simply a single-width selection.

## Multiple selections

Also inspired by Kakoune, multiple selections are a core mode of interaction in Zmax. For example, the standard way of replacing multiple instances of a word is to first select all instances (so there is one selection per instance) and then use the change action (`c`) to edit them all at the same time.

## Motions

Motions are commands that move the cursor or modify selections. They're used for navigation and text manipulation. Examples include `w` to move to the next word, or `f` to find a character. See the [Movement](./keymap.md#movement) section of the keymap for more motions.

## Wildfire

In the modal presets, `<ret>` (Enter) in normal mode selects the closest text object around the cursor; pressing it again grows the selection to the next enclosing object (word → pair → larger pair → …). `<backspace>` shrinks back to the previous selection. This is the "expand selection by syntax" workflow without picking an explicit text object each time.

## Snippets

A snippet is a template with tab stops written in LSP snippet syntax — `${1:default}` for the Nth stop with a placeholder, `$0` for where the cursor finally rests, and a number repeated (e.g. `${1}`) to mirror an edit at several sites at once.

Snippets reach you two ways:

* **Language-server snippets** appear as completion candidates; accept one and it expands with the tab stops queued.
* **Your own library** is edited with `:snippets`, which opens a panel for creating, editing, and deleting reusable snippets (each with a trigger word, a scope — a language name, or `*`/empty for every language — a description, and a body). The library is saved to `snippets.toml` in your config directory.

To use a library snippet, type its trigger word and press `Tab`: if the word before the cursor matches a trigger whose scope applies to the current language, the body expands in place (user triggers take priority over emmet). Once a snippet is active, `Tab` and `Shift-Tab` walk forward and backward through its tab stops.

## Hex editing

`:hex` (also `:hexview`/`:hexedit`) opens a byte-faithful, xxd-style view of a file's raw bytes — an offset column, the hex bytes, and the ASCII rendering side by side. It takes an optional path and otherwise uses the current buffer's file. Binary files opened the normal way are routed here automatically rather than rejected. Press `i`/`R` to edit, type into the hex or ASCII column (`Tab` toggles between them), `Ctrl-s` to write the raw bytes back, and `q` to close.

## Merge conflicts

When a file has git merge conflicts, `:merge` (also `:resolve`) opens a three-pane resolver — *ours* on the left, the *result* you are building in the middle, *theirs* on the right, with a diff3 *base* pane — modeled on JetBrains' merge tool, with inline character highlighting and horizontal scrolling. `]n` and `[n` jump between conflict markers, and, in the modal presets, the space-`g` git menu resolves them: `SPC g m` (or `SPC g c r`) opens the resolver, `SPC g c O`/`SPC g c T` take all of our/their side, and `SPC g =` shows a read-only diff of the buffer against git `HEAD` (`:diff`). The per-conflict typable commands `:conflict-ours`/`:conflict-theirs`/`:conflict-both` resolve the conflict at the cursor.

