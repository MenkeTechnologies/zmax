use anyhow::Result;
use indexmap::IndexMap;
use std::path::{Path, PathBuf};
use zmax_core::Position;
use zmax_view::tree::Layout;

#[derive(Default, Debug)]
pub struct Args {
    pub display_help: bool,
    pub display_version: bool,
    pub health: bool,
    pub health_arg: Option<String>,
    pub load_tutor: bool,
    /// Boot with the IDE workbench open (file-tree sidebar, etc.).
    pub ide: bool,
    pub fetch_grammars: bool,
    pub build_grammars: bool,
    pub strict: bool,
    pub split: Option<Layout>,
    pub verbosity: u64,
    pub log_file: Option<PathBuf>,
    pub config_file: Option<PathBuf>,
    pub files: IndexMap<PathBuf, Vec<Position>>,
    pub working_directory: Option<PathBuf>,
}

impl Args {
    pub fn parse_args() -> Result<Args> {
        Self::parse_from(std::env::args())
    }

    /// Parses an argument vector, program name included. `parse_args` hands this
    /// the real one; tests hand it a literal.
    #[allow(clippy::too_many_lines)]
    pub fn parse_from(argv: impl Iterator<Item = String>) -> Result<Args> {
        let mut args = Args::default();
        let mut argv = argv.peekable();
        let mut line_number = 0;

        let mut insert_file_with_position = |file_with_position: &str| {
            let (filename, position) = parse_file(file_with_position);

            // Before setting the working directory, resolve all the paths in args.files
            let filename = zmax_stdx::path::canonicalize(filename);

            args.files
                .entry(filename)
                .and_modify(|positions| positions.push(position))
                .or_insert_with(|| vec![position]);
        };

        argv.next(); // skip the program, we don't care about that

        while let Some(arg) = argv.next() {
            match arg.as_str() {
                "--" => break, // stop parsing at this point treat the remaining as files
                "--version" => args.display_version = true,
                "--help" => args.display_help = true,
                "--strict" => args.strict = true,
                "--tutor" => args.load_tutor = true,
                "--ide" => args.ide = true,
                "--vsplit" => match args.split {
                    Some(_) => anyhow::bail!("can only set a split once of a specific type"),
                    None => args.split = Some(Layout::Vertical),
                },
                "--hsplit" => match args.split {
                    Some(_) => anyhow::bail!("can only set a split once of a specific type"),
                    None => args.split = Some(Layout::Horizontal),
                },
                "--health" => {
                    args.health = true;
                    args.health_arg = argv.next_if(|opt| !opt.starts_with('-'));
                }
                "-g" | "--grammar" => match argv.next().as_deref() {
                    Some("fetch") => args.fetch_grammars = true,
                    Some("build") => args.build_grammars = true,
                    _ => {
                        anyhow::bail!("--grammar must be followed by either 'fetch' or 'build'")
                    }
                },
                "-c" | "--config" => match argv.next().as_deref() {
                    Some(path) => args.config_file = Some(path.into()),
                    None => anyhow::bail!("--config must specify a path to read"),
                },
                "--log" => match argv.next().as_deref() {
                    Some(path) => args.log_file = Some(path.into()),
                    None => anyhow::bail!("--log must specify a path to write"),
                },
                "-w" | "--working-dir" => match argv.next().as_deref() {
                    Some(path) => {
                        args.working_directory = if Path::new(path).is_dir() {
                            Some(PathBuf::from(path))
                        } else {
                            anyhow::bail!(
                                "--working-dir specified does not exist or is not a directory"
                            )
                        }
                    }
                    None => {
                        anyhow::bail!("--working-dir must specify an initial working directory")
                    }
                },
                arg if arg.starts_with("--") => {
                    anyhow::bail!("unexpected double dash argument: {}", arg)
                }
                arg if arg.starts_with('-') => {
                    let arg = arg.get(1..).unwrap().chars();
                    for chr in arg {
                        match chr {
                            'v' => args.verbosity += 1,
                            'V' => args.display_version = true,
                            'h' => args.display_help = true,
                            _ => anyhow::bail!("unexpected short arg {}", chr),
                        }
                    }
                }
                "+" => line_number = usize::MAX,
                arg if arg.starts_with('+') => {
                    match arg[1..].parse::<usize>() {
                        Ok(n) => line_number = n.saturating_sub(1),
                        _ => insert_file_with_position(arg),
                    };
                }
                arg => insert_file_with_position(arg),
            }
        }

        // push the remaining args, if any to the files
        for arg in argv {
            insert_file_with_position(&arg);
        }

        if line_number != 0 {
            if let Some(first_position) = args
                .files
                .first_mut()
                .and_then(|(_, positions)| positions.first_mut())
            {
                first_position.row = line_number;
            }
        }

        Ok(args)
    }
}

/// Parse arg into [`PathBuf`] and position.
pub(crate) fn parse_file(s: &str) -> (PathBuf, Position) {
    let def = || (PathBuf::from(s), Position::default());
    if Path::new(s).exists() {
        return def();
    }
    split_path_row_col(s)
        .or_else(|| split_path_row(s))
        .unwrap_or_else(def)
}

/// Split file.rs:10:2 into [`PathBuf`], row and col.
///
/// Does not validate if file.rs is a file or directory.
fn split_path_row_col(s: &str) -> Option<(PathBuf, Position)> {
    let mut s = s.trim_end_matches(':').rsplitn(3, ':');
    let col: usize = s.next()?.parse().ok()?;
    let row: usize = s.next()?.parse().ok()?;
    let path = s.next()?.into();
    let pos = Position::new(row.saturating_sub(1), col.saturating_sub(1));
    Some((path, pos))
}

/// Split file.rs:10 into [`PathBuf`] and row.
///
/// Does not validate if file.rs is a file or directory.
fn split_path_row(s: &str) -> Option<(PathBuf, Position)> {
    let (path, row) = s.trim_end_matches(':').rsplit_once(':')?;
    let row: usize = row.parse().ok()?;
    let path = path.into();
    let pos = Position::new(row.saturating_sub(1), 0);
    Some((path, pos))
}

#[cfg(test)]
mod test {
    use super::*;

    fn parse(argv: &[&str]) -> Result<Args> {
        Args::parse_from(
            std::iter::once("zmax".to_string()).chain(argv.iter().map(|a| (*a).to_string())),
        )
    }

    /// `zmax -g fetch && zmax -g build` is the documented way to get grammars
    /// (`book/src/building-from-source.md`), and both spellings of the flag reach
    /// the same two booleans `main` dispatches on.
    #[test]
    fn grammar_flag_selects_fetch_or_build() {
        for flag in ["-g", "--grammar"] {
            let args = parse(&[flag, "fetch"]).unwrap();
            assert!(args.fetch_grammars, "{flag} fetch");
            assert!(!args.build_grammars, "{flag} fetch must not also build");

            let args = parse(&[flag, "build"]).unwrap();
            assert!(args.build_grammars, "{flag} build");
            assert!(!args.fetch_grammars, "{flag} build must not also fetch");
        }
    }

    /// The subcommand is mandatory and closed: a bare flag, or anything other
    /// than fetch/build, is an error rather than a silent no-op that leaves the
    /// editor booting with neither boolean set.
    #[test]
    fn grammar_flag_rejects_a_missing_or_unknown_subcommand() {
        for argv in [vec!["-g"], vec!["-g", "rebuild"], vec!["--grammar", "-c"]] {
            let err = parse(&argv).unwrap_err().to_string();
            assert!(
                err.contains("'fetch' or 'build'"),
                "{argv:?} must name the two accepted subcommands, got: {err}"
            );
        }
    }

    /// Both phases can be asked for in one invocation -- `main` runs fetch then
    /// build off two independent booleans, so `zmax -g fetch -g build` does what
    /// `zmax -g fetch && zmax -g build` does in one process.
    #[test]
    fn both_grammar_phases_can_be_requested_at_once() {
        let args = parse(&["-g", "fetch", "-g", "build"]).unwrap();

        assert!(args.fetch_grammars && args.build_grammars);
    }

    /// The grammar subcommand is consumed by the flag, not left to fall through
    /// to the file list -- `zmax -g fetch` must not also open a file named
    /// "fetch".
    #[test]
    fn grammar_subcommand_is_not_taken_for_a_file() {
        let args = parse(&["-g", "fetch"]).unwrap();
        assert!(args.files.is_empty(), "files: {:?}", args.files);
    }

    /// `file:row:col` and `file:row` are how an editor jump lands on a line, and
    /// both are one-based on the command line but zero-based in `Position`. A row
    /// of 0 saturates rather than wrapping to `usize::MAX`.
    #[test]
    fn file_positions_are_parsed_one_based_and_saturate() {
        let cases = [
            ("src/main.rs:10:2", "src/main.rs", 9, 1),
            ("src/main.rs:10", "src/main.rs", 9, 0),
            ("src/main.rs:10:2:", "src/main.rs", 9, 1),
            ("src/main.rs:0:0", "src/main.rs", 0, 0),
            // No trailing numbers at all: the whole argument is the path.
            ("src/main.rs", "src/main.rs", 0, 0),
            // A non-numeric tail is part of the name, not a position.
            ("src/main.rs:notanumber", "src/main.rs:notanumber", 0, 0),
        ];

        for (arg, path, row, col) in cases {
            let (parsed_path, position) = parse_file(arg);
            assert_eq!(parsed_path, PathBuf::from(path), "path of {arg}");
            assert_eq!((position.row, position.col), (row, col), "position of {arg}");
        }
    }

    /// The same file named twice collects both positions under one entry, so the
    /// editor opens one buffer rather than two.
    #[test]
    fn repeating_a_file_collects_its_positions() {
        let args = parse(&["src/main.rs:1:1", "src/main.rs:5:1"]).unwrap();

        assert_eq!(args.files.len(), 1);
        assert_eq!(args.files.values().next().unwrap().len(), 2);
    }

    /// A split can be set once; asking for both a vertical and a horizontal one
    /// is a contradiction rather than a last-one-wins.
    #[test]
    fn a_split_cannot_be_set_twice() {
        assert!(parse(&["--vsplit"]).unwrap().split.is_some());
        assert!(parse(&["--hsplit"]).unwrap().split.is_some());

        for argv in [
            vec!["--vsplit", "--hsplit"],
            vec!["--vsplit", "--vsplit"],
            vec!["--hsplit", "--vsplit"],
        ] {
            let err = parse(&argv).unwrap_err().to_string();
            assert!(err.contains("can only set a split once"), "{argv:?}: {err}");
        }
    }

    /// Flags that take a value error out when the value is missing instead of
    /// silently swallowing the next flag or leaving the option unset.
    #[test]
    fn value_flags_require_their_value() {
        for (argv, expected) in [
            (vec!["-c"], "--config must specify a path"),
            (vec!["--log"], "--log must specify a path"),
            (vec!["-w"], "--working-dir must specify"),
            (vec!["-w", "definitely-not-a-directory"], "does not exist"),
        ] {
            let err = parse(&argv).unwrap_err().to_string();
            assert!(err.contains(expected), "{argv:?}: {err}");
        }
    }

    /// `--health` takes an optional category, and must not eat a following flag
    /// as one.
    #[test]
    fn health_takes_an_optional_category_but_not_a_flag() {
        let args = parse(&["--health"]).unwrap();
        assert!(args.health && args.health_arg.is_none());

        let args = parse(&["--health", "languages"]).unwrap();
        assert_eq!(args.health_arg.as_deref(), Some("languages"));

        let args = parse(&["--health", "--strict"]).unwrap();
        assert!(args.health && args.health_arg.is_none() && args.strict);
    }

    /// Everything after `--` is a file, including a word that would otherwise
    /// parse as a flag.
    #[test]
    fn double_dash_stops_flag_parsing() {
        let args = parse(&["--", "-g"]).unwrap();
        assert!(!args.fetch_grammars && !args.build_grammars);
        assert_eq!(args.files.len(), 1);
    }
}
