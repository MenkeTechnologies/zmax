//! Emacs `shortdoc` (`shortdoc.el`) — "an overview of functions relevant for a
//! particular topic".
//!
//! `M-x shortdoc` asks for an area of interest (`string`, `list`, `file`, …) and
//! pops a buffer where the functions that handle it are listed with a one-line
//! description and an example call. zmax's callable surface is its commands, so a
//! group here is an area of editing and its entries are the commands that do it,
//! each with the example the user would actually type.
//!
//! Only the grouping and the examples are written down: every description is read
//! from the live command tables when the buffer is rendered, so a doc string can
//! never go stale here. A group entry naming a command that no longer exists is a
//! test failure (`groups_only_name_real_commands`), not a silently empty line.

use crate::commands::{typed::TYPABLE_COMMAND_LIST, MappableCommand};

/// One listed command: the name as the command tables spell it (a leading `:` for
/// a typable command) and the example invocation shown under it.
pub struct Entry {
    pub command: &'static str,
    pub example: &'static str,
}

/// One area of interest.
pub struct Group {
    pub name: &'static str,
    pub desc: &'static str,
    pub entries: &'static [Entry],
}

macro_rules! entries {
    ($($cmd:literal => $ex:literal),* $(,)?) => {
        &[$(Entry { command: $cmd, example: $ex }),*]
    };
}

pub const GROUPS: &[Group] = &[
    Group {
        name: "buffer",
        desc: "Switching, listing and closing open buffers",
        entries: entries! {
            "buffer_picker" => "SPC b b — pick a buffer by name",
            "goto_next_buffer" => "]b — the next buffer in the list",
            "goto_previous_buffer" => "[b — the previous buffer",
            ":buffer-close" => ":bc — close the current buffer",
            ":buffer-close-others" => ":bco — close every other buffer",
            ":buffer-next" => ":bn — the next buffer",
        },
    },
    Group {
        name: "file",
        desc: "Finding, saving and reloading files",
        entries: entries! {
            "file_picker" => "SPC f f — fuzzy-find a file in the workspace",
            ":write" => "C-x C-s or :w path — write the buffer, optionally under a new name",
            ":write-quit" => ":wq — write and close the window",
            ":reload" => ":reload — re-read the file from disk",
            ":new" => ":new — a scratch buffer",
        },
    },
    Group {
        name: "window",
        desc: "Splitting, moving between and closing windows",
        entries: entries! {
            "hsplit" => "C-w s — split horizontally",
            "vsplit" => "C-w v — split vertically",
            "jump_view_left" => "C-w h — focus the window to the left",
            "wclose" => "C-w q — close this window",
            "wonly" => "C-w o — close every other window",
        },
    },
    Group {
        name: "search",
        desc: "Searching this buffer and the whole workspace",
        entries: entries! {
            "search" => "/pattern — incremental regex search",
            "search_next" => "n — the next match",
            "search_selection" => "* — search for the selected text",
            "global_search" => "SPC / — regex search across the workspace",
            "select_regex" => "s pattern — select every match inside the selection",
            ":substitute" => ":%s/old/new/g — substitute throughout the buffer",
        },
    },
    Group {
        name: "edit",
        desc: "Changing text: undo, yank, paste, comment, format",
        entries: entries! {
            "undo" => "u — undo the last change",
            "redo" => "C-r — redo it",
            "yank" => "y — yank the selection",
            "paste_after" => "p — paste after the selection",
            "toggle_comments" => "SPC c — comment or uncomment the lines",
            "format_selections" => "= — format the selection with the language server",
            "increment" => "C-a — increment the number under the cursor",
        },
    },
    Group {
        name: "selection",
        desc: "Building and pruning multiple selections",
        entries: entries! {
            "split_selection" => "S pattern — split the selection on a regex",
            "split_selection_on_newline" => "A-s — one selection per line",
            "keep_primary_selection" => ", — drop all but the primary selection",
            "collapse_selection" => ";  — collapse each selection to its cursor",
            "align_selections" => "& — align the selections in a column",
            "join_selections" => "J — join the selected lines",
        },
    },
    Group {
        name: "lsp",
        desc: "Language-server navigation and refactoring",
        entries: entries! {
            "goto_definition" => "gd — jump to the definition",
            "goto_reference" => "gr — list the references",
            "hover" => "SPC k — documentation for the item under the cursor",
            "rename_symbol" => "SPC r — rename it everywhere",
            "code_action" => "SPC a — the code actions available here",
            "diagnostics_picker" => "SPC d — this file's diagnostics",
        },
    },
    Group {
        name: "shell",
        desc: "Running shell commands over the buffer",
        entries: entries! {
            "shell_pipe" => "| cmd — pipe each selection through cmd",
            "shell_insert_output" => "! cmd — insert cmd's output before the selection",
            "shell_append_output" => "A-! cmd — insert it after",
            "shell_keep_pipe" => "SPC & cmd — keep the selections cmd succeeds on",
            ":run-shell-command" => ":sh cmd — run cmd and show its output",
        },
    },
    Group {
        name: "register",
        desc: "Registers, marks and macros",
        entries: entries! {
            "select_register" => "\" a — use register a for the next yank or paste",
            "record_macro" => "q a — record a macro into register a",
            "replay_macro" => "@ a — replay it",
            "save_selection" => "C-s — push the selection onto the jumplist",
            "jump_backward" => "C-o — back along the jumplist",
        },
    },
];

/// The group named `name`, if there is one.
pub fn group(name: &str) -> Option<&'static Group> {
    GROUPS.iter().find(|g| g.name.eq_ignore_ascii_case(name))
}

/// Every group name — the completion table of the `shortdoc` prompt.
pub fn group_names() -> Vec<String> {
    GROUPS.iter().map(|g| g.name.to_string()).collect()
}

/// The documentation string a command name resolves to right now: a `:`-prefixed
/// name is looked up in the typable table, everything else in the static one.
pub fn doc_of(command: &str) -> Option<&'static str> {
    match command.strip_prefix(':') {
        Some(name) => TYPABLE_COMMAND_LIST
            .iter()
            .find(|c| c.name == name || c.aliases.contains(&name))
            .map(|c| c.doc),
        None => MappableCommand::STATIC_COMMAND_LIST
            .iter()
            .find(|c| c.name() == command)
            .map(|c| c.doc()),
    }
}

/// The `*Shortdoc <group>*` buffer text: every command in the group with its live
/// documentation and the example call.
pub fn render(group: &Group) -> String {
    let mut out = format!("Shortdoc — {} ({})\n\n", group.name, group.desc);
    for e in group.entries {
        let doc = doc_of(e.command).unwrap_or("(command not found)");
        out.push_str(&format!("{}\n  {}\n  {}\n\n", e.command, doc, e.example));
    }
    out.push_str("Areas: ");
    out.push_str(&group_names().join("  "));
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_only_name_real_commands() {
        let missing: Vec<&str> = GROUPS
            .iter()
            .flat_map(|g| g.entries.iter())
            .filter(|e| doc_of(e.command).is_none())
            .map(|e| e.command)
            .collect();
        assert!(
            missing.is_empty(),
            "shortdoc groups name commands that do not exist: {missing:?}"
        );
    }

    #[test]
    fn render_lists_every_entry_with_its_live_doc() {
        let g = group("file").expect("the file group exists");
        let text = render(g);
        for e in g.entries {
            assert!(text.contains(e.command), "{} is listed", e.command);
            assert!(text.contains(e.example));
            assert!(text.contains(doc_of(e.command).unwrap()));
        }
        assert!(group("no-such-area").is_none());
    }
}
