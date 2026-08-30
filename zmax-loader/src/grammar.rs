use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::SystemTime;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::channel,
};
use tempfile::TempPath;
use tree_house::tree_sitter::Grammar;

#[cfg(target_os = "macos")]
const DYLIB_EXTENSION: &str = "dylib";

#[cfg(all(unix, not(target_os = "macos")))]
const DYLIB_EXTENSION: &str = "so";

#[cfg(windows)]
const DYLIB_EXTENSION: &str = "dll";

#[cfg(target_arch = "wasm32")]
const DYLIB_EXTENSION: &str = "wasm";

#[derive(Debug, Serialize, Deserialize)]
struct Configuration {
    #[serde(rename = "use-grammars")]
    pub grammar_selection: Option<GrammarSelection>,
    pub grammar: Vec<GrammarConfiguration>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", untagged)]
pub enum GrammarSelection {
    Only { only: HashSet<String> },
    Except { except: HashSet<String> },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrammarConfiguration {
    #[serde(rename = "name")]
    pub grammar_id: String,
    pub source: GrammarSource,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", untagged)]
pub enum GrammarSource {
    Local {
        path: String,
    },
    Git {
        #[serde(rename = "git")]
        remote: String,
        #[serde(rename = "rev")]
        revision: String,
        subpath: Option<String>,
    },
}

const BUILD_TARGET: &str = env!("BUILD_TARGET");
const REMOTE_NAME: &str = "origin";

#[cfg(target_arch = "wasm32")]
pub fn get_language(name: &str) -> Result<Option<Grammar>> {
    unimplemented!()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn get_language(name: &str) -> Result<Option<Grammar>> {
    let mut rel_library_path = PathBuf::new().join("grammars").join(name);
    rel_library_path.set_extension(DYLIB_EXTENSION);
    let library_path = crate::runtime_file(&rel_library_path);
    if !library_path.exists() {
        return Ok(None);
    }

    let grammar = unsafe { Grammar::new(name, &library_path) }?;
    Ok(Some(grammar))
}

fn ensure_git_is_available() -> Result<()> {
    zmax_stdx::env::which("git")?;
    Ok(())
}

/// Print a notice if the current workspace has a `.zmax/languages.toml` that we *would* have
/// merged but the workspace-trust gate is keeping us from.
fn warn_if_workspace_languages_skipped(trust: &crate::workspace_trust::WorkspaceTrust) {
    let workspace_languages = crate::workspace_lang_config_file();
    if !workspace_languages.exists() {
        return;
    }
    if trust
        .query_current(crate::workspace_trust::TrustQuery::LocalConfig)
        .is_trusted()
    {
        return;
    }
    println!(
        "Note: workspace `{}` was skipped because the workspace is not trusted. Run \
         `:workspace-trust` from an interactive zmax session in this workspace to opt in.",
        workspace_languages.display(),
    );
}

pub fn fetch_grammars(strict: bool) -> Result<()> {
    ensure_git_is_available()?;

    let mut grammars = get_grammar_configs()?;
    grammars.retain(is_fetchable);

    let total = grammars.len();
    let counter = Arc::new(AtomicUsize::new(0));

    println!("Fetching {} grammars", total);
    let counter = Arc::clone(&counter);

    let results = run_parallel(grammars, move |grammar| {
        let current = counter.fetch_add(1, Ordering::Relaxed) + 1;

        println!(
            "Fetching grammars ({}/{}): {}",
            current, total, grammar.grammar_id
        );
        fetch_grammar(grammar)
    });

    let mut errors = Vec::new();
    let mut git_updated = Vec::new();
    let mut git_up_to_date = 0;
    let mut non_git = Vec::new();

    for (grammar_id, res) in results {
        match res {
            Ok(FetchStatus::GitUpToDate) => git_up_to_date += 1,
            Ok(FetchStatus::GitUpdated { revision }) => git_updated.push((grammar_id, revision)),
            Ok(FetchStatus::NonGit) => non_git.push(grammar_id),
            Err(e) => errors.push((grammar_id, e)),
        }
    }

    non_git.sort_unstable();
    git_updated.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    if git_up_to_date != 0 {
        println!("{} up to date git grammars", git_up_to_date);
    }

    if !non_git.is_empty() {
        println!("{} non git grammars", non_git.len());
        println!("\t{:?}", non_git);
    }

    if !git_updated.is_empty() {
        println!("{} updated grammars", git_updated.len());
        // We checked the vec is not empty, unwrapping will not panic
        let longest_id = git_updated.iter().map(|x| x.0.len()).max().unwrap();
        for (id, rev) in git_updated {
            println!(
                "\t{id:width$} now on {rev}",
                id = id,
                width = longest_id,
                rev = rev
            );
        }
    }

    if !errors.is_empty() {
        let len = errors.len();
        for (i, (grammar, error)) in errors.into_iter().enumerate() {
            println!("Failure {}/{len}: {grammar} {error:#}", i + 1);
        }
        if strict {
            bail!("{len} grammars failed to fetch");
        }
    }

    Ok(())
}

pub fn build_grammars(target: Option<String>, strict: bool) -> Result<()> {
    ensure_git_is_available()?;

    let grammars = get_grammar_configs()?;

    let total = grammars.len();
    let counter = Arc::new(AtomicUsize::new(0));

    println!("Building {} grammars", grammars.len());

    // A worker thread that panics never sends a result, so its grammar used to
    // drop out of the summary entirely: the counts came up one short and the
    // build reported success while a grammar had blown up mid-way. Keep the
    // requested ids so those can be reported as the failures they are.
    let requested: HashSet<String> = grammars
        .iter()
        .map(|grammar| grammar.grammar_id.clone())
        .collect();

    let counter = Arc::clone(&counter);
    let results = run_parallel(grammars, move |grammar| {
        let current = counter.fetch_add(1, Ordering::Relaxed) + 1;

        println!(
            "Building grammars ({}/{}): {}",
            current, total, grammar.grammar_id
        );
        build_grammar(grammar, target.as_deref())
    });

    let mut errors = Vec::new();
    let mut already_built = 0;
    let mut built = Vec::new();
    let mut reported = HashSet::new();

    for (grammar_id, res) in results {
        reported.insert(grammar_id.clone());
        match res {
            Ok(BuildStatus::AlreadyBuilt) => already_built += 1,
            Ok(BuildStatus::Built) => built.push(grammar_id),
            Err(e) => errors.push((grammar_id, e)),
        }
    }

    // Grammars whose worker never reported: the job panicked (the panic message
    // is on stderr above). Counting them as failures keeps the summary honest.
    let mut panicked: Vec<&String> = requested.difference(&reported).collect();
    panicked.sort_unstable();
    for grammar_id in panicked {
        errors.push((
            grammar_id.clone(),
            anyhow!("build panicked, see the panic message above"),
        ));
    }

    built.sort_unstable();

    if already_built != 0 {
        println!("{} grammars already built", already_built);
    }

    if !built.is_empty() {
        println!("{} grammars built now", built.len());
        println!("\t{:?}", built);
    }

    if !errors.is_empty() {
        let len = errors.len();
        for (i, (grammar_id, error)) in errors.into_iter().enumerate() {
            println!("Failure {}/{len}: {grammar_id} {error:#}", i + 1);
        }
        if strict {
            bail!("{len} grammars failed to build");
        }
    }

    Ok(())
}

// Returns the set of grammar configurations the user requests.
// Grammars are configured in the default and user `languages.toml` and are
// merged. The `grammar_selection` key of the config is then used to filter
// down all grammars into a subset of the user's choosing.
fn get_grammar_configs() -> Result<Vec<GrammarConfiguration>> {
    // `--grammar fetch/build` clones grammar sources from URLs in `languages.toml` and compiles
    // them into `.so` files zmax later loads at runtime. If we let workspace
    // `.zmax/languages.toml` in through `fully_trusted`, a malicious workspace could inject a
    // grammar with an attacker-controlled git source — running grammar build in that
    // directory would clone and compile attacker code
    let trust = crate::workspace_trust::WorkspaceTrust::new(Default::default());
    warn_if_workspace_languages_skipped(&trust);
    let config: Configuration = crate::config::user_lang_config(&trust)
        .context("Could not parse languages.toml")?
        .try_into()?;

    Ok(select_grammars(config))
}

/// A local grammar is checked into the tree at a `path`, so there is nothing to
/// fetch: only git sources are cloned. Fetching one would mean running `git` in a
/// directory the repo ships.
fn is_fetchable(grammar: &GrammarConfiguration) -> bool {
    !matches!(grammar.source, GrammarSource::Local { .. })
}

/// Applies the `use-grammars` key: `only` keeps just the named grammars, `except`
/// drops them, and an absent key takes every grammar in the merged config.
fn select_grammars(config: Configuration) -> Vec<GrammarConfiguration> {
    match config.grammar_selection {
        Some(GrammarSelection::Only { only: selections }) => config
            .grammar
            .into_iter()
            .filter(|grammar| selections.contains(&grammar.grammar_id))
            .collect(),
        Some(GrammarSelection::Except { except: rejections }) => config
            .grammar
            .into_iter()
            .filter(|grammar| !rejections.contains(&grammar.grammar_id))
            .collect(),
        None => config.grammar,
    }
}

pub fn get_grammar_names() -> Result<Option<HashSet<String>>> {
    // See `get_grammar_configs`, same threat: workspace-local
    // `languages.toml` must not influence the grammar set without
    // explicit on-disk trust.
    let trust = crate::workspace_trust::WorkspaceTrust::new(Default::default());
    warn_if_workspace_languages_skipped(&trust);
    let config: Configuration = crate::config::user_lang_config(&trust)
        .context("Could not parse languages.toml")?
        .try_into()?;

    let grammars = match config.grammar_selection {
        Some(GrammarSelection::Only { only: selections }) => Some(selections),
        Some(GrammarSelection::Except { except: rejections }) => Some(
            config
                .grammar
                .into_iter()
                .map(|grammar| grammar.grammar_id)
                .filter(|id| !rejections.contains(id))
                .collect(),
        ),
        None => None,
    };

    Ok(grammars)
}

fn run_parallel<F, Res>(grammars: Vec<GrammarConfiguration>, job: F) -> Vec<(String, Result<Res>)>
where
    F: Fn(GrammarConfiguration) -> Result<Res> + Send + 'static + Clone,
    Res: Send + 'static,
{
    let pool = threadpool::Builder::new().build();
    let (tx, rx) = channel();

    for grammar in grammars {
        let tx = tx.clone();
        let job = job.clone();

        pool.execute(move || {
            // Ignore any SendErrors, if any job in another thread has encountered an
            // error the Receiver will be closed causing this send to fail.
            let _ = tx.send((grammar.grammar_id.clone(), job(grammar)));
        });
    }

    drop(tx);

    rx.iter().collect()
}

enum FetchStatus {
    GitUpToDate,
    GitUpdated { revision: String },
    NonGit,
}

#[derive(Copy, Clone)]
enum GitObjectFormat {
    Sha1,
    Sha256,
}

impl GitObjectFormat {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
        }
    }
}

fn extract_object_format_from_revision(rev: &str) -> (GitObjectFormat, &str) {
    if let Some(stripped) = rev.strip_prefix("sha1:") {
        return (GitObjectFormat::Sha1, stripped);
    }

    if let Some(stripped) = rev.strip_prefix("sha256:") {
        return (GitObjectFormat::Sha256, stripped);
    }

    if rev.len() == 64 && rev.bytes().all(|b| b.is_ascii_hexdigit()) {
        return (GitObjectFormat::Sha256, rev);
    }

    (GitObjectFormat::Sha1, rev)
}

struct VendoredGrammar {
    dir: PathBuf,
}

impl VendoredGrammar {
    fn new(grammar: &str) -> Self {
        let dir = crate::runtime_dirs()
            .first()
            .expect("No runtime directories provided") // guaranteed by post-condition
            .join("grammars")
            .join("sources")
            .join(grammar);

        Self { dir }
    }

    /// Gets the current revision of the repo.
    fn revision(&self) -> Option<String> {
        git(&self.dir, ["rev-parse", "HEAD"]).ok()
    }

    /// Fetches grammar at the given revision.
    ///
    /// To ensure clean state, existing grammar directory is removed and re-inited
    /// before fetch operation.
    fn fetch(&self, remote: &str, rev: &str, object_format: GitObjectFormat) -> Result<()> {
        self.reinit(remote, object_format)?;

        git(&self.dir, ["fetch", "--depth", "1", REMOTE_NAME, rev])?;
        git(&self.dir, ["checkout", rev])?;

        Ok(())
    }

    /// Initializes the grammar directory.
    ///
    /// Creates directory and sets it up as a git repo, with remote set correctly.
    fn init(&self, remote: &str, object_format: GitObjectFormat) -> Result<()> {
        // Create the grammar directory if needed.
        fs::create_dir_all(&self.dir)
            .context(format!("Could not create grammar directory {:?}", self.dir))?;

        // Ensure directory is git initialized.
        if !self.dir.join(".git").exists() {
            git(
                &self.dir,
                ["init", "--object-format", object_format.as_str()],
            )?;
        }

        // Ensure the remote matches the configured remote, setting if needed.
        if self.remote().as_deref() != Some(remote) {
            self.set_remote(remote)?;
        }

        Ok(())
    }

    /// Removes the grammar directory before initializing again.
    fn reinit(&self, remote: &str, object_format: GitObjectFormat) -> Result<()> {
        fs::remove_dir_all(&self.dir)?;
        self.init(remote, object_format)?;
        Ok(())
    }

    /// Gets remote URL of grammar repo.
    fn remote(&self) -> Option<String> {
        git(&self.dir, ["remote", "get-url", REMOTE_NAME]).ok()
    }

    /// Sets remote URL of grammar repo.
    fn set_remote(&self, remote: &str) -> Result<()> {
        git(&self.dir, ["remote", "set-url", REMOTE_NAME, remote])
            .or_else(|_| git(&self.dir, ["remote", "add", REMOTE_NAME, remote]))?;
        Ok(())
    }
}

fn fetch_grammar(grammar: GrammarConfiguration) -> Result<FetchStatus> {
    let GrammarSource::Git {
        remote, revision, ..
    } = grammar.source
    else {
        return Ok(FetchStatus::NonGit);
    };

    let repo = VendoredGrammar::new(&grammar.grammar_id);

    let (object_format, revision) = extract_object_format_from_revision(&revision);

    // WARN: Must init before other operations are done.
    repo.init(&remote, object_format)?;

    if repo.revision().is_some_and(|rev| rev == revision) {
        return Ok(FetchStatus::GitUpToDate);
    }

    // Fetch the grammar if the revision doesn't match.
    repo.fetch(&remote, revision, object_format)?;

    Ok(FetchStatus::GitUpdated {
        revision: revision.to_string(),
    })
}

// A wrapper around 'git' commands which returns stdout in success and a
// helpful error message showing the command, stdout, and stderr in error.
fn git<I, S>(repository_dir: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .current_dir(repository_dir)
        .output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned())
    } else {
        // TODO: figure out how to display the git command using `args`
        Err(anyhow!(
            "Git command failed.\nStdout: {}\nStderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ))
    }
}

#[derive(Debug)]
enum BuildStatus {
    AlreadyBuilt,
    Built,
}

/// Resolves the directory a `path = ...` grammar source names.
///
/// Canonicalized to an absolute path: the compile step `cd`s into the grammar's
/// `src/` dir, after which a *relative* path (e.g. one starting with `../`) would
/// resolve against the wrong base and the parser file would not be found. Git
/// sources sidestep this because `runtime_dirs()` is already absolute.
///
/// A relative path in `languages.toml` is anchored to `runtime_dir`, not to whatever
/// directory zmax was invoked from -- the shipped stryke entry
/// (`../runtime/grammars/sources/stryke`) otherwise resolves against the CWD and
/// `zmax -g build` fails from anywhere but a workspace crate directory. A relative
/// path that misses under `runtime_dir` is left as-is, so a caller who really did
/// mean a CWD-relative path still gets one.
fn local_grammar_dir(path: &Path, runtime_dir: &Path) -> PathBuf {
    let path = if path.is_relative() {
        let anchored = runtime_dir.join(path);
        if anchored.exists() {
            anchored
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    };
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn build_grammar(grammar: GrammarConfiguration, target: Option<&str>) -> Result<BuildStatus> {
    let grammar_dir = if let GrammarSource::Local { path } = &grammar.source {
        local_grammar_dir(
            Path::new(path),
            crate::runtime_dirs()
                .first()
                .expect("No runtime directories provided"), // guaranteed by post-condition
        )
    } else {
        crate::runtime_dirs()
            .first()
            .expect("No runtime directories provided") // guaranteed by post-condition
            .join("grammars")
            .join("sources")
            .join(&grammar.grammar_id)
    };

    let grammar_dir_entries = grammar_dir.read_dir().with_context(|| {
        format!(
            "Failed to read directory {:?}. Did you use 'zmax -g fetch'?",
            grammar_dir
        )
    })?;

    if grammar_dir_entries.count() == 0 {
        return Err(anyhow!(
            "Directory {:?} is empty. Did you use 'zmax -g fetch'?",
            grammar_dir
        ));
    };

    let path = match &grammar.source {
        GrammarSource::Git {
            subpath: Some(subpath),
            ..
        } => grammar_dir.join(subpath),
        _ => grammar_dir,
    }
    .join("src");

    generate_parser_if_missing(&path)
        .with_context(|| format!("Failed to generate a parser in {}", path.display()))?;

    build_tree_sitter_library(&path, grammar, target)
}

/// Write `src/parser.c` (and the headers it includes) when the grammar ships
/// only `src/grammar.json`.
///
/// Not every grammar checks its generated parser in — upstreams that gitignore
/// `src/*` expect `tree-sitter generate` to run before the build (the raku
/// grammar, a fork of tree-sitter-perl, is one). The compile step feeds
/// `parser.c` straight to the C compiler, so those grammars failed with
/// `clang++: no such file or directory: …/src/parser.c`.
///
/// The grammar's *dumped JSON* is what gets read here, so this needs neither the
/// `tree-sitter` CLI nor a JS runtime to evaluate `grammar.js`. A grammar that
/// ships `parser.c` never reaches the generator.
fn generate_parser_if_missing(src_path: &Path) -> Result<()> {
    let parser_path = src_path.join("parser.c");
    if parser_path.exists() {
        return Ok(());
    }
    let grammar_json_path = src_path.join("grammar.json");
    if !grammar_json_path.exists() {
        // Nothing to generate from — let the compile step report the missing
        // parser, which is the more useful error for a broken checkout.
        return Ok(());
    }

    let grammar_json = fs::read_to_string(&grammar_json_path)
        .with_context(|| format!("Failed to read {}", grammar_json_path.display()))?;
    let (_language_name, parser_c) = tree_sitter_generate::generate_parser_for_grammar(
        &grammar_json,
        Some(grammar_semantic_version(src_path)),
    )
    .map_err(|err| anyhow!("{err}"))?;
    fs::write(&parser_path, parser_c)
        .with_context(|| format!("Failed to write {}", parser_path.display()))?;

    // `tree-sitter generate` also drops its runtime headers next to the parser;
    // a grammar that never generates locally can be missing them (raku ships
    // only `alloc.h`, while its scanner includes `array.h` and `parser.h`).
    // Existing headers are left alone — a grammar may ship a patched copy.
    let header_dir = src_path.join("tree_sitter");
    fs::create_dir_all(&header_dir)
        .with_context(|| format!("Failed to create {}", header_dir.display()))?;
    for (name, contents) in [
        ("parser.h", tree_sitter_generate::PARSER_HEADER),
        ("array.h", tree_sitter_generate::ARRAY_HEADER),
        ("alloc.h", tree_sitter_generate::ALLOC_HEADER),
    ] {
        let header_path = header_dir.join(name);
        if !header_path.exists() {
            fs::write(&header_path, contents)
                .with_context(|| format!("Failed to write {}", header_path.display()))?;
        }
    }
    Ok(())
}

/// The grammar's `metadata.version` from its `tree-sitter.json`, which the
/// generator embeds in the parser. ABI 15 requires one — without it the
/// generator panics with "Metadata is required to generate ABI version 15" —
/// and the `tree-sitter` CLI reads it from exactly this file. Grammars that
/// predate `tree-sitter.json` get `0.0.0`; the value is metadata only, read back
/// through `ts_language_metadata`, and never affects parsing.
fn grammar_semantic_version(src_path: &Path) -> (u8, u8, u8) {
    let config_path = src_path
        .parent()
        .unwrap_or(src_path)
        .join("tree-sitter.json");
    let parsed = fs::read_to_string(config_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| json["metadata"]["version"].as_str().map(str::to_owned));

    let Some(version) = parsed else {
        return (0, 0, 0);
    };
    let mut parts = version
        .split('.')
        .map(|part| part.trim().parse::<u8>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

fn build_tree_sitter_library(
    src_path: &Path,
    grammar: GrammarConfiguration,
    target: Option<&str>,
) -> Result<BuildStatus> {
    let header_path = src_path;
    let parser_path = src_path.join("parser.c");
    let mut scanner_path = src_path.join("scanner.c");

    let scanner_path = if scanner_path.exists() {
        Some(scanner_path)
    } else {
        scanner_path.set_extension("cc");
        if scanner_path.exists() {
            Some(scanner_path)
        } else {
            None
        }
    };
    let parser_lib_path = crate::runtime_dirs()
        .first()
        .expect("No runtime directories provided") // guaranteed by post-condition
        .join("grammars");
    let mut library_path = parser_lib_path.join(&grammar.grammar_id);
    library_path.set_extension(DYLIB_EXTENSION);

    // if we are running inside a buildscript emit cargo metadata
    // to detect if we are running from a buildscript check some env variables
    // that cargo only sets for build scripts
    if std::env::var("OUT_DIR").is_ok() && std::env::var("CARGO").is_ok() {
        if let Some(scanner_path) = scanner_path.as_ref().and_then(|path| path.to_str()) {
            println!("cargo:rerun-if-changed={scanner_path}");
        }
        if let Some(parser_path) = parser_path.to_str() {
            println!("cargo:rerun-if-changed={parser_path}");
        }
    }

    let recompile = needs_recompile(&library_path, &parser_path, scanner_path.as_ref())
        .context("Failed to compare source and binary timestamps")?;

    if !recompile {
        return Ok(BuildStatus::AlreadyBuilt);
    }

    let mut config = cc::Build::new();
    config
        .cpp(true)
        .opt_level(3)
        .cargo_metadata(false)
        .host(BUILD_TARGET)
        .target(target.unwrap_or(BUILD_TARGET));
    let compiler = config.get_compiler();
    let mut command = Command::new(compiler.path());
    command.current_dir(src_path);
    for (key, value) in compiler.env() {
        command.env(key, value);
    }

    command.args(compiler.args());
    // used to delay dropping the temporary object file until after the compilation is complete
    let _path_guard;

    if compiler.is_like_msvc() {
        command
            .args(["/nologo", "/LD", "/I"])
            .arg(header_path)
            .arg("/utf-8")
            .arg("/std:c11");
        if let Some(scanner_path) = scanner_path.as_ref() {
            if scanner_path.extension() == Some("c".as_ref()) {
                command.arg(scanner_path);
            } else {
                let mut cpp_command = Command::new(compiler.path());
                cpp_command.current_dir(src_path);
                for (key, value) in compiler.env() {
                    cpp_command.env(key, value);
                }
                cpp_command.args(compiler.args());
                let object_file =
                    library_path.with_file_name(format!("{}_scanner.obj", grammar.grammar_id));
                cpp_command
                    .args(["/nologo", "/LD", "/I"])
                    .arg(header_path)
                    .arg("/utf-8")
                    .arg("/std:c++14")
                    .arg(format!("/Fo{}", object_file.display()))
                    .arg("/c")
                    .arg(scanner_path);
                let output = cpp_command
                    .output()
                    .context("Failed to execute C++ compiler")?;

                if !output.status.success() {
                    return Err(anyhow!(
                        "Parser compilation failed.\nStdout: {}\nStderr: {}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
                command.arg(&object_file);
                _path_guard = TempPath::try_from_path(object_file).unwrap();
            }
        }

        command
            .arg(parser_path)
            .arg("/link")
            .arg(format!("/out:{}", library_path.to_str().unwrap()));
    } else {
        #[cfg(not(windows))]
        command.arg("-fPIC");

        command
            .arg("-shared")
            .arg("-fno-exceptions")
            .arg("-I")
            .arg(header_path)
            .arg("-o")
            .arg(&library_path);

        if let Some(scanner_path) = scanner_path.as_ref() {
            if scanner_path.extension() == Some("c".as_ref()) {
                command.arg("-xc").arg("-std=c11").arg(scanner_path);
            } else {
                let mut cpp_command = Command::new(compiler.path());
                cpp_command.current_dir(src_path);
                for (key, value) in compiler.env() {
                    cpp_command.env(key, value);
                }
                cpp_command.args(compiler.args());
                let object_file =
                    library_path.with_file_name(format!("{}_scanner.o", grammar.grammar_id));

                #[cfg(not(windows))]
                cpp_command.arg("-fPIC");

                cpp_command
                    .arg("-fno-exceptions")
                    .arg("-I")
                    .arg(header_path)
                    .arg("-o")
                    .arg(&object_file)
                    .arg("-std=c++14")
                    .arg("-c")
                    .arg(scanner_path);
                let output = cpp_command
                    .output()
                    .context("Failed to execute C++ compiler")?;
                if !output.status.success() {
                    return Err(anyhow!(
                        "Parser compilation failed.\nStdout: {}\nStderr: {}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }

                command.arg(&object_file);
                _path_guard = TempPath::try_from_path(object_file).unwrap();
            }
        }
        command.arg("-xc").arg("-std=c11").arg(parser_path);
        if cfg!(all(
            unix,
            not(any(target_os = "macos", target_os = "illumos"))
        )) {
            command.arg("-Wl,-z,relro,-z,now");
        }
    }

    let output = command
        .output()
        .context("Failed to execute C/C++ compiler")?;
    if !output.status.success() {
        return Err(anyhow!(
            "Parser compilation failed.\nStdout: {}\nStderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(BuildStatus::Built)
}

fn needs_recompile(
    lib_path: &Path,
    parser_c_path: &Path,
    scanner_path: Option<&PathBuf>,
) -> Result<bool> {
    if !lib_path.exists() {
        return Ok(true);
    }
    let lib_mtime = mtime(lib_path)?;
    if mtime(parser_c_path)? > lib_mtime {
        return Ok(true);
    }
    if let Some(scanner_path) = scanner_path {
        if mtime(scanner_path)? > lib_mtime {
            return Ok(true);
        }
    }
    Ok(false)
}

fn mtime(path: &Path) -> Result<SystemTime> {
    Ok(fs::metadata(path)?.modified()?)
}

/// Gives the contents of a file from a language's `runtime/queries/<lang>`
/// directory
pub fn load_runtime_file(language: &str, filename: &str) -> Result<String, std::io::Error> {
    let path = crate::runtime_file(PathBuf::new().join("queries").join(language).join(filename));
    std::fs::read_to_string(path)
}

#[cfg(test)]
mod test {
    use super::*;

    /// A grammar with one rule, enough to drive the generator end to end.
    const MINIMAL_GRAMMAR_JSON: &str = r#"{
        "name": "zmaxtest",
        "rules": { "source_file": { "type": "STRING", "value": "a" } }
    }"#;

    fn grammar_dir(version: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        if let Some(version) = version {
            fs::write(
                dir.path().join("tree-sitter.json"),
                format!(r#"{{ "metadata": {{ "version": "{version}" }} }}"#),
            )
            .unwrap();
        }
        dir
    }

    #[test]
    fn semantic_version_comes_from_tree_sitter_json() {
        let dir = grammar_dir(Some("1.2.3"));
        assert_eq!(grammar_semantic_version(&dir.path().join("src")), (1, 2, 3));

        // Short and unparseable values degrade to zeros rather than failing the
        // build: the version is metadata, not something parsing depends on.
        let dir = grammar_dir(Some("2"));
        assert_eq!(grammar_semantic_version(&dir.path().join("src")), (2, 0, 0));
        let dir = grammar_dir(Some("not.a.version"));
        assert_eq!(grammar_semantic_version(&dir.path().join("src")), (0, 0, 0));

        // No `tree-sitter.json` at all (grammars that predate it).
        let dir = grammar_dir(None);
        assert_eq!(grammar_semantic_version(&dir.path().join("src")), (0, 0, 0));
    }

    /// The generator refuses to emit ABI 15 without a version — it panics with
    /// "Metadata is required to generate ABI version 15" — so this also pins
    /// that `grammar_semantic_version` is actually being handed to it.
    #[test]
    fn parser_is_generated_from_grammar_json() {
        let dir = grammar_dir(Some("1.0.0"));
        let src = dir.path().join("src");
        fs::write(src.join("grammar.json"), MINIMAL_GRAMMAR_JSON).unwrap();

        generate_parser_if_missing(&src).unwrap();

        let parser = fs::read_to_string(src.join("parser.c")).unwrap();
        // tree-house loads ABI 13..=15 (`MIN_COMPATIBLE_ABI_VERSION` /
        // `ABI_VERSION` in tree-house-bindings' `grammar.rs`), and the generator
        // emits its own `ABI_VERSION_MAX`. Pin the value so a generator bump to
        // ABI 16 fails here instead of at grammar-load time.
        assert!(
            parser.contains("#define LANGUAGE_VERSION 15"),
            "generated parser must declare an ABI tree-house accepts (13..=15)"
        );
        // The headers the generated parser and a scanner include.
        assert!(src.join("tree_sitter/parser.h").exists());
        assert!(src.join("tree_sitter/array.h").exists());
        assert!(src.join("tree_sitter/alloc.h").exists());
    }

    /// Lays out a workspace holding a runtime directory with one local grammar
    /// source, mirroring what `languages.toml` points `stryke` at:
    /// `<workspace>/runtime/grammars/sources/<id>`. The returned directory is the
    /// *workspace*; the runtime directory is `.join("runtime")` of it.
    fn workspace_with_grammar(grammar_id: &str) -> tempfile::TempDir {
        let workspace = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            workspace
                .path()
                .join("runtime")
                .join("grammars")
                .join("sources")
                .join(grammar_id),
        )
        .unwrap();
        workspace
    }

    /// The shipped stryke entry is `path = "../runtime/grammars/sources/stryke"`.
    /// Resolved against the process CWD it only lands from a workspace crate
    /// directory, so `zmax -g build` failed on stryke from the repo root and from
    /// anywhere else. Anchoring to the runtime directory makes it CWD-independent.
    #[test]
    fn relative_local_grammar_path_anchors_to_the_runtime_dir() {
        let workspace = workspace_with_grammar("stryke");
        let runtime = workspace.path().join("runtime");
        let expected =
            std::fs::canonicalize(runtime.join("grammars").join("sources").join("stryke")).unwrap();

        let resolved = local_grammar_dir(Path::new("../runtime/grammars/sources/stryke"), &runtime);

        assert_eq!(resolved, expected);
    }

    /// A relative path that names nothing under the runtime directory is handed
    /// back unchanged rather than rewritten into a runtime path that does not
    /// exist -- the caller may genuinely have meant a CWD-relative directory, and
    /// `build_grammar`'s error should name the path the user wrote.
    #[test]
    fn relative_local_grammar_path_without_a_runtime_match_is_left_alone() {
        let runtime = tempfile::tempdir().unwrap();
        let path = Path::new("../nowhere/grammars/sources/absent");

        assert_eq!(local_grammar_dir(path, runtime.path()), path.to_path_buf());
    }

    /// An absolute path is never re-based onto the runtime directory.
    #[test]
    fn absolute_local_grammar_path_is_not_anchored() {
        let grammar = tempfile::tempdir().unwrap();
        let workspace = workspace_with_grammar("stryke");

        assert_eq!(
            local_grammar_dir(grammar.path(), &workspace.path().join("runtime")),
            std::fs::canonicalize(grammar.path()).unwrap()
        );
    }

    /// Both "did you fetch?" messages name a real zmax command. They said
    /// `hx --grammar fetch` -- a helix invocation that does not exist here -- and
    /// the failure listing printed only anyhow's outermost context, so a broken
    /// runtime directory reported `Failed to read directory ...` with no reason.
    /// `{:#}` is what the CLI prints, so assert on that rendering.
    #[test]
    fn missing_grammar_dir_names_zmax_and_the_io_reason() {
        let runtime = tempfile::tempdir().unwrap();
        let absent = runtime.path().join("absent");
        let err = build_grammar(
            GrammarConfiguration {
                grammar_id: "absent".to_string(),
                source: GrammarSource::Local {
                    path: absent.to_str().unwrap().to_string(),
                },
            },
            None,
        )
        .unwrap_err();

        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("zmax -g fetch"),
            "message must name a zmax command, got: {rendered}"
        );
        assert!(
            !rendered.contains("hx "),
            "message must not name helix, got: {rendered}"
        );
        assert!(
            rendered.contains("No such file or directory"),
            "the io reason must survive into the rendered chain, got: {rendered}"
        );
    }

    /// A fetched-but-empty source directory is its own message, and it too used to
    /// name `hx`.
    #[test]
    fn empty_grammar_dir_names_zmax() {
        let empty = tempfile::tempdir().unwrap();
        let err = build_grammar(
            GrammarConfiguration {
                grammar_id: "empty".to_string(),
                source: GrammarSource::Local {
                    path: empty.path().to_str().unwrap().to_string(),
                },
            },
            None,
        )
        .unwrap_err();

        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("is empty") && rendered.contains("zmax -g fetch"),
            "got: {rendered}"
        );
    }

    /// `use-grammars` decides which grammars `zmax -g fetch`/`-g build` touch at
    /// all. Each arm is exercised over the same three-grammar config so a filter
    /// inverted in either direction shows up as a concrete id list.
    #[test]
    fn use_grammars_selects_only_excepts_or_everything() {
        let config = |selection: &str| -> Configuration {
            toml::from_str(&format!(
                r#"
                {selection}
                [[grammar]]
                name = "rust"
                source = {{ path = "/tmp/rust" }}
                [[grammar]]
                name = "stryke"
                source = {{ path = "/tmp/stryke" }}
                [[grammar]]
                name = "zsh"
                source = {{ path = "/tmp/zsh" }}
                "#
            ))
            .unwrap()
        };
        let ids = |config: Configuration| -> Vec<String> {
            select_grammars(config)
                .into_iter()
                .map(|grammar| grammar.grammar_id)
                .collect()
        };

        assert_eq!(
            ids(config(r#"use-grammars = { only = ["stryke"] }"#)),
            vec!["stryke"]
        );
        assert_eq!(
            ids(config(r#"use-grammars = { except = ["stryke"] }"#)),
            vec!["rust", "zsh"]
        );
        assert_eq!(ids(config("")), vec!["rust", "stryke", "zsh"]);
    }

    /// An `only` naming a grammar the config does not define selects nothing
    /// rather than falling back to every grammar.
    #[test]
    fn use_grammars_only_with_an_unknown_name_selects_nothing() {
        let config: Configuration = toml::from_str(
            r#"
            use-grammars = { only = ["absent"] }
            [[grammar]]
            name = "rust"
            source = { path = "/tmp/rust" }
            "#,
        )
        .unwrap();

        assert!(select_grammars(config).is_empty());
    }

    /// The two `[[grammar]]` source shapes `languages.toml` uses. `GrammarSource`
    /// is an untagged enum, so a mistyped key does not fall through to the other
    /// variant -- `deny_unknown_fields` on the entry makes it an error instead of
    /// a grammar that silently never builds.
    #[test]
    fn grammar_sources_parse_as_local_or_git_and_reject_typos() {
        let local: GrammarConfiguration = toml::from_str(
            r#"
            name = "stryke"
            source = { path = "../runtime/grammars/sources/stryke" }
            "#,
        )
        .unwrap();
        assert!(matches!(local.source, GrammarSource::Local { path } if path.ends_with("stryke")));

        let git: GrammarConfiguration = toml::from_str(
            r#"
            name = "rust"
            source = { git = "https://example.invalid/tree-sitter-rust", rev = "abc123", subpath = "sub" }
            "#,
        )
        .unwrap();
        assert!(matches!(
            git.source,
            GrammarSource::Git { subpath: Some(subpath), .. } if subpath == "sub"
        ));

        // `revision` is the struct field name; `rev` is the key. Accepting the
        // former would deserialize a git source with no revision to check out.
        assert!(toml::from_str::<GrammarConfiguration>(
            r#"
            name = "rust"
            source = { git = "https://example.invalid/tree-sitter-rust", revision = "abc123" }
            "#,
        )
        .is_err());
    }

    /// Every grammar the shipped `languages.toml` declares must parse, and no id
    /// may repeat: `get_grammar_configs` keys `use-grammars` off these ids, and a
    /// duplicate would fetch and build the same grammar twice under one name.
    #[test]
    fn the_shipped_languages_toml_declares_unique_parseable_grammars() {
        let config: Configuration = toml::from_str(include_str!("../../languages.toml")).unwrap();

        assert!(
            config.grammar.len() > 300,
            "expected the full grammar set, got {}",
            config.grammar.len()
        );

        let mut seen = HashSet::new();
        for grammar in &config.grammar {
            assert!(
                seen.insert(&grammar.grammar_id),
                "duplicate grammar id: {}",
                grammar.grammar_id
            );
        }
    }

    /// The one local grammar the repo ships is stryke, and its path is relative --
    /// the case `local_grammar_dir` anchors. If it ever becomes absolute or moves
    /// out of `runtime/grammars/sources`, the anchoring tests above stop covering
    /// anything real.
    #[test]
    fn stryke_is_the_shipped_local_grammar_and_its_path_is_relative() {
        let config: Configuration = toml::from_str(include_str!("../../languages.toml")).unwrap();

        let local: Vec<_> = config
            .grammar
            .iter()
            .filter_map(|grammar| match &grammar.source {
                GrammarSource::Local { path } => Some((grammar.grammar_id.as_str(), path.as_str())),
                GrammarSource::Git { .. } => None,
            })
            .collect();

        assert_eq!(
            local,
            vec![("stryke", "../runtime/grammars/sources/stryke")]
        );
        assert!(Path::new(local[0].1).is_relative());
    }

    /// `-g fetch` clones git sources and skips local ones -- which is why fetch
    /// reports one fewer grammar than build. A local source reaching the fetch
    /// path would run `git` against a directory the repo ships.
    #[test]
    fn only_git_grammars_are_fetched() {
        let config: Configuration = toml::from_str(include_str!("../../languages.toml")).unwrap();
        let total = config.grammar.len();

        let fetchable: Vec<_> = config.grammar.into_iter().filter(is_fetchable).collect();

        assert_eq!(
            fetchable.len(),
            total - 1,
            "exactly the one local grammar (stryke) is skipped"
        );
        assert!(fetchable
            .iter()
            .all(|grammar| matches!(grammar.source, GrammarSource::Git { .. })));
    }

    /// The git branch of `build_grammar` resolves under the runtime directory
    /// rather than through `local_grammar_dir`, and reports the same fetch hint
    /// with the io reason attached.
    #[test]
    fn a_git_grammar_with_no_fetched_source_reports_the_fetch_hint() {
        let err = build_grammar(
            GrammarConfiguration {
                grammar_id: "zmax-test-never-fetched".to_string(),
                source: GrammarSource::Git {
                    remote: "https://example.invalid/tree-sitter-absent".to_string(),
                    revision: "0000000000000000000000000000000000000000".to_string(),
                    subpath: None,
                },
            },
            None,
        )
        .unwrap_err();

        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("zmax-test-never-fetched")
                && rendered.contains("zmax -g fetch")
                && rendered.contains("No such file or directory"),
            "got: {rendered}"
        );
    }

    #[test]
    fn existing_parser_is_never_overwritten() {
        let dir = grammar_dir(Some("1.0.0"));
        let src = dir.path().join("src");
        fs::write(src.join("grammar.json"), MINIMAL_GRAMMAR_JSON).unwrap();
        fs::write(src.join("parser.c"), "/* checked in by the grammar */").unwrap();

        generate_parser_if_missing(&src).unwrap();

        assert_eq!(
            fs::read_to_string(src.join("parser.c")).unwrap(),
            "/* checked in by the grammar */"
        );
    }
}
