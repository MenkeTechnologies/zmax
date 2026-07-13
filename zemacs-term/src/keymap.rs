pub mod default;
pub mod emacs;
pub mod macros;
pub mod major_mode;
pub mod spacemacs;
pub mod vim;
pub mod vim_map;

pub use crate::commands::MappableCommand;
// zemacs ships the spacemacs keymap as the default keymap that `Config` loads:
// vim/evil keys + the `SPC` leader + the Emacs `C-x` prefix. The pure-vim base
// is `vim::default`, and the selection-first keymap remains available as
// `default::default` (module path) for reference, but neither is the default.
pub use spacemacs::default;

/// The keymap preset names selectable via `keymap = "..."` in config.toml and
/// the `:keymap` command.
pub const PRESETS: &[&str] = &["spacemacs", "vim", "helix", "emacs"];

/// Resolve a named keymap preset to its base keybindings. Returns `None` for an
/// unknown name so callers can report it.
pub fn preset(name: &str) -> Option<HashMap<Mode, KeyTrie>> {
    match name {
        "spacemacs" => Some(spacemacs::default()),
        "vim" => Some(vim::default()),
        "helix" => Some(default::default()),
        "emacs" => Some(emacs::default()),
        _ => None,
    }
}

/// The mode the editor should start in for a keymap preset. Emacs is modeless
/// (you are always inserting), so it starts in Insert; the modal keymaps
/// (spacemacs, vim, helix) start in Normal.
pub fn default_mode(name: &str) -> Mode {
    match name {
        "emacs" => Mode::Insert,
        _ => Mode::Normal,
    }
}

use arc_swap::{
    access::{DynAccess, DynGuard},
    ArcSwap,
};
use indexmap::IndexMap;
use macros::key;
use serde::Deserialize;
use std::{
    borrow::Cow,
    collections::HashMap,
    ops::{Deref, DerefMut},
    sync::Arc,
};
use zemacs_view::{document::Mode, info::Info, input::KeyEvent};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct KeyTrieNode {
    /// A label for keys coming under this node, like "Goto mode"
    #[serde(skip)]
    name: String,
    #[serde(flatten)]
    map: IndexMap<KeyEvent, KeyTrie>,
    #[serde(skip)]
    pub is_sticky: bool,
    /// Spacemacs transient state: the command run at the moment this (sticky)
    /// node is entered, so that a single key both *acts* and *latches* the
    /// state — `SPC w [` shrinks the window and leaves you in the window
    /// transient state, where a bare `[` shrinks again. Without this, an entry
    /// key could only do one of the two.
    #[serde(skip)]
    pub on_enter: Option<MappableCommand>,
}

impl KeyTrieNode {
    pub fn new(name: &str, map: IndexMap<KeyEvent, KeyTrie>) -> Self {
        Self {
            name: name.to_string(),
            map,
            is_sticky: false,
            on_enter: None,
        }
    }

    /// This node as a transient-state entry point: sticky, and running `cmd`
    /// when entered. Used to bind several keys to the same transient state with
    /// a different opening action each (`SPC w [` / `]` / `{` / `}` …).
    pub fn transient_entry(&self, cmd: MappableCommand) -> Self {
        let mut node = self.clone();
        node.is_sticky = true;
        node.on_enter = Some(cmd);
        node
    }

    /// Merge another Node in. Leaves and subnodes from the other node replace
    /// corresponding keyevent in self, except when both other and self have
    /// subnodes for same key. In that case the merge is recursive.
    pub fn merge(&mut self, mut other: Self) {
        if other.on_enter.is_some() {
            self.on_enter = other.on_enter.take();
        }
        for (key, trie) in std::mem::take(&mut other.map) {
            if let Some(KeyTrie::Node(node)) = self.map.get_mut(&key) {
                if let KeyTrie::Node(other_node) = trie {
                    node.merge(other_node);
                    continue;
                }
            }
            self.map.insert(key, trie);
        }
    }

    pub fn infobox(&self) -> Info {
        // One `key : description` row per binding (Emacs `describe-bindings` /
        // Spacemacs which-key style) — do NOT collapse keys by shared description,
        // which used to smear a whole prefix map into one comma-joined line with an
        // empty description. Description is the command's doc, falling back to its
        // (dash-ized) name; a submap shows `+name`.
        let mut body: Vec<(String, String)> = Vec::with_capacity(self.len());
        for (&key, trie) in self.iter() {
            let desc = match trie {
                KeyTrie::MappableCommand(cmd) => {
                    if cmd.name() == "no_op" {
                        continue;
                    }
                    let doc = cmd.doc();
                    if doc.is_empty() {
                        cmd.name().replace('_', "-")
                    } else {
                        doc.to_string()
                    }
                }
                KeyTrie::Node(n) => {
                    if n.name.is_empty() {
                        "+prefix".to_string()
                    } else {
                        format!("+{}", n.name)
                    }
                }
                KeyTrie::Sequence(_) => "[key macro]".to_string(),
            };
            body.push((key.to_string(), desc));
        }
        body.sort_by(|a, b| a.0.cmp(&b.0));
        Info::new(self.name.clone(), &body)
    }
}

impl PartialEq for KeyTrieNode {
    fn eq(&self, other: &Self) -> bool {
        self.map == other.map
    }
}

impl Deref for KeyTrieNode {
    type Target = IndexMap<KeyEvent, KeyTrie>;

    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl DerefMut for KeyTrieNode {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.map
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeyTrie {
    MappableCommand(MappableCommand),
    Sequence(Vec<MappableCommand>),
    Node(KeyTrieNode),
}

impl<'de> Deserialize<'de> for KeyTrie {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(KeyTrieVisitor)
    }
}

struct KeyTrieVisitor;

impl<'de> serde::de::Visitor<'de> for KeyTrieVisitor {
    type Value = KeyTrie;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a command, list of commands, or sub-keymap")
    }

    fn visit_str<E>(self, command: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        command
            .parse::<MappableCommand>()
            .map(KeyTrie::MappableCommand)
            .map_err(E::custom)
    }

    fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
    where
        S: serde::de::SeqAccess<'de>,
    {
        let mut commands = Vec::new();
        while let Some(command) = seq.next_element::<String>()? {
            commands.push(
                command
                    .parse::<MappableCommand>()
                    .map_err(serde::de::Error::custom)?,
            )
        }

        // Prevent macro keybindings from being used in command sequences.
        // This is meant to be a temporary restriction pending a larger
        // refactor of how command sequences are executed.
        if commands
            .iter()
            .any(|cmd| matches!(cmd, MappableCommand::Macro { .. }))
        {
            return Err(serde::de::Error::custom(
                "macro keybindings may not be used in command sequences",
            ));
        }

        Ok(KeyTrie::Sequence(commands))
    }

    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: serde::de::MapAccess<'de>,
    {
        let mut mapping = IndexMap::new();
        while let Some((key, value)) = map.next_entry::<KeyEvent, KeyTrie>()? {
            mapping.insert(key, value);
        }
        Ok(KeyTrie::Node(KeyTrieNode::new("", mapping)))
    }
}

impl KeyTrie {
    pub fn reverse_map(&self) -> ReverseKeymap {
        // recursively visit all nodes in keymap
        fn map_node(cmd_map: &mut ReverseKeymap, node: &KeyTrie, keys: &mut Vec<KeyEvent>) {
            match node {
                KeyTrie::MappableCommand(MappableCommand::Macro { .. }) => {}
                KeyTrie::MappableCommand(cmd) => {
                    let name = cmd.name();
                    if name != "no_op" {
                        cmd_map.entry(name.into()).or_default().push(keys.clone())
                    }
                }
                KeyTrie::Node(next) => {
                    for (key, trie) in &next.map {
                        keys.push(*key);
                        map_node(cmd_map, trie, keys);
                        keys.pop();
                    }
                }
                KeyTrie::Sequence(_) => {}
            };
        }

        let mut res = HashMap::new();
        map_node(&mut res, self, &mut Vec::new());
        res
    }

    pub fn node(&self) -> Option<&KeyTrieNode> {
        match *self {
            KeyTrie::Node(ref node) => Some(node),
            KeyTrie::MappableCommand(_) | KeyTrie::Sequence(_) => None,
        }
    }

    pub fn node_mut(&mut self) -> Option<&mut KeyTrieNode> {
        match *self {
            KeyTrie::Node(ref mut node) => Some(node),
            KeyTrie::MappableCommand(_) | KeyTrie::Sequence(_) => None,
        }
    }

    /// Merge another KeyTrie in, assuming that this KeyTrie and the other
    /// are both Nodes. Panics otherwise.
    pub fn merge_nodes(&mut self, mut other: Self) {
        let node = std::mem::take(other.node_mut().unwrap());
        self.node_mut().unwrap().merge(node);
    }

    /// Descend a trie following the given path of keys
    pub fn search(&self, keys: &[KeyEvent]) -> Option<&KeyTrie> {
        let mut trie = self;
        for key in keys {
            trie = match trie {
                KeyTrie::Node(map) => map.get(key),
                // leaf encountered while keys left to process
                KeyTrie::MappableCommand(_) | KeyTrie::Sequence(_) => None,
            }?
        }
        Some(trie)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeymapResult {
    /// Needs more keys to execute a command. Contains valid keys for next keystroke.
    Pending(KeyTrieNode),
    Matched(MappableCommand),
    /// Matched a sequence of commands to execute.
    MatchedSequence(Vec<MappableCommand>),
    /// Key was not found in the root keymap
    NotFound,
    /// Key is invalid in combination with previous keys. Contains keys leading upto
    /// and including current (invalid) key.
    Cancelled(Vec<KeyEvent>),
}

/// A map of command names to keybinds that will execute the command.
pub type ReverseKeymap = HashMap<String, Vec<Vec<KeyEvent>>>;

pub struct Keymaps {
    pub map: Box<dyn DynAccess<HashMap<Mode, KeyTrie>>>,
    /// Stores pending keys waiting for the next key. This is relative to a
    /// sticky node if one is in use.
    state: Vec<KeyEvent>,
    /// Stores the sticky node if one is activated.
    pub sticky: Option<KeyTrieNode>,
}

impl Keymaps {
    pub fn new(map: Box<dyn DynAccess<HashMap<Mode, KeyTrie>>>) -> Self {
        Self {
            map,
            state: Vec::new(),
            sticky: None,
        }
    }

    pub fn map(&self) -> DynGuard<HashMap<Mode, KeyTrie>> {
        self.map.load()
    }

    /// Returns list of keys waiting to be disambiguated in current mode.
    pub fn pending(&self) -> &[KeyEvent] {
        &self.state
    }

    pub fn sticky(&self) -> Option<&KeyTrieNode> {
        self.sticky.as_ref()
    }

    pub fn contains_key(&self, mode: Mode, key: KeyEvent) -> bool {
        let keymaps = &*self.map();
        let keymap = &keymaps[&mode];
        keymap
            .search(self.pending())
            .and_then(KeyTrie::node)
            .is_some_and(|node| node.contains_key(&key))
    }

    /// Lookup `key` in the keymap to try and find a command to execute. Escape
    /// key cancels pending keystrokes. If there are no pending keystrokes but a
    /// sticky node is in use, it will be cleared.
    pub fn get(&mut self, mode: Mode, key: KeyEvent) -> KeymapResult {
        self.get_with_language(mode, key, None)
    }

    /// [`Keymaps::get`], with the focused document's `language` — its Emacs
    /// *major mode* (see [`major_mode`]). A chord bound in that language's
    /// overlay shadows the base keymap, exactly like an Emacs major-mode map
    /// shadows the global map; everything else falls through unchanged.
    pub fn get_with_language(
        &mut self,
        mode: Mode,
        key: KeyEvent,
        language: Option<&str>,
    ) -> KeymapResult {
        // TODO: remove the sticky part and look up manually
        let keymaps = &*self.map();
        let keymap = &keymaps[&mode];

        if key!(Esc) == key {
            if !self.state.is_empty() {
                // Note that Esc is not included here
                return KeymapResult::Cancelled(self.state.drain(..).collect());
            }
            self.sticky = None;
        }

        // A sticky node is a transient state (`SPC w`, `SPC n`, …): it is
        // self-contained, so the major-mode overlay does not apply inside it.
        if self.sticky.is_none() {
            if let Some(overlay) = language.and_then(|lang| major_mode::overlay(lang, mode)) {
                let mut path = self.state.clone();
                path.push(key);
                match overlay.search(&path) {
                    Some(KeyTrie::MappableCommand(cmd)) => {
                        self.state.clear();
                        return KeymapResult::Matched(cmd.clone());
                    }
                    Some(KeyTrie::Sequence(cmds)) => {
                        self.state.clear();
                        return KeymapResult::MatchedSequence(cmds.clone());
                    }
                    Some(KeyTrie::Node(overlay_node)) => {
                        // A major-mode *prefix* may only open where the base map
                        // already has one: in the `vim` preset `C-c` is escape and
                        // in `helix` it is toggle-comments, and turning either into
                        // a prefix would strand the key. (Where the base has a
                        // prefix — `spacemacs`, `emacs` — the overlay merges into
                        // it, so the global chords it does not bind still work.)
                        let base = keymap.search(&path);
                        if path.len() > 1 || matches!(base, Some(KeyTrie::Node(_)) | None) {
                            let node = match base {
                                Some(KeyTrie::Node(base_node)) => {
                                    let mut node = base_node.clone();
                                    node.merge(overlay_node.clone());
                                    node
                                }
                                _ => overlay_node.clone(),
                            };
                            self.state.push(key);
                            return KeymapResult::Pending(node);
                        }
                    }
                    None => {}
                }
            }
        }

        let first = self.state.first().unwrap_or(&key);
        let trie_node = match self.sticky {
            Some(ref trie) => Cow::Owned(KeyTrie::Node(trie.clone())),
            None => Cow::Borrowed(keymap),
        };

        let trie = match trie_node.search(&[*first]) {
            Some(KeyTrie::MappableCommand(ref cmd)) => {
                return KeymapResult::Matched(cmd.clone());
            }
            Some(KeyTrie::Sequence(ref cmds)) => {
                return KeymapResult::MatchedSequence(cmds.clone());
            }
            None => return KeymapResult::NotFound,
            Some(t) => t,
        };

        self.state.push(key);
        match trie.search(&self.state[1..]) {
            Some(KeyTrie::Node(map)) => {
                if map.is_sticky {
                    self.state.clear();
                    self.sticky = Some(map.clone());
                    // Transient-state entry key: latch the state *and* run its
                    // opening command (spacemacs `SPC w [`, `SPC n +`, …).
                    if let Some(cmd) = map.on_enter.clone() {
                        return KeymapResult::Matched(cmd);
                    }
                }
                KeymapResult::Pending(map.clone())
            }
            Some(KeyTrie::MappableCommand(cmd)) => {
                self.state.clear();
                KeymapResult::Matched(cmd.clone())
            }
            Some(KeyTrie::Sequence(cmds)) => {
                self.state.clear();
                KeymapResult::MatchedSequence(cmds.clone())
            }
            None => KeymapResult::Cancelled(self.state.drain(..).collect()),
        }
    }
}

impl Default for Keymaps {
    fn default() -> Self {
        // zemacs ships the spacemacs keymap as the default (see keymap/spacemacs.rs).
        Self::new(Box::new(ArcSwap::new(Arc::new(spacemacs::default()))))
    }
}

/// Merge default config keys with user overwritten keys for custom user config.
pub fn merge_keys(dst: &mut HashMap<Mode, KeyTrie>, mut delta: HashMap<Mode, KeyTrie>) {
    for (mode, keys) in dst {
        keys.merge_nodes(
            delta
                .remove(mode)
                .unwrap_or_else(|| KeyTrie::Node(KeyTrieNode::default())),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::macros::keymap;
    use super::*;
    use crate::commands::MappableCommand;
    use arc_swap::access::Constant;
    use indexmap::indexmap;
    use zemacs_core::hashmap;
    use zemacs_view::input::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn all_presets_build() {
        // Each named preset must build without panicking (this exercises every
        // key-string and `:typable` parse, e.g. emacs's `A-<` and `C-x C-s`) and
        // define all three editor modes.
        for name in PRESETS {
            let km = preset(name).unwrap_or_else(|| panic!("missing preset `{name}`"));
            assert!(km.contains_key(&Mode::Normal), "{name}: no Normal mode");
            assert!(km.contains_key(&Mode::Select), "{name}: no Select mode");
            assert!(km.contains_key(&Mode::Insert), "{name}: no Insert mode");
        }
        assert!(preset("nope").is_none());
        assert_eq!(default_mode("emacs"), Mode::Insert);
        assert_eq!(default_mode("vim"), Mode::Normal);
    }

    #[test]
    fn no_vim_only_commands_leak_into_emacs_or_helix() {
        // Vim-specific commands must never appear in the `emacs`/`helix` presets —
        // those modes have their own models and vim keybindings must not leak in.
        // Pins the separation as the vim keymap grows (walks sequences too).
        fn collect(trie: &KeyTrie, out: &mut std::collections::HashSet<String>) {
            match trie {
                KeyTrie::MappableCommand(cmd) => {
                    out.insert(cmd.name().to_string());
                }
                KeyTrie::Sequence(cmds) => {
                    for c in cmds {
                        out.insert(c.name().to_string());
                    }
                }
                KeyTrie::Node(node) => {
                    for t in node.map.values() {
                        collect(t, out);
                    }
                }
            }
        }
        const VIM_ONLY: &[&str] = &[
            "search_next_vim",
            "search_prev_vim",
            "extend_search_next_vim",
            "extend_search_prev_vim",
            "select_gn_match",
            "select_gn_match_prev",
            "select_paragraph_forward_vim",
            "select_paragraph_backward_vim",
            "select_paragraph_forward_vim_linewise",
            "select_paragraph_backward_vim_linewise",
            "block_insert",
            "block_append",
            "goto_older_change",
            "goto_newer_change",
            "reflow_selections_keep_cursor",
            "delete_chars_forward_vim",
        ];
        for name in ["emacs", "helix"] {
            let km = preset(name).unwrap();
            let mut names = std::collections::HashSet::new();
            for trie in km.values() {
                collect(trie, &mut names);
            }
            for cmd in VIM_ONLY {
                assert!(
                    !names.contains(*cmd),
                    "{name}: vim-only command `{cmd}` leaked into the keymap"
                );
            }
        }
    }

    #[test]
    #[should_panic]
    fn duplicate_keys_should_panic() {
        keymap!({ "Normal mode"
            "i" => normal_mode,
            "i" => goto_definition,
        });
    }

    #[test]
    fn check_duplicate_keys_in_default_keymap() {
        // will panic on duplicate keys, assumes that `Keymaps` uses keymap! macro
        Keymaps::default();
    }

    #[test]
    fn merge_partial_keys() {
        let keymap = hashmap! {
            Mode::Normal => keymap!({ "Normal mode"
                "i" => normal_mode,
                "无" => insert_mode,
                "z" => jump_backward,
                "g" => { "Merge into goto mode"
                    "$" => goto_line_end,
                    "g" => delete_char_forward,
                },
            })
        };
        let mut merged_keyamp = default();
        merge_keys(&mut merged_keyamp, keymap.clone());
        assert_ne!(keymap, merged_keyamp);

        let mut keymap = Keymaps::new(Box::new(Constant(merged_keyamp.clone())));
        assert_eq!(
            keymap.get(Mode::Normal, key!('i')),
            KeymapResult::Matched(MappableCommand::normal_mode),
            "Leaf should replace leaf"
        );
        assert_eq!(
            keymap.get(Mode::Normal, key!('无')),
            KeymapResult::Matched(MappableCommand::insert_mode),
            "New leaf should be present in merged keymap"
        );
        // Assumes that z is a node in the default keymap
        assert_eq!(
            keymap.get(Mode::Normal, key!('z')),
            KeymapResult::Matched(MappableCommand::jump_backward),
            "Leaf should replace node"
        );

        let keymap = merged_keyamp.get_mut(&Mode::Normal).unwrap();
        // Assumes that `g` is a node in default keymap
        assert_eq!(
            keymap.search(&[key!('g'), key!('$')]).unwrap(),
            &KeyTrie::MappableCommand(MappableCommand::goto_line_end),
            "Leaf should be present in merged subnode"
        );
        // Assumes that `gg` is in default keymap
        assert_eq!(
            keymap.search(&[key!('g'), key!('g')]).unwrap(),
            &KeyTrie::MappableCommand(MappableCommand::delete_char_forward),
            "Leaf should replace old leaf in merged subnode"
        );
        // Assumes that `ge` is in default keymap. The merge above does not touch
        // `ge`, so it keeps the vim binding (back to end of previous word).
        assert_eq!(
            keymap.search(&[key!('g'), key!('e')]).unwrap(),
            &KeyTrie::MappableCommand(MappableCommand::vim_move_prev_word_end),
            "Old leaves in subnode should be present in merged node"
        );

        assert!(
            merged_keyamp
                .get(&Mode::Normal)
                .and_then(|key_trie| key_trie.node())
                .unwrap()
                .len()
                > 1
        );
        assert!(!merged_keyamp
            .get(&Mode::Insert)
            .and_then(|key_trie| key_trie.node())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn order_should_be_set() {
        let keymap = hashmap! {
            Mode::Normal => keymap!({ "Normal mode"
                "space" => { ""
                    "s" => { ""
                        "v" => vsplit,
                        "z" => hsplit,
                    },
                },
            })
        };
        let mut merged_keymap = default();
        merge_keys(&mut merged_keymap, keymap.clone());
        assert_ne!(keymap, merged_keymap);
        let keymap = merged_keymap.get_mut(&Mode::Normal).unwrap();
        // Make sure mapping works
        assert_eq!(
            keymap.search(&[key!(' '), key!('s'), key!('v')]).unwrap(),
            &KeyTrie::MappableCommand(MappableCommand::vsplit),
            "Leaf should be present in merged subnode"
        );
        // Merged nodes are appended at the end. The vim default already has a
        // `SPC s` (search) submap, so the freshly merged `v`/`z` land last.
        let node = keymap.search(&[key!(' '), key!('s')]).unwrap();
        let keys = node.node().unwrap().keys().copied().collect::<Vec<_>>();
        assert_eq!(
            &keys[keys.len() - 2..],
            &[key!('v'), key!('z')],
            "newly merged keys should be ordered at the end"
        );
    }

    /// The static command a leaf names, if it is one.
    fn cmd_name_of(trie: &KeyTrie) -> Option<&str> {
        match trie {
            KeyTrie::MappableCommand(MappableCommand::Static { name, .. }) => Some(name),
            _ => None,
        }
    }

    #[test]
    fn aliased_modes_are_same_in_default_keymap() {
        let keymaps = Keymaps::default().map();
        let root = keymaps.get(&Mode::Normal).unwrap();
        // `SPC w` and vim's `C-w` reach the same window menu, but `C-w` also
        // carries vim-specific window idioms (`C-w ]` goto-definition, `C-w }`
        // hover, `C-w ^` alternate-file, `C-w T` window-to-tab, …) that have no
        // place under the spacemacs leader. So `C-w` is a superset: every `SPC w`
        // binding must appear identically under `C-w` — EXCEPT on the keys where
        // vim and spacemacs disagree about what the key means. There, vim's
        // meaning is what `C-w` must keep (it is vim's prefix), and spacemacs's is
        // what `SPC w` must carry (it is spacemacs's leader); forcing them equal
        // would mean one of the two editors is simply not ported on that key.
        const VIM_OWNS: &[char] = &[
            // vim `C-w c` closes the window; spacemacs `SPC w c` is the
            // centered-cursor prefix (`SPC w c c` / `SPC w c .`).
            'c',
            // vim `C-w d` splits and jumps to the definition under the cursor;
            // spacemacs `SPC w d` deletes the window.
            'd',
        ];
        let spc_w = root
            .search(&[key!(' '), key!('w')])
            .unwrap()
            .node()
            .unwrap();
        let ctrl_w = root
            .search(&["C-w".parse::<KeyEvent>().unwrap()])
            .unwrap()
            .node()
            .unwrap();
        for (key, trie) in spc_w.iter() {
            if matches!(key.code, zemacs_view::keyboard::KeyCode::Char(c) if VIM_OWNS.contains(&c))
            {
                continue;
            }
            assert_eq!(
                ctrl_w.get(key),
                Some(trie),
                "SPC w {key:?} and C-w {key:?} must map to the same window command"
            );
        }
        // The divergent keys still have to mean the right thing on each side.
        assert_eq!(
            ctrl_w.get(&key!('c')).and_then(cmd_name_of),
            Some("wclose"),
            "C-w c stays vim's close-window"
        );
        assert!(
            matches!(spc_w.get(&key!('c')), Some(KeyTrie::Node(_))),
            "SPC w c is spacemacs's centered-cursor prefix"
        );
        assert_eq!(
            ctrl_w.get(&key!('d')).and_then(cmd_name_of),
            Some("xref_find_definitions_other_window"),
            "C-w d stays vim's split-and-goto-definition"
        );
        assert_eq!(
            spc_w.get(&key!('d')).and_then(cmd_name_of),
            Some("wclose"),
            "SPC w d stays spacemacs's delete-window"
        );
        // Note: zemacs ships the vim keymap, which intentionally does NOT alias
        // `z` and `Z` (vim reserves `Z` for `ZZ`/`ZQ`), so the Zemacs `z`==`Z`
        // view-mode invariant does not apply here.
    }

    #[test]
    fn reverse_map() {
        let normal_mode = keymap!({ "Normal mode"
            "i" => insert_mode,
            "g" => { "Goto"
                "g" => goto_file_start,
                "e" => goto_file_end,
            },
            "j" | "k" => move_line_down,
        });
        let keymap = normal_mode;
        let mut reverse_map = keymap.reverse_map();

        // sort keybindings in order to have consistent tests
        // HashMaps can be compared but we can still get different ordering of bindings
        // for commands that have multiple bindings assigned
        for v in reverse_map.values_mut() {
            v.sort()
        }

        assert_eq!(
            reverse_map,
            HashMap::from([
                ("insert_mode".to_string(), vec![vec![key!('i')]]),
                (
                    "goto_file_start".to_string(),
                    vec![vec![key!('g'), key!('g')]]
                ),
                (
                    "goto_file_end".to_string(),
                    vec![vec![key!('g'), key!('e')]]
                ),
                (
                    "move_line_down".to_string(),
                    vec![vec![key!('j')], vec![key!('k')]]
                ),
            ]),
            "Mismatch"
        )
    }

    /// Deserialize into KeyTrieNode
    #[test]
    fn deserialize_node() {
        let keys = r#"
"+" = "select_all"
a = "append_mode"
        "#;
        let expectation = KeyTrie::Node(KeyTrieNode::new(
            "",
            indexmap! {
                key!('+') => KeyTrie::MappableCommand(
                    MappableCommand::select_all
                ),
                key!('a') => KeyTrie::MappableCommand(
                    MappableCommand::append_mode
                ),
            },
        ));

        assert_eq!(toml::from_str(keys), Ok(expectation));

        // Other fields in KeyTrieNode CANNOT be deserialized
        let invalid = r#"
name = "name"
is_sticky = false
        "#;
        let result = toml::from_str::<KeyTrieNode>(invalid);
        assert!(result.is_err_and(|error| error.message().contains("Invalid key code 'is_sticky'")));
    }

    #[test]
    fn escaped_keymap() {
        let keys = r#"
"+" = [
    "select_all",
    ":pipe sed -E 's/\\s+$//g'",
]
        "#;

        let key = KeyEvent {
            code: KeyCode::Char('+'),
            modifiers: KeyModifiers::NONE,
        };

        let expectation = KeyTrie::Node(KeyTrieNode::new(
            "",
            indexmap! {
                key => KeyTrie::Sequence(vec!{
                    MappableCommand::select_all,
                    MappableCommand::Typable {
                        name: "pipe".to_string(),
                        args: "sed -E 's/\\s+$//g'".to_string(),
                        doc: "".to_string(),
                    },
                })
            },
        ));

        assert_eq!(toml::from_str(keys), Ok(expectation));
    }
}
