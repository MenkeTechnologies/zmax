use crate::helpers;
use crate::path;
use crate::DynError;

use zemacs_term::commands::MappableCommand;
use zemacs_term::commands::TYPABLE_COMMAND_LIST;
use zemacs_term::health::TsFeature;
use zemacs_term::keymap::ReverseKeymap;
use zemacs_view::document::Mode;

use std::collections::HashSet;
use std::fs;

pub const TYPABLE_COMMANDS_MD_OUTPUT: &str = "typable-cmd.md";
pub const STATIC_COMMANDS_MD_OUTPUT: &str = "static-cmd.md";
pub const LANG_SUPPORT_MD_OUTPUT: &str = "lang-support.md";

fn md_table_heading(cols: &[String]) -> String {
    let mut header = String::new();
    header += &md_table_row(cols);
    header += &md_table_row(&vec!["---".to_string(); cols.len()]);
    header
}

fn md_table_row(cols: &[String]) -> String {
    format!("| {} |\n", cols.join(" | "))
}

fn md_mono(s: &str) -> String {
    format!("`{}`", s)
}

/// Make a command-description cell safe for the pandoc pipeline.
///
/// Descriptions are prose that may contain stray backticks — TeX smart quotes
/// (`` `` ``), mark-register notation (`` (g`) ``), and so on. A backtick run
/// that is not part of a balanced single-backtick code span opens an inline-code
/// span that pandoc lets run past the end of the cell and across the following
/// table rows, merging dozens of rows into one unbreakable box; lualatex then
/// aborts the whole document with "Dimension too large". Backticks are escaped
/// only when they are *not* already a set of balanced single-backtick spans (an
/// even count with no run of two or more) — so intended inline code, including
/// spans that contain `|` (e.g. `` `:lsp info|restart|stop` ``), is preserved.
/// Pipes are deliberately left alone: pandoc keeps a `|` inside a code span, and
/// escaping it would render a literal backslash there.
fn md_text_cell(s: &str) -> String {
    let balanced_code_spans = !s.contains("``") && s.matches('`').count() % 2 == 0;
    if balanced_code_spans {
        s.to_owned()
    } else {
        s.replace('`', "\\`")
    }
}

pub fn typable_commands() -> Result<String, DynError> {
    let mut md = String::new();
    md.push_str(&md_table_heading(&[
        "Name".to_owned(),
        "Description".to_owned(),
    ]));

    // escape | so it doesn't get rendered as a column separator
    let cmdify = |s: &str| format!("`:{}`", s.replace('|', "\\|"));

    for cmd in TYPABLE_COMMAND_LIST {
        let names = std::iter::once(&cmd.name)
            .chain(cmd.aliases.iter())
            .map(|a| cmdify(a))
            .collect::<Vec<_>>()
            .join(", ");

        let doc = md_text_cell(cmd.doc).replace('\n', "<br>");

        md.push_str(&md_table_row(&[names.to_owned(), doc.to_owned()]));
    }

    Ok(md)
}

pub fn static_commands() -> Result<String, DynError> {
    let mut md = String::new();

    // zemacs ships four keymap presets (keymap::PRESETS). keymap::default() is
    // only one of them — spacemacs — so documenting it alone hid the vim, helix,
    // and emacs bindings. Build each preset's reverse map (command -> key
    // sequences) for every mode up front, in PRESETS order.
    let modes = [
        ("normal", Mode::Normal),
        ("select", Mode::Select),
        ("insert", Mode::Insert),
    ];
    let presets: Vec<(&str, Vec<(&str, ReverseKeymap)>)> = zemacs_term::keymap::PRESETS
        .iter()
        .filter_map(|&name| {
            let keymap = zemacs_term::keymap::preset(name)?;
            let per_mode = modes
                .iter()
                .map(|&(label, mode)| (label, keymap[&mode].reverse_map()))
                .collect();
            Some((name, per_mode))
        })
        .collect();

    md.push_str(&md_table_heading(&[
        "Name".to_owned(),
        "Description".to_owned(),
        "Keybinds".to_owned(),
    ]));

    for cmd in MappableCommand::STATIC_COMMAND_LIST {
        // The mode-packed binding string for one preset, e.g.
        // "normal: `` h ``, insert: `` <left> ``" (empty when unbound here).
        let binds_in = |per_mode: &[(&str, ReverseKeymap)]| -> String {
            per_mode
                .iter()
                .filter_map(|(mode, rmap)| {
                    let bindings = rmap.get(cmd.name())?;
                    let mut bind_strings: Vec<_> = bindings
                        .iter()
                        .map(|bind| {
                            let keys = bind
                                .iter()
                                .map(|key| key.key_sequence_format())
                                .collect::<String>()
                                // escape | so it doesn't get rendered as a column separator
                                .replace('|', "\\|");
                            format!("`` {} ``", keys)
                        })
                        .collect();
                    // sort for stable output. sorting by length puts simple
                    // keybindings first and groups similar keys together
                    bind_strings.sort_by_key(|s| (s.len(), s.to_owned()));
                    Some(format!("{}: {}", mode, bind_strings.join(", ")))
                })
                .collect::<Vec<_>>()
                .join(", ")
        };

        // Group presets that share an identical binding string so keys common to
        // several presets aren't repeated; keep PRESETS order for the labels.
        let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
        for (name, per_mode) in &presets {
            let binds = binds_in(per_mode);
            if binds.is_empty() {
                continue;
            }
            match groups.iter_mut().find(|(b, _)| *b == binds) {
                Some(group) => group.1.push(name),
                None => groups.push((binds, vec![name])),
            }
        }
        let keymap_string = groups
            .iter()
            .map(|(binds, names)| format!("**{}** — {}", names.join(", "), binds))
            .collect::<Vec<_>>()
            .join("<br>");

        md.push_str(&md_table_row(&[
            md_mono(cmd.name()),
            md_text_cell(cmd.doc()),
            keymap_string,
        ]));
    }

    Ok(md)
}

pub fn lang_features() -> Result<String, DynError> {
    let mut md = String::new();
    let ts_features = TsFeature::all();

    let mut cols = vec!["Language".to_owned()];
    cols.append(
        &mut ts_features
            .iter()
            .map(|t| t.long_title().to_string())
            .collect::<Vec<_>>(),
    );
    cols.push("Default language servers".to_owned());

    md.push_str(&md_table_heading(&cols));
    let config = zemacs_core::config::default_lang_config();

    let mut langs = config
        .language
        .iter()
        .map(|l| l.language_id.clone())
        .collect::<Vec<_>>();
    langs.sort_unstable();

    let mut ts_features_to_langs = Vec::new();
    for &feat in ts_features {
        ts_features_to_langs.push((feat, helpers::ts_lang_support(feat)));
    }

    let mut row = Vec::new();
    for lang in langs {
        let lc = config
            .language
            .iter()
            .find(|l| l.language_id == lang)
            .unwrap(); // lang comes from config
        row.push(lc.language_id.clone());

        for (_feat, support_list) in &ts_features_to_langs {
            row.push(
                if support_list.contains(&lang) {
                    "✓"
                } else {
                    ""
                }
                .to_owned(),
            );
        }
        let mut seen_commands = HashSet::new();
        let mut commands = String::new();
        for ls_config in lc
            .language_servers
            .iter()
            .filter_map(|ls| config.language_server.get(&ls.name))
        {
            let command = &ls_config.command;
            if !seen_commands.insert(command) {
                continue;
            }

            if !commands.is_empty() {
                commands.push_str(", ");
            }

            commands.push_str(&md_mono(command));
        }
        row.push(commands);

        md.push_str(&md_table_row(&row));
        row.clear();
    }

    Ok(md)
}

pub fn write(filename: &str, data: &str) {
    let error = format!("Could not write to {}", filename);
    let path = path::book_gen().join(filename);
    fs::write(path, data).expect(&error);
}
