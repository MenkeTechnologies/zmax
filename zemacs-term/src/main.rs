use anyhow::{Context, Error, Result};
use zemacs_loader::VERSION_AND_GIT_HASH;
use zemacs_term::application::Application;
use zemacs_term::args::Args;
use zemacs_term::config::{Config, ConfigLoadError};

fn setup_logging(verbosity: u64) -> Result<()> {
    let level = match verbosity {
        0 => log::LevelFilter::Warn,
        1 => log::LevelFilter::Info,
        2 => log::LevelFilter::Debug,
        _3_or_more => log::LevelFilter::Trace,
    };

    zemacs_term::logging::init_file(level, &zemacs_loader::log_file())?;

    Ok(())
}

fn main() -> Result<()> {
    let exit_code = main_impl()?;
    std::process::exit(exit_code);
}

#[tokio::main]
async fn main_impl() -> Result<i32> {
    let args = Args::parse_args().context("could not parse arguments")?;

    zemacs_loader::initialize_config_file(args.config_file.clone());
    zemacs_loader::initialize_log_file(args.log_file.clone());

    // Help has a higher priority and should be handled separately.
    // Cyberpunk help chrome, matching the rest of the toolchain (`tp --help`,
    // `ztmux --help`, `zt --help`). Banner is `figlet -f "ANSI Shadow" ZEMACS`,
    // gradient cyan→magenta→red; `\x1b[0m` resets keep the terminal clean.
    if args.display_help {
        print!(
            "\
\x1b[36m ███████╗███████╗███╗   ███╗ █████╗  ██████╗███████╗\x1b[0m
\x1b[36m ╚══███╔╝██╔════╝████╗ ████║██╔══██╗██╔════╝██╔════╝\x1b[0m
\x1b[35m   ███╔╝ █████╗  ██╔████╔██║███████║██║     ███████╗\x1b[0m
\x1b[35m  ███╔╝  ██╔══╝  ██║╚██╔╝██║██╔══██║██║     ╚════██║\x1b[0m
\x1b[31m ███████╗███████╗██║ ╚═╝ ██║██║  ██║╚██████╗███████║\x1b[0m
\x1b[31m ╚══════╝╚══════╝╚═╝     ╚═╝╚═╝  ╚═╝ ╚═════╝╚══════╝\x1b[0m
\x1b[36m ┌──────────────────────────────────────────────────────┐\x1b[0m
\x1b[36m │ STATUS: ONLINE  // SIGNAL: ████████░░ // v{}\x1b[36m   │\x1b[0m
\x1b[36m └──────────────────────────────────────────────────────┘\x1b[0m
\x1b[35m  >> MODAL EDITOR // TREE-SITTER + LSP <<\x1b[0m

{}

\x1b[33m  USAGE:\x1b[0m zemacs [FLAGS] [files]...

\x1b[36m  ── FILES ──────────────────────────────────────────────\x1b[0m
    <files>...                   \x1b[32m//\x1b[0m Input file(s); position as file[:row[:col]]
\x1b[36m  ── MODE ───────────────────────────────────────────────\x1b[0m
    --strict                     \x1b[32m//\x1b[0m Bail on error for commands that can fail
    --tutor                      \x1b[32m//\x1b[0m Load the tutorial
    --ide                        \x1b[32m//\x1b[0m Boot the IDE workbench (sidebar; toggle F2)
    --health [CATEGORY]          \x1b[32m//\x1b[0m Check editor setup. CATEGORY = a language or
                                 \x1b[32m//\x1b[0m 'clipboard','languages','all-languages','all'.
                                 \x1b[32m//\x1b[0m 'languages' respects user config; 'all-*' don't.
                                 \x1b[32m//\x1b[0m Default: 'all' with languages filtering.
\x1b[36m  ── GRAMMARS ───────────────────────────────────────────\x1b[0m
    -g, --grammar {{fetch|build}}  \x1b[32m//\x1b[0m Fetch/build tree-sitter grammars (languages.toml)
\x1b[36m  ── CONFIG ─────────────────────────────────────────────\x1b[0m
    -c, --config <file>          \x1b[32m//\x1b[0m Use <file> for configuration
    -w, --working-dir <path>     \x1b[32m//\x1b[0m Specify an initial working directory
    --log <file>                 \x1b[32m//\x1b[0m Use <file> for logging
                                 \x1b[32m//\x1b[0m (default: {})
\x1b[36m  ── LAYOUT ─────────────────────────────────────────────\x1b[0m
    --vsplit                     \x1b[32m//\x1b[0m Split given files vertically into windows
    --hsplit                     \x1b[32m//\x1b[0m Split given files horizontally into windows
    +[N]                         \x1b[32m//\x1b[0m Open first file at line N (or last line)
\x1b[36m  ── SYSTEM ─────────────────────────────────────────────\x1b[0m
    -v                           \x1b[32m//\x1b[0m Increase logging verbosity (up to 3 times)
    -V, --version                \x1b[32m//\x1b[0m Print version information
    -h, --help                   \x1b[32m//\x1b[0m Print this help and exit

\x1b[35m  zemacs {} \x1b[0m// \x1b[33m{}\x1b[0m
\x1b[33m  >>> JACK IN. MODES ENGAGED. OWN YOUR BUFFERS. <<<\x1b[0m
\x1b[36m ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░\x1b[0m
",
            VERSION_AND_GIT_HASH,
            env!("CARGO_PKG_DESCRIPTION"),
            zemacs_loader::default_log_file().display(),
            VERSION_AND_GIT_HASH,
            env!("CARGO_PKG_AUTHORS"),
        );
        std::process::exit(0);
    }

    if args.display_version {
        println!("zemacs {}", VERSION_AND_GIT_HASH);
        std::process::exit(0);
    }

    if args.health {
        if let Err(err) = zemacs_term::health::print_health(args.health_arg) {
            // Piping to for example `head -10` requires special handling:
            // https://stackoverflow.com/a/65760807/7115678
            if err.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(err.into());
            }
        }

        std::process::exit(0);
    }

    if args.fetch_grammars {
        zemacs_loader::grammar::fetch_grammars(args.strict)?;
        return Ok(0);
    }

    if args.build_grammars {
        zemacs_loader::grammar::build_grammars(None, args.strict)?;
        return Ok(0);
    }

    setup_logging(args.verbosity).context("failed to initialize logging")?;

    // NOTE: Set the working directory early so the correct configuration is loaded. Be aware that
    // Application::new() depends on this logic so it must be updated if this changes.
    if let Some(path) = &args.working_directory {
        zemacs_stdx::env::set_current_working_dir(path)?;
    } else if let Some((path, _)) = args.files.first().filter(|p| p.0.is_dir()) {
        // If the first file is a directory, it will be the working directory unless -w was specified
        zemacs_stdx::env::set_current_working_dir(path)?;
    } else if let Err(err) = std::env::current_dir() {
        eprintln!("Couldn't determine the current working directory: {err}");
        eprintln!("Check that it still exists, or pass an initial directory with `--working-dir`");
        return Ok(1);
    }

    let config = match Config::load_default() {
        Ok(config) => config,
        Err(ConfigLoadError::Error(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            // First run: no `~/.zemacs/config.toml` yet. Seed it with the default
            // starter config so the user has an editable file. Failure is non-fatal
            // (logged only) — zemacs still runs on in-memory defaults.
            if let Err(write_err) = zemacs_term::config::write_default_config_file() {
                log::warn!("could not write default config file: {write_err}");
            }
            Config::default()
        }
        Err(ConfigLoadError::Error(err)) => return Err(Error::new(err)),
        Err(ConfigLoadError::BadConfig(err)) => {
            eprintln!("Bad config: {}", err);
            eprintln!("Press <ENTER> to continue with default config");
            use std::io::Read;
            let _ = std::io::stdin().read(&mut []);
            Config::default()
        }
    };

    let workspace_trust = zemacs_loader::workspace_trust::WorkspaceTrust::new(
        (&config.editor.workspace_trust).into(),
    );

    let lang_loader =
        zemacs_core::config::user_lang_loader(&workspace_trust).unwrap_or_else(|err| {
            eprintln!("{}", err);
            eprintln!("Press <ENTER> to continue with default language config");
            use std::io::Read;
            // This waits for an enter press.
            let _ = std::io::stdin().read(&mut []);
            zemacs_core::config::default_lang_loader()
        });

    // TODO: use the thread local executor to spawn the application task separately from the work pool
    let mut app = Application::new(args, config, lang_loader, workspace_trust)
        .context("unable to start Zemacs")?;

    // Load embedded-scripting init files (~/.zemacs/init.el) before the UI loop.
    app.load_init_scripts();

    let mut events = app.event_stream();

    let exit_code = app.run(&mut events).await?;

    Ok(exit_code)
}
