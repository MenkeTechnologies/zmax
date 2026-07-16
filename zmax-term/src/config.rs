use crate::keymap;
use crate::keymap::{merge_keys, KeyTrie};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs;
use std::io::Error as IOError;
use toml::de::Error as TomlError;
use zmax_loader::merge_toml_values;
use zmax_view::{document::Mode, theme};

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub theme: Option<theme::Config>,
    pub keys: HashMap<Mode, KeyTrie>,
    pub editor: zmax_view::editor::Config,
    /// Selected keymap preset name ("spacemacs" | "vim" | "helix" | "emacs").
    /// `keys` is this preset with any `[keys]` overrides merged on top. Drives the
    /// startup mode (emacs starts in Insert) and the `:keymap` command's current value.
    pub keymap: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigRaw {
    pub theme: Option<theme::Config>,
    pub keys: Option<HashMap<Mode, KeyTrie>>,
    pub editor: Option<toml::Value>,
    /// Base keymap preset: "spacemacs" (default), "vim", "helix", or "emacs".
    pub keymap: Option<String>,
}

/// Default keymap preset when none is configured.
pub const DEFAULT_KEYMAP: &str = "spacemacs";

/// Resolve a keymap preset name to its base bindings, warning + falling back to
/// the default preset on an unknown name.
fn keymap_base(name: &str) -> HashMap<Mode, KeyTrie> {
    keymap::preset(name).unwrap_or_else(|| {
        log::warn!("unknown keymap preset `{name}`, falling back to `{DEFAULT_KEYMAP}`");
        keymap::preset(DEFAULT_KEYMAP).unwrap_or_else(keymap::default)
    })
}

impl Default for Config {
    fn default() -> Config {
        Config {
            theme: None,
            keys: keymap::default(),
            editor: zmax_view::editor::Config::default(),
            keymap: DEFAULT_KEYMAP.to_string(),
        }
    }
}

#[derive(Debug)]
pub enum ConfigLoadError {
    BadConfig(TomlError),
    Error(IOError),
}

impl Default for ConfigLoadError {
    fn default() -> Self {
        ConfigLoadError::Error(IOError::new(std::io::ErrorKind::NotFound, "place holder"))
    }
}

impl Display for ConfigLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigLoadError::BadConfig(err) => err.fmt(f),
            ConfigLoadError::Error(err) => err.fmt(f),
        }
    }
}

impl Config {
    pub fn load(
        global: Result<&String, ConfigLoadError>,
        local: Result<String, ConfigLoadError>,
    ) -> Result<Config, ConfigLoadError> {
        let global_config: Result<ConfigRaw, ConfigLoadError> =
            global.and_then(|file| toml::from_str(file).map_err(ConfigLoadError::BadConfig));
        let local_config: Result<ConfigRaw, ConfigLoadError> =
            local.and_then(|file| toml::from_str(&file).map_err(ConfigLoadError::BadConfig));
        let res = match (global_config, local_config) {
            (Ok(global), Ok(local)) => {
                let keymap_name = local
                    .keymap
                    .clone()
                    .or_else(|| global.keymap.clone())
                    .unwrap_or_else(|| DEFAULT_KEYMAP.to_string());
                let mut keys = keymap_base(&keymap_name);
                if let Some(global_keys) = global.keys {
                    merge_keys(&mut keys, global_keys)
                }
                if let Some(local_keys) = local.keys {
                    merge_keys(&mut keys, local_keys)
                }

                let editor = match (global.editor, local.editor) {
                    (None, None) => zmax_view::editor::Config::default(),
                    (None, Some(val)) | (Some(val), None) => {
                        val.try_into().map_err(ConfigLoadError::BadConfig)?
                    }
                    (Some(global), Some(local)) => merge_toml_values(global, local, 3)
                        .try_into()
                        .map_err(ConfigLoadError::BadConfig)?,
                };

                Config {
                    theme: local.theme.or(global.theme),
                    keys,
                    editor,
                    keymap: keymap_name,
                }
            }
            // if any configs are invalid return that first
            (_, Err(ConfigLoadError::BadConfig(err)))
            | (Err(ConfigLoadError::BadConfig(err)), _) => {
                return Err(ConfigLoadError::BadConfig(err))
            }
            (Ok(config), Err(_)) | (Err(_), Ok(config)) => {
                let keymap_name = config
                    .keymap
                    .clone()
                    .unwrap_or_else(|| DEFAULT_KEYMAP.to_string());
                let mut keys = keymap_base(&keymap_name);
                if let Some(user_keys) = config.keys {
                    merge_keys(&mut keys, user_keys);
                }
                Config {
                    theme: config.theme,
                    keys,
                    editor: config.editor.map_or_else(
                        || Ok(zmax_view::editor::Config::default()),
                        |val| val.try_into().map_err(ConfigLoadError::BadConfig),
                    )?,
                    keymap: keymap_name,
                }
            }

            // these are just two io errors return the one for the global config
            (Err(err), Err(_)) => return Err(err),
        };

        Ok(res)
    }

    pub fn load_default() -> Result<Config, ConfigLoadError> {
        let global_config =
            fs::read_to_string(zmax_loader::config_file()).map_err(ConfigLoadError::Error)?;
        let local_config = fs::read_to_string(zmax_loader::workspace_config_file())
            .map_err(ConfigLoadError::Error);

        let phony_config = ConfigLoadError::Error(IOError::other("hacky placeholder"));
        let global_parsed = Config::load(Ok(&global_config), Err(phony_config))?;

        // We need to build a transient `WorkspaceTrust` just to ask whether the workspace is
        // trusted enough to load its `.zmax/config.toml`. The persisted-trust file on disk is the
        // source of truth either way; this transient instance has an empty cache and is dropped
        // after the check.
        let trust = zmax_loader::workspace_trust::WorkspaceTrust::new(
            (&global_parsed.editor.workspace_trust).into(),
        );
        if trust
            .query_current(zmax_loader::workspace_trust::TrustQuery::LocalConfig)
            .is_trusted()
        {
            let mut merged = Config::load(Ok(&global_config), local_config)?;
            // editor.workspace-trust is global/user-scope only. Without this override, a
            // workspace's `.zmax/config.toml` could set `level = "insecure"`; once the user trusted
            // *that* workspace, refresh_config would re-load with the override merged in and from
            // then on every subsequent workspace in the session would be implicitly trusted. Pin
            // the gate's own configuration to the global file.
            merged.editor.workspace_trust = global_parsed.editor.workspace_trust;
            Ok(merged)
        } else {
            Ok(global_parsed)
        }
    }
}

/// Starter `config.toml` written to `~/.zmax/config.toml` on first run when no
/// global config exists. Every value shown is the in-code default (most are left
/// commented so future default changes still reach existing users — only the
/// active settings here are pinned). `{keymap}` is filled from [`DEFAULT_KEYMAP`]
/// so the written file never drifts from the compiled-in default.
const DEFAULT_CONFIG_BODY: &str = r#"# Zmax configuration. See `book/src/configuration.md` for the full reference.

# Base keymap preset: "spacemacs" (default), "vim", "helix", or "emacs".
keymap = "{keymap}"

# theme = "default"

[editor]
# line-number = "absolute"
# mouse = true
# cursorline = false
# color-modes = true
# bufferline = "never"

[editor.cursor-shape]
# insert = "bar"
# normal = "block"
# select = "underline"

[editor.file-picker]
# hidden = true

[editor.file-explorer]
# Show dotfiles in the project file tree. Defaults to false (dotfiles shown);
# set to true to hide files and directories whose name starts with a dot.
# hidden = false

[editor.statusline]
# left = ["mode", "spinner", "file-name", "read-only-indicator", "file-modification-indicator"]
"#;

/// Render [`DEFAULT_CONFIG_BODY`] with the compiled-in default keymap substituted.
pub fn default_config_body() -> String {
    DEFAULT_CONFIG_BODY.replace("{keymap}", DEFAULT_KEYMAP)
}

/// Write the starter [`default_config_body`] to `~/.zmax/config.toml` if that
/// file does not already exist. Creates the parent directory as needed. Never
/// overwrites an existing config. Errors are returned so the caller can decide
/// whether to surface them; a failure here is non-fatal (zmax still runs on
/// in-memory defaults).
pub fn write_default_config_file() -> std::io::Result<()> {
    let path = zmax_loader::config_file();
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, default_config_body())
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Config {
        fn load_test(config: &str) -> Config {
            Config::load(Ok(&config.to_owned()), Err(ConfigLoadError::default())).unwrap()
        }
    }

    #[test]
    fn parsing_keymaps_config_file() {
        use crate::keymap;
        use zmax_core::hashmap;
        use zmax_view::document::Mode;

        let sample_keymaps = r#"
            [keys.insert]
            y = "move_line_down"
            S-C-a = "delete_selection"

            [keys.normal]
            A-F12 = "move_next_word_end"
        "#;

        let mut keys = keymap::default();
        merge_keys(
            &mut keys,
            hashmap! {
                Mode::Insert => keymap!({ "Insert mode"
                    "y" => move_line_down,
                    "S-C-a" => delete_selection,
                }),
                Mode::Normal => keymap!({ "Normal mode"
                    "A-F12" => move_next_word_end,
                }),
            },
        );

        assert_eq!(
            Config::load_test(sample_keymaps),
            Config {
                keys,
                ..Default::default()
            }
        );
    }

    /// The starter `config.toml` we write on first run must parse cleanly and
    /// must select the compiled-in default keymap (so the seeded file matches the
    /// in-memory defaults a fresh run would otherwise use).
    #[test]
    fn default_config_body_parses_to_defaults() {
        let body = default_config_body();
        assert!(
            body.contains(&format!("keymap = \"{DEFAULT_KEYMAP}\"")),
            "seeded config must pin the default keymap, got:\n{body}"
        );
        let parsed = Config::load_test(&body);
        assert_eq!(parsed.keymap, DEFAULT_KEYMAP);
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn keys_resolve_to_correct_defaults() {
        // From serde default
        let default_keys = Config::load_test("").keys;
        assert_eq!(default_keys, keymap::default());

        // From the Default trait
        let default_keys = Config::default().keys;
        assert_eq!(default_keys, keymap::default());
    }

    /// The default (spacemacs) keymap is vim-derived, not the legacy
    /// selection-first one. Pin a couple of vim bindings so a regression in the
    /// `keymap::default` re-export (or the vim base) is caught here.
    #[test]
    fn default_keymap_is_vim() {
        use crate::keymap::{KeyTrie, MappableCommand};
        use zmax_view::input::KeyEvent;

        let keys = Config::default().keys;
        let normal = &keys[&Mode::Normal];

        let resolve = |chord: &str| -> Option<KeyTrie> {
            let evs: Vec<KeyEvent> = chord.split(' ').map(|k| k.parse().unwrap()).collect();
            normal.search(&evs).cloned()
        };
        let is_static = |t: Option<KeyTrie>, name: &str| matches!(t, Some(KeyTrie::MappableCommand(MappableCommand::Static { name: n, .. })) if n == name);

        // G jumps to the last line (vim), and C-v starts visual-block mode.
        assert!(
            is_static(resolve("G"), "goto_last_line"),
            "G should be goto_last_line"
        );
        assert!(
            is_static(resolve("C-v"), "visual_block_mode"),
            "C-v should start visual-block mode"
        );
        // V enters vim linewise (Visual Line) mode.
        assert!(
            is_static(resolve("V"), "visual_line_mode"),
            "V should enter visual-line mode"
        );
    }

    /// Spacemacs (the default) turns `C-x` into the Emacs prefix submenu and
    /// keeps the `SPC` leader; the pure `vim` preset leaves `C-x = decrement` and
    /// has no `SPC` leader (so it shows no which-key popup).
    #[test]
    fn spacemacs_has_cx_prefix_and_leader_vim_does_not() {
        use crate::keymap::{self, KeyTrie, MappableCommand};
        use zmax_view::input::KeyEvent;

        let cx: KeyEvent = "C-x".parse().unwrap();
        let space: KeyEvent = "space".parse().unwrap();

        let spacemacs = keymap::preset("spacemacs").unwrap();
        let sm_normal = &spacemacs[&Mode::Normal];
        assert!(
            matches!(sm_normal.search(&[cx]), Some(KeyTrie::Node(_))),
            "spacemacs C-x should be a prefix submenu (which-key)"
        );
        assert!(
            matches!(sm_normal.search(&[space]), Some(KeyTrie::Node(_))),
            "spacemacs should keep the SPC leader"
        );
        // The Emacs C-x C-f / C-x C-s bindings are present under the prefix.
        assert!(sm_normal.search(&[cx, "C-f".parse().unwrap()]).is_some());
        assert!(sm_normal.search(&[cx, "C-s".parse().unwrap()]).is_some());

        let vim = keymap::preset("vim").unwrap();
        let vim_normal = &vim[&Mode::Normal];
        assert!(
            matches!(
                vim_normal.search(&[cx]),
                Some(KeyTrie::MappableCommand(MappableCommand::Static { name, .. })) if *name == "decrement"
            ),
            "vim C-x should stay decrement"
        );
        assert!(
            !matches!(vim_normal.search(&[space]), Some(KeyTrie::Node(_))),
            "vim should have no SPC leader (no which-key popup)"
        );
    }
}
