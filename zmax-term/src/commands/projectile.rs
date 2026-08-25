//! Projectile — the project-interaction surface of `bbatsov/projectile`.
//!
//! Projectile is a project *library* first and a set of commands second: it
//! keeps a list of known projects, a per-project file cache, and a command map
//! (`C-c p`) over them. zmax already knows what a project is (`find_workspace`
//! walks to the `.git`/`.zmax` marker) and keeps the known list in
//! `<config-dir>/projects`; this module is projectile's commands over that
//! model, one function per `projectile-*` command.
//!
//! The commands live here rather than in `commands/typed.rs` so the port reads
//! as one piece; the `:` table in `typed.rs` points at them.

use std::path::{Path, PathBuf};

use crate::commands::{expand_home, known_projects, write_known_projects};
use crate::compositor;
use crate::ui::PromptEvent;
use zmax_loader::find_workspace;

/// The project the current buffer is in — projectile's `projectile-project-root`,
/// which is the marker-bearing ancestor `find_workspace` resolves to.
pub(crate) fn project_root() -> PathBuf {
    find_workspace().0
}

/// `projectile-project-name`: the project root's last component.
pub(crate) fn project_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned())
}

/// Whether `dir` looks like a project root: it carries one of the markers
/// `zmax_core::project::is_project_marker` names (`.git`, `.hg`, `Cargo.toml`, …),
/// which is how `projectile-project-p` decides.
pub(crate) fn is_project_root(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(zmax_core::project::is_project_marker)
    })
}

/// Add `root` to the known-projects list, keeping the list's most-recent-first
/// order. Returns whether it was new.
fn remember(root: &Path) -> bool {
    let root = root.to_string_lossy().into_owned();
    let mut list = known_projects();
    let known = list.iter().any(|p| *p == root);
    zmax_core::project::record_project(&mut list, &root);
    let _ = write_known_projects(&list);
    !known
}

/// `projectile-add-known-project`: "Add PROJECT-ROOT to the list of known
/// projects." With no argument the current project is added, which is what the
/// interactive form reads from `default-directory`.
pub(crate) fn add_known_project(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = match args.first() {
        Some(dir) => expand_home(dir),
        None => project_root(),
    };
    if !root.is_dir() {
        anyhow::bail!("projectile-add-known-project: {} is not a directory", root.display());
    }
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    if remember(&root) {
        cx.editor
            .set_status(format!("Added {} to known projects", root.display()));
    } else {
        cx.editor
            .set_status(format!("{} is already a known project", root.display()));
    }
    Ok(())
}

/// `projectile-remove-known-project`: "Remove PROJECT from the list of known
/// projects."
pub(crate) fn remove_known_project(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let Some(dir) = args.first() else {
        anyhow::bail!("projectile-remove-known-project: needs a project directory");
    };
    let root = expand_home(dir).to_string_lossy().into_owned();
    let mut list = known_projects();
    if zmax_core::project::forget_project(&mut list, &root) {
        write_known_projects(&list)?;
        cx.editor
            .set_status(format!("Removed {root} from known projects"));
    } else {
        cx.editor
            .set_error(format!("{root} is not a known project"));
    }
    Ok(())
}

/// `projectile-remove-current-project-from-known-projects`.
pub(crate) fn remove_current_project(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root().to_string_lossy().into_owned();
    let mut list = known_projects();
    if zmax_core::project::forget_project(&mut list, &root) {
        write_known_projects(&list)?;
        cx.editor
            .set_status(format!("Removed {root} from known projects"));
    } else {
        cx.editor.set_error(format!("{root} is not a known project"));
    }
    Ok(())
}

/// `projectile-clear-known-projects`: "Clear both `projectile-known-projects' and
/// `projectile-known-projects-on-file'."
pub(crate) fn clear_known_projects(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let count = known_projects().len();
    write_known_projects(&[])?;
    cx.editor
        .set_status(format!("Cleared {count} known project(s)"));
    Ok(())
}

/// `projectile-cleanup-known-projects`: "Remove known projects that don't exist
/// anymore."
pub(crate) fn cleanup_known_projects(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    // `known_projects` already drops roots that are gone, so the cleanup is the
    // difference between what the file holds and what it resolves to.
    let raw = std::fs::read_to_string(crate::commands::known_projects_file()).unwrap_or_default();
    let before = raw.lines().filter(|l| !l.trim().is_empty()).count();
    let live = known_projects();
    let removed = before.saturating_sub(live.len());
    write_known_projects(&live)?;
    cx.editor.set_status(match removed {
        0 => "No known projects have been removed".to_string(),
        1 => "Removed 1 project that no longer exists".to_string(),
        n => format!("Removed {n} projects that no longer exist"),
    });
    Ok(())
}

/// `projectile-forget-projects-under`: "Remove known projects located under
/// DIRECTORY."
pub(crate) fn forget_projects_under(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let Some(dir) = args.first() else {
        anyhow::bail!("projectile-forget-projects-under: needs a directory");
    };
    let under = expand_home(dir);
    let under = std::fs::canonicalize(&under).unwrap_or(under);
    let before = known_projects();
    let after: Vec<String> = before
        .iter()
        .filter(|root| !Path::new(root).starts_with(&under))
        .cloned()
        .collect();
    let removed = before.len() - after.len();
    write_known_projects(&after)?;
    cx.editor.set_status(match removed {
        0 => format!("No projects under {}", under.display()),
        1 => format!("Removed 1 project under {}", under.display()),
        n => format!("Removed {n} projects under {}", under.display()),
    });
    Ok(())
}

/// The project roots directly under `dir`, one level down — what
/// `projectile-discover-projects-in-directory` collects with its default depth.
pub(crate) fn projects_in_directory(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut frontier = vec![(dir.to_path_buf(), 0usize)];
    while let Some((current, level)) = frontier.pop() {
        if is_project_root(&current) {
            found.push(current);
            // projectile does not descend into a project it has already found.
            continue;
        }
        if level >= depth {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') {
                    continue;
                }
                frontier.push((entry.path(), level + 1));
            }
        }
    }
    found.sort();
    found
}

/// `projectile-discover-projects-in-directory`: "Discover any projects in
/// DIRECTORY and add them to the projectile cache." The optional second argument
/// is the search depth (projectile's own default is 1 level below DIRECTORY).
pub(crate) fn discover_projects_in_directory(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let Some(dir) = args.first() else {
        anyhow::bail!("projectile-discover-projects-in-directory: needs a directory");
    };
    let depth = args
        .get(1)
        .and_then(|d| d.parse::<usize>().ok())
        .unwrap_or(1);
    let root = expand_home(dir);
    if !root.is_dir() {
        anyhow::bail!(
            "projectile-discover-projects-in-directory: {} is not a directory",
            root.display()
        );
    }
    let found = projects_in_directory(&root, depth);
    let mut added = 0;
    for project in &found {
        if remember(project) {
            added += 1;
        }
    }
    cx.editor.set_status(format!(
        "Found {} project(s) under {}, {added} new",
        found.len(),
        root.display()
    ));
    Ok(())
}

/// `projectile-project-search-path`: the directories
/// `projectile-discover-projects-in-search-path` scans. Projectile keeps it in a
/// defcustom, so the elisp global is read first; `ZMAX_PROJECT_SEARCH_PATH` (a
/// `:`-separated list) is the shell-side way to set it.
pub(crate) fn project_search_path() -> Vec<PathBuf> {
    if let Some(paths) = crate::commands::scripting::elisp_global_string_list(
        "projectile-project-search-path",
    ) {
        if !paths.is_empty() {
            return paths.iter().map(|p| expand_home(p)).collect();
        }
    }
    std::env::var("ZMAX_PROJECT_SEARCH_PATH")
        .ok()
        .map(|raw| {
            raw.split(':')
                .filter(|p| !p.is_empty())
                .map(expand_home)
                .collect()
        })
        .unwrap_or_default()
}

/// `projectile-discover-projects-in-search-path`: "Discover projects in
/// `projectile-project-search-path'."
pub(crate) fn discover_projects_in_search_path(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let search_path = project_search_path();
    if search_path.is_empty() {
        anyhow::bail!(
            "projectile-project-search-path is empty — set it with `:elisp (setq \
             projectile-project-search-path '(\"~/src\"))` or $ZMAX_PROJECT_SEARCH_PATH"
        );
    }
    let mut found = 0;
    let mut added = 0;
    for dir in &search_path {
        for project in projects_in_directory(dir, 1) {
            found += 1;
            if remember(&project) {
                added += 1;
            }
        }
    }
    cx.editor
        .set_status(format!("Found {found} project(s) in the search path, {added} new"));
    Ok(())
}

/// `projectile-switch-to-most-recent-project`: "Switch to the project recorded in
/// `projectile-most-recent-project'" — the most recent one that is not this one.
pub(crate) fn switch_to_most_recent_project(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let here = project_root().to_string_lossy().into_owned();
    let Some(previous) = known_projects().into_iter().find(|root| *root != here) else {
        anyhow::bail!("projectile-switch-to-most-recent-project: no other known project");
    };
    crate::commands::project_switch_to(cx, PathBuf::from(previous));
    Ok(())
}

/// The known projects that currently have an open buffer — what
/// `projectile-switch-open-project` completes over.
pub(crate) fn open_projects(editor: &zmax_view::Editor) -> Vec<String> {
    let mut roots: Vec<String> = Vec::new();
    for doc in editor.documents() {
        let Some(path) = doc.path() else { continue };
        if let Some(root) = known_projects()
            .into_iter()
            .find(|root| path.starts_with(root))
        {
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    roots
}

/// `projectile-switch-open-project`: "Switch to a project we have currently
/// opened" — the known projects that have a buffer open, rather than every
/// project ever visited.
pub(crate) fn switch_open_project(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let open = open_projects(cx.editor);
    if open.is_empty() {
        anyhow::bail!("projectile-switch-open-project: no project buffers are open");
    }
    match args.first() {
        Some(dir) => {
            let root = expand_home(dir);
            crate::commands::project_switch_to(cx, root);
        }
        None if open.len() == 1 => {
            crate::commands::project_switch_to(cx, PathBuf::from(&open[0]));
        }
        None => {
            // More than one is open and none was named: say which, the way the
            // completing read would offer them.
            cx.editor.set_status(format!(
                "Open projects: {} — :projectile-switch-open-project <root>",
                open.join(", ")
            ));
        }
    }
    Ok(())
}

/// `projectile-add-and-switch-project`: "Add PROJECT-ROOT to the list of known
/// projects and switch to it."
pub(crate) fn add_and_switch_project(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let Some(dir) = args.first() else {
        anyhow::bail!("projectile-add-and-switch-project: needs a project directory");
    };
    let root = expand_home(dir);
    if !root.is_dir() {
        anyhow::bail!(
            "projectile-add-and-switch-project: {} is not a directory",
            root.display()
        );
    }
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    remember(&root);
    crate::commands::project_switch_to(cx, root);
    Ok(())
}

/// `projectile-project-info`: "Display info for current project."
pub(crate) fn project_info(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root();
    cx.editor.set_status(format!(
        "Project {} ({}), root {}",
        project_name(&root),
        project_type(&root),
        root.display()
    ));
    Ok(())
}

/// One row of projectile's project-type table: how a project of this kind is
/// recognised, and the external commands its lifecycle phases run.
///
/// Ported from the `projectile-register-project-type` forms in projectile.el —
/// all 97 of them, in the order projectile resolves them (each registration is
/// pushed onto the front of `projectile-project-types`, so the last one
/// registered is the first one tried).
pub(crate) struct ProjectType {
    pub name: &'static str,
    /// Files that must *all* be present (`projectile-verify-files`).
    pub all: &'static [&'static str],
    /// Files of which *any* one is enough (the `(:any …)` marker form). A
    /// `?*.ext` entry is projectile's wildcard: any file with that extension.
    pub any: &'static [&'static str],
    pub configure: Option<&'static str>,
    pub compile: Option<&'static str>,
    pub test: Option<&'static str>,
    pub install: Option<&'static str>,
    pub package: Option<&'static str>,
    pub run: Option<&'static str>,
    pub test_prefix: Option<&'static str>,
    pub test_suffix: Option<&'static str>,
}

#[rustfmt::skip]
pub(crate) const PROJECT_TYPES: &[ProjectType] = &[
    ProjectType {
        name: "platformio",
        all: &["platformio.ini"],
        any: &[],
        configure: None,
        compile: Some("pio run"),
        test: Some("pio test"),
        install: Some("pio run -t upload"),
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "godot",
        all: &["project.godot"],
        any: &[],
        configure: None,
        compile: None,
        test: None,
        install: None,
        package: None,
        run: Some("godot --path ."),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "foundry",
        all: &["foundry.toml"],
        any: &[],
        configure: None,
        compile: Some("forge build"),
        test: Some("forge test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some(".t"),
    },
    ProjectType {
        name: "alire",
        all: &["alire.toml"],
        any: &[],
        configure: None,
        compile: Some("alr build"),
        test: Some("alr test"),
        install: None,
        package: None,
        run: Some("alr run"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "fpm",
        all: &["fpm.toml"],
        any: &[],
        configure: None,
        compile: Some("fpm build"),
        test: Some("fpm test"),
        install: None,
        package: None,
        run: Some("fpm run"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "dub",
        all: &[],
        any: &["dub.json", "dub.sdl"],
        configure: None,
        compile: Some("dub build"),
        test: Some("dub test"),
        install: None,
        package: None,
        run: Some("dub run"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "swift-spm",
        all: &["Package.swift"],
        any: &[],
        configure: None,
        compile: Some("swift build"),
        test: Some("swift test"),
        install: None,
        package: None,
        run: Some("swift run"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "zig",
        all: &[],
        any: &["build.zig", "build.zig.zon"],
        configure: None,
        compile: Some("zig build"),
        test: Some("zig build test"),
        install: None,
        package: None,
        run: Some("zig build run"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "ocaml-dune",
        all: &["dune-project"],
        any: &[],
        configure: None,
        compile: Some("dune build"),
        test: Some("dune runtest"),
        install: Some("dune install"),
        package: Some("dune build @install"),
        run: Some("dune exec"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "julia",
        all: &["Project.toml"],
        any: &[],
        configure: None,
        compile: Some("julia --project=@. -e 'import Pkg; Pkg.precompile(); Pkg.build()'"),
        test: Some("julia --project=@. -e 'import Pkg; Pkg.test()' --check-bounds=yes"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "elm",
        all: &["elm.json"],
        any: &[],
        configure: None,
        compile: Some("elm make"),
        test: None,
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "flutter",
        all: &["pubspec.yaml"],
        any: &[],
        configure: None,
        compile: Some("flutter build"),
        test: Some("flutter test"),
        install: None,
        package: None,
        run: Some("flutter run"),
        test_prefix: None,
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "dart",
        all: &["pubspec.yaml"],
        any: &[],
        configure: None,
        compile: Some("dart pub get"),
        test: Some("dart test"),
        install: None,
        package: None,
        run: Some("dart run"),
        test_prefix: None,
        test_suffix: Some("_test.dart"),
    },
    ProjectType {
        name: "racket",
        all: &["info.rkt"],
        any: &[],
        configure: None,
        compile: None,
        test: Some("raco test ."),
        install: Some("raco pkg install"),
        package: Some("raco pkg create --source $(pwd)"),
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "rust-cargo",
        all: &["Cargo.toml"],
        any: &[],
        configure: None,
        compile: Some("cargo build"),
        test: Some("cargo test"),
        install: None,
        package: None,
        run: Some("cargo run"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "haskell-stack",
        all: &["stack.yaml"],
        any: &[],
        configure: None,
        compile: Some("stack build"),
        test: Some("stack build --test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("Spec"),
    },
    ProjectType {
        name: "r",
        all: &["DESCRIPTION"],
        any: &[],
        configure: None,
        compile: Some("R CMD INSTALL --with-keep.source ."),
        test: None,
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "emacs-eldev",
        all: &["Eldev"],
        any: &[],
        configure: None,
        compile: Some("eldev compile"),
        test: Some("eldev test"),
        install: None,
        package: Some("eldev package"),
        run: Some("eldev emacs"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "emacs-eask",
        all: &["Eask"],
        any: &[],
        configure: None,
        compile: Some("eask install"),
        test: Some("eask test"),
        install: None,
        package: None,
        run: None,
        test_prefix: Some("test-"),
        test_suffix: Some("-test"),
    },
    ProjectType {
        name: "emacs-cask",
        all: &["Cask"],
        any: &[],
        configure: None,
        compile: Some("cask install"),
        test: None,
        install: None,
        package: None,
        run: None,
        test_prefix: Some("test-"),
        test_suffix: Some("-test"),
    },
    ProjectType {
        name: "crystal-spec",
        all: &["shard.yml"],
        any: &[],
        configure: None,
        compile: None,
        test: Some("crystal spec"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("_spec"),
    },
    ProjectType {
        name: "rails-rspec",
        all: &["Gemfile", "app", "lib", "db", "config", "spec"],
        any: &[],
        configure: None,
        compile: Some("bundle exec rake"),
        test: Some("bundle exec rspec"),
        install: None,
        package: None,
        run: Some("bundle exec rails server"),
        test_prefix: None,
        test_suffix: Some("_spec"),
    },
    ProjectType {
        name: "rails-test",
        all: &["Gemfile", "app", "lib", "db", "config", "test"],
        any: &[],
        configure: None,
        compile: Some("bundle exec rake"),
        test: Some("bundle exec rake test"),
        install: None,
        package: None,
        run: Some("bundle exec rails server"),
        test_prefix: None,
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "ruby-test",
        all: &["Gemfile", "lib", "test"],
        any: &[],
        configure: None,
        compile: Some("bundle exec rake"),
        test: Some("bundle exec rake test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "ruby-rspec",
        all: &["Gemfile", "lib", "spec"],
        any: &[],
        configure: None,
        compile: Some("bundle exec rake"),
        test: Some("bundle exec rspec"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("_spec"),
    },
    ProjectType {
        name: "babashka",
        all: &["bb.edn"],
        any: &[],
        configure: None,
        compile: None,
        test: None,
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "clojure-cli",
        all: &["deps.edn"],
        any: &[],
        configure: None,
        compile: None,
        test: None,
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "boot-clj",
        all: &["build.boot"],
        any: &[],
        configure: None,
        compile: Some("boot aot"),
        test: Some("boot test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "lein-midje",
        all: &["project.clj", ".midje.clj"],
        any: &[],
        configure: None,
        compile: Some("lein compile"),
        test: Some("lein midje"),
        install: None,
        package: None,
        run: None,
        test_prefix: Some("t_"),
        test_suffix: None,
    },
    ProjectType {
        name: "lein-test",
        all: &["project.clj"],
        any: &[],
        configure: None,
        compile: Some("lein compile"),
        test: Some("lein test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "scala-cli",
        all: &["project.scala"],
        any: &[],
        configure: None,
        compile: Some("scala-cli compile ."),
        test: Some("scala-cli test ."),
        install: None,
        package: None,
        run: Some("scala-cli run ."),
        test_prefix: None,
        test_suffix: Some("Test"),
    },
    ProjectType {
        name: "bloop",
        all: &[".bloop/bloop.settings.json"],
        any: &[],
        configure: None,
        compile: Some("bloop compile root"),
        test: Some("bloop test --propagate --reporter scalac root"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("Spec"),
    },
    ProjectType {
        name: "mill",
        all: &[],
        any: &["build.sc", "build.mill"],
        configure: None,
        compile: Some("mill __.compile"),
        test: Some("mill __.test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("Test"),
    },
    ProjectType {
        name: "sbt",
        all: &["build.sbt"],
        any: &[],
        configure: None,
        compile: Some("sbt compile"),
        test: Some("sbt test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("Spec"),
    },
    ProjectType {
        name: "grails",
        all: &["application.yml", "grails-app"],
        any: &[],
        configure: None,
        compile: Some("grails package"),
        test: Some("grails test-app"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("Spec"),
    },
    ProjectType {
        name: "gradlew",
        all: &["gradlew"],
        any: &[],
        configure: None,
        compile: Some("./gradlew build"),
        test: Some("./gradlew test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("Spec"),
    },
    ProjectType {
        name: "gradle",
        all: &[],
        any: &["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"],
        configure: None,
        compile: Some("gradle build"),
        test: Some("gradle test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("Spec"),
    },
    ProjectType {
        name: "maven",
        all: &["pom.xml"],
        any: &[],
        configure: None,
        compile: Some("mvn -B clean install"),
        test: Some("mvn -B test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("Test"),
    },
    ProjectType {
        name: "django",
        all: &["manage.py"],
        any: &[],
        configure: None,
        compile: Some("python manage.py collectstatic"),
        test: Some("python manage.py test"),
        install: None,
        package: None,
        run: Some("python manage.py runserver"),
        test_prefix: Some("test_"),
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "python-uv",
        all: &["uv.lock"],
        any: &[],
        configure: None,
        compile: Some("uv build"),
        test: Some("uv run pytest"),
        install: Some("uv sync"),
        package: None,
        run: None,
        test_prefix: Some("test_"),
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "python-pdm",
        all: &["pdm.lock"],
        any: &[],
        configure: None,
        compile: Some("pdm build"),
        test: Some("pdm run pytest"),
        install: Some("pdm install"),
        package: None,
        run: None,
        test_prefix: Some("test_"),
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "python-poetry",
        all: &["poetry.lock"],
        any: &[],
        configure: None,
        compile: Some("poetry build"),
        test: Some("poetry run pytest"),
        install: None,
        package: None,
        run: None,
        test_prefix: Some("test_"),
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "python-pipenv",
        all: &["Pipfile"],
        any: &[],
        configure: None,
        compile: Some("pipenv run build"),
        test: Some("pipenv run test"),
        install: None,
        package: None,
        run: None,
        test_prefix: Some("test_"),
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "python-tox",
        all: &["tox.ini"],
        any: &[],
        configure: None,
        compile: Some("tox -r --notest"),
        test: Some("tox"),
        install: None,
        package: None,
        run: None,
        test_prefix: Some("test_"),
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "python-toml",
        all: &["pyproject.toml"],
        any: &[],
        configure: None,
        compile: Some("python -m build"),
        test: Some("python -m unittest discover"),
        install: None,
        package: None,
        run: None,
        test_prefix: Some("test_"),
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "python-pkg",
        all: &["setup.py"],
        any: &[],
        configure: None,
        compile: Some("python -m build"),
        test: Some("python -m unittest discover"),
        install: None,
        package: None,
        run: None,
        test_prefix: Some("test_"),
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "python-pip",
        all: &["requirements.txt"],
        any: &[],
        configure: None,
        compile: Some("pip install -r requirements.txt"),
        test: Some("python -m unittest discover"),
        install: None,
        package: None,
        run: None,
        test_prefix: Some("test_"),
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "quarto",
        all: &["_quarto.yml"],
        any: &[],
        configure: None,
        compile: Some("quarto render"),
        test: None,
        install: None,
        package: None,
        run: Some("quarto preview"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "mkdocs",
        all: &["mkdocs.yml"],
        any: &[],
        configure: None,
        compile: Some("mkdocs build"),
        test: None,
        install: None,
        package: None,
        run: Some("mkdocs serve"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "hugo",
        all: &[],
        any: &["hugo.toml", "hugo.yaml", "hugo.json"],
        configure: None,
        compile: Some("hugo"),
        test: None,
        install: None,
        package: None,
        run: Some("hugo server"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "zola",
        all: &["config.toml", "content"],
        any: &[],
        configure: None,
        compile: Some("zola build"),
        test: None,
        install: None,
        package: None,
        run: Some("zola serve"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "jekyll",
        all: &["_config.yml"],
        any: &[],
        configure: None,
        compile: Some("bundle exec jekyll build"),
        test: None,
        install: None,
        package: None,
        run: Some("bundle exec jekyll serve"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "php-laravel",
        all: &["composer.json", "artisan"],
        any: &[],
        configure: None,
        compile: Some("composer install"),
        test: Some("php artisan test"),
        install: None,
        package: None,
        run: Some("php artisan serve"),
        test_prefix: None,
        test_suffix: Some("Test"),
    },
    ProjectType {
        name: "php-symfony",
        all: &["composer.json"],
        any: &["bin/console", "app/console"],
        configure: None,
        compile: Some("composer install"),
        test: Some("vendor/bin/phpunit"),
        install: None,
        package: None,
        run: Some("symfony serve"),
        test_prefix: None,
        test_suffix: Some("Test"),
    },
    ProjectType {
        name: "php-composer",
        all: &["composer.json"],
        any: &[],
        configure: None,
        compile: Some("composer install"),
        test: Some("vendor/bin/phpunit"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("Test"),
    },
    ProjectType {
        name: "turborepo",
        all: &["turbo.json"],
        any: &[],
        configure: None,
        compile: Some("turbo build"),
        test: Some("turbo test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some(".test"),
    },
    ProjectType {
        name: "nx",
        all: &["nx.json"],
        any: &[],
        configure: None,
        compile: Some("npx nx run-many -t build"),
        test: Some("npx nx run-many -t test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some(".spec"),
    },
    ProjectType {
        name: "nextjs",
        all: &[],
        any: &["next.config.js", "next.config.mjs", "next.config.ts"],
        configure: None,
        compile: Some("next build"),
        test: Some("npm test"),
        install: None,
        package: None,
        run: Some("next dev"),
        test_prefix: None,
        test_suffix: Some(".test"),
    },
    ProjectType {
        name: "angular",
        all: &[],
        any: &["angular.json", ".angular-cli.json"],
        configure: None,
        compile: Some("ng build"),
        test: Some("ng test"),
        install: None,
        package: None,
        run: Some("ng serve"),
        test_prefix: None,
        test_suffix: Some(".spec"),
    },
    ProjectType {
        name: "deno",
        all: &[],
        any: &["deno.json", "deno.jsonc"],
        configure: None,
        compile: Some("deno check ."),
        test: Some("deno test"),
        install: None,
        package: None,
        run: Some("deno task start"),
        test_prefix: None,
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "bun",
        all: &["package.json"],
        any: &[],
        configure: None,
        compile: Some("bun install"),
        test: Some("bun test"),
        install: None,
        package: None,
        run: Some("bun run start"),
        test_prefix: None,
        test_suffix: Some(".test"),
    },
    ProjectType {
        name: "pnpm",
        all: &["package.json", "pnpm-lock.yaml"],
        any: &[],
        configure: None,
        compile: Some("pnpm install && pnpm build"),
        test: Some("pnpm test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some(".test"),
    },
    ProjectType {
        name: "yarn",
        all: &["package.json", "yarn.lock"],
        any: &[],
        configure: None,
        compile: Some("yarn && yarn build"),
        test: Some("yarn test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some(".test"),
    },
    ProjectType {
        name: "npm",
        all: &["package.json", "package-lock.json"],
        any: &[],
        configure: None,
        compile: Some("npm install && npm run build"),
        test: Some("npm test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some(".test"),
    },
    ProjectType {
        name: "gulp",
        all: &["gulpfile.js"],
        any: &[],
        configure: None,
        compile: Some("gulp"),
        test: Some("gulp test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "grunt",
        all: &["Gruntfile.js"],
        any: &[],
        configure: None,
        compile: Some("grunt"),
        test: Some("grunt test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "node",
        all: &["package.json"],
        any: &[],
        configure: None,
        compile: Some("npm install"),
        test: Some("npm test"),
        install: None,
        package: None,
        run: Some("npm start"),
        test_prefix: None,
        test_suffix: Some(".test"),
    },
    ProjectType {
        name: "gleam",
        all: &["gleam.toml"],
        any: &[],
        configure: None,
        compile: Some("gleam build"),
        test: Some("gleam test"),
        install: None,
        package: None,
        run: Some("gleam run"),
        test_prefix: None,
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "elixir",
        all: &["mix.exs"],
        any: &[],
        configure: None,
        compile: Some("mix compile"),
        test: Some("mix test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "erlang-mk",
        all: &["erlang.mk"],
        any: &[],
        configure: None,
        compile: Some("make"),
        test: Some("make tests"),
        install: None,
        package: None,
        run: Some("make run"),
        test_prefix: None,
        test_suffix: Some("_SUITE"),
    },
    ProjectType {
        name: "rebar",
        all: &["rebar.config"],
        any: &[],
        configure: None,
        compile: Some("rebar3 compile"),
        test: Some("rebar3 do eunit,ct"),
        install: Some("rebar3 release"),
        package: Some("rebar3 tar"),
        run: Some("rebar3 shell"),
        test_prefix: None,
        test_suffix: Some("_SUITE"),
    },
    ProjectType {
        name: "go",
        all: &["go.mod"],
        any: &[],
        configure: None,
        compile: Some("go build"),
        test: Some("go test ./..."),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("_test"),
    },
    ProjectType {
        name: "go-task",
        all: &[],
        any: &["Taskfile.yml", "Taskfile.yaml", "Taskfile.dist.yml", "Taskfile.dist.yaml"],
        configure: None,
        compile: Some("task build"),
        test: Some("task test"),
        install: Some("task install"),
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "cmake",
        all: &["CMakeLists.txt"],
        any: &[],
        configure: None,
        compile: None,
        test: None,
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "gnumake",
        all: &["GNUmakefile"],
        any: &[],
        configure: None,
        compile: Some("make"),
        test: Some("make test"),
        install: Some("make install"),
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "make",
        all: &["Makefile"],
        any: &["Makefile", "makefile", "GNUmakefile"],
        configure: None,
        compile: Some("make"),
        test: Some("make test"),
        install: Some("make install"),
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "debian",
        all: &["debian/control"],
        any: &[],
        configure: None,
        compile: Some("debuild -uc -us"),
        test: None,
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "pants",
        all: &["pants.toml"],
        any: &[],
        configure: None,
        compile: Some("pants package ::"),
        test: Some("pants test ::"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "buck2",
        all: &[".buckconfig"],
        any: &[],
        configure: None,
        compile: Some("buck2 build //..."),
        test: Some("buck2 test //..."),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "bazel",
        all: &[],
        any: &["MODULE.bazel", "WORKSPACE", "WORKSPACE.bazel"],
        configure: None,
        compile: Some("bazel build //..."),
        test: Some("bazel test //..."),
        install: None,
        package: None,
        run: Some("bazel run"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "nix-flake",
        all: &["flake.nix"],
        any: &[],
        configure: None,
        compile: Some("nix build"),
        test: Some("nix flake check"),
        install: None,
        package: None,
        run: Some("nix run"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "nix",
        all: &["default.nix"],
        any: &[],
        configure: None,
        compile: Some("nix-build"),
        test: Some("nix-build"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "meson",
        all: &["meson.build", "build"],
        any: &[],
        configure: Some("meson %s"),
        compile: Some("ninja"),
        test: Some("ninja test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "scons",
        all: &["SConstruct"],
        any: &[],
        configure: None,
        compile: Some("scons"),
        test: Some("scons test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: Some("test"),
    },
    ProjectType {
        name: "xmake",
        all: &["xmake.lua"],
        any: &[],
        configure: None,
        compile: Some("xmake build"),
        test: Some("xmake test"),
        install: Some("xmake install"),
        package: None,
        run: Some("xmake run"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "just",
        all: &[],
        any: &["justfile", ".justfile", "Justfile"],
        configure: None,
        compile: Some("just build"),
        test: Some("just test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "mise",
        all: &[],
        any: &["mise.toml", ".mise.toml"],
        configure: None,
        compile: Some("mise run build"),
        test: Some("mise run test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "pulumi",
        all: &["Pulumi.yaml"],
        any: &[],
        configure: None,
        compile: Some("pulumi preview"),
        test: None,
        install: None,
        package: None,
        run: Some("pulumi up"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "helm",
        all: &["Chart.yaml"],
        any: &[],
        configure: None,
        compile: Some("helm template ."),
        test: Some("helm lint"),
        install: Some("helm install"),
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "ansible",
        all: &["ansible.cfg"],
        any: &[],
        configure: None,
        compile: None,
        test: Some("ansible-lint"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "docker-compose",
        all: &[],
        any: &["compose.yaml", "compose.yml", "docker-compose.yaml", "docker-compose.yml"],
        configure: None,
        compile: Some("docker compose build"),
        test: None,
        install: None,
        package: None,
        run: Some("docker compose up"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "nim-nimble",
        all: &["?*.nimble"],
        any: &[],
        configure: None,
        compile: Some("nimble --noColor build --colors:off"),
        test: Some("nimble --noColor test -d:nimUnittestColor:off --colors:off"),
        install: Some("nimble --noColor install --colors:off"),
        package: None,
        run: Some("nimble --noColor run --colors:off"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "xcode",
        all: &[],
        any: &["?*.xcworkspace", "?*.xcodeproj"],
        configure: None,
        compile: Some("xcodebuild build"),
        test: Some("xcodebuild test"),
        install: None,
        package: None,
        run: None,
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "terraform",
        all: &["?*.tf"],
        any: &[],
        configure: Some("terraform init"),
        compile: Some("terraform plan"),
        test: Some("terraform validate"),
        install: None,
        package: None,
        run: Some("terraform apply"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "dotnet-sln",
        all: &[],
        any: &["?*.sln", "?*.slnx"],
        configure: None,
        compile: Some("dotnet build"),
        test: Some("dotnet test"),
        install: None,
        package: None,
        run: Some("dotnet run"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "dotnet",
        all: &[],
        any: &["?*.csproj", "?*.fsproj"],
        configure: None,
        compile: Some("dotnet build"),
        test: Some("dotnet test"),
        install: None,
        package: None,
        run: Some("dotnet run"),
        test_prefix: None,
        test_suffix: None,
    },
    ProjectType {
        name: "haskell-cabal",
        all: &[],
        any: &["?*.cabal"],
        configure: None,
        compile: Some("cabal build"),
        test: Some("cabal test"),
        install: None,
        package: None,
        run: Some("cabal run"),
        test_prefix: None,
        test_suffix: Some("Spec"),
    },
];

/// Whether `root` holds `marker` — a plain file name, or projectile's `?*.ext`
/// wildcard, which asks whether any file with that extension is there.
fn has_marker(root: &Path, marker: &str) -> bool {
    match marker.strip_prefix("?*") {
        Some(ext) => std::fs::read_dir(root).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.ends_with(ext))
            })
        }),
        None => root.join(marker).exists(),
    }
}

/// The project type of `root` — the first row of [`PROJECT_TYPES`] whose markers
/// are satisfied, which is how `projectile-project-type` resolves it.
pub(crate) fn project_type_of(root: &Path) -> Option<&'static ProjectType> {
    PROJECT_TYPES.iter().find(|kind| {
        let all_ok = !kind.all.is_empty() && kind.all.iter().all(|m| has_marker(root, m));
        let any_ok = kind.any.iter().any(|m| has_marker(root, m));
        all_ok || any_ok
    })
}

/// `projectile-project-type`: the kind of project at `root`, by name. "generic"
/// when no registered type's markers are there, which is what projectile calls
/// a project it has no table row for.
pub(crate) fn project_type(root: &Path) -> &'static str {
    project_type_of(root).map(|kind| kind.name).unwrap_or("generic")
}

/// The lifecycle phases projectile runs external commands for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Configure,
    Compile,
    Test,
    Install,
    Package,
    Run,
}

impl Phase {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Phase::Configure => "configure",
            Phase::Compile => "compile",
            Phase::Test => "test",
            Phase::Install => "install",
            Phase::Package => "package",
            Phase::Run => "run",
        }
    }

    /// The project type's default command for this phase.
    fn default_command(self, kind: &ProjectType) -> Option<&'static str> {
        match self {
            Phase::Configure => kind.configure,
            Phase::Compile => kind.compile,
            Phase::Test => kind.test,
            Phase::Install => kind.install,
            Phase::Package => kind.package,
            Phase::Run => kind.run,
        }
    }
}

/// Where the per-project lifecycle commands are remembered. Projectile keeps the
/// same map in `projectile-project-compilation-cmd` and friends, so a command you
/// typed once is what the next `C-c p c c` runs.
fn command_cache_file() -> PathBuf {
    zmax_loader::config_dir().join("projectile-commands")
}

/// The cache as `(root, phase, command)` rows.
fn command_cache() -> Vec<(String, String, String)> {
    std::fs::read_to_string(command_cache_file())
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            Some((
                parts.next()?.to_string(),
                parts.next()?.to_string(),
                parts.next()?.to_string(),
            ))
        })
        .collect()
}

fn write_command_cache(rows: &[(String, String, String)]) -> std::io::Result<()> {
    let file = command_cache_file();
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body: String = rows
        .iter()
        .map(|(root, phase, cmd)| format!("{root}\t{phase}\t{cmd}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(file, body)
}

/// The command remembered for `root`'s `phase`, if any.
pub(crate) fn cached_command(root: &Path, phase: Phase) -> Option<String> {
    let root = root.to_string_lossy().into_owned();
    command_cache()
        .into_iter()
        .find(|(r, p, _)| *r == root && p == phase.name())
        .map(|(_, _, cmd)| cmd)
}

/// Remember `command` as `root`'s command for `phase`.
pub(crate) fn remember_command(root: &Path, phase: Phase, command: &str) {
    let root = root.to_string_lossy().into_owned();
    let mut rows = command_cache();
    rows.retain(|(r, p, _)| !(*r == root && p == phase.name()));
    rows.push((root, phase.name().to_string(), command.to_string()));
    let _ = write_command_cache(&rows);
}

/// The command a phase runs in `root`: what was remembered for it, else the
/// project type's default. `None` when the type has no command for that phase —
/// projectile then asks, which is what the caller reports.
pub(crate) fn phase_command(root: &Path, phase: Phase) -> Option<String> {
    cached_command(root, phase).or_else(|| {
        project_type_of(root)
            .and_then(|kind| phase.default_command(kind))
            .map(str::to_string)
    })
}

/// The last external command projectile ran, for `projectile-repeat-last-command`.
fn last_command_file() -> PathBuf {
    zmax_loader::config_dir().join("projectile-last-command")
}

/// Run `command` in `root` through the compilation path, remembering it as this
/// project's command for `phase` — `projectile-run-compilation` in one step.
fn run_phase(
    cx: &mut compositor::Context,
    root: &Path,
    phase: Phase,
    command: &str,
) -> anyhow::Result<()> {
    remember_command(root, phase, command);
    let _ = std::fs::write(
        last_command_file(),
        format!("{}\t{command}", root.to_string_lossy()),
    );
    // Projectile runs the command *in the project root*; `:compile` runs it in
    // the editor's working directory, so the root is prefixed.
    let in_root = format!("cd {} && {command}", shell_quote(&root.to_string_lossy()));
    crate::commands::typed::run_compile_command(cx, &in_root)
}

/// Single-quote `text` for the shell, as `shell-quote-argument` does.
pub(crate) fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The body every `projectile-<phase>-project` command shares: run the phase's
/// command (an explicit argument overrides and is remembered).
fn lifecycle(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
    phase: Phase,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root();
    let command = match args.first() {
        Some(_) => args
            .into_iter()
            .map(|arg| arg.to_string())
            .collect::<Vec<_>>()
            .join(" "),
        None => phase_command(&root, phase).ok_or_else(|| {
            anyhow::anyhow!(
                "projectile-{}-project: no {} command for a {} project — pass one",
                phase.name(),
                phase.name(),
                project_type(&root)
            )
        })?,
    };
    run_phase(cx, &root, phase, &command)
}

/// `projectile-configure-project` (`C-c p c o`).
pub(crate) fn configure_project(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    lifecycle(cx, args, event, Phase::Configure)
}

/// `projectile-compile-project` (`C-c p c c`).
pub(crate) fn compile_project(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    lifecycle(cx, args, event, Phase::Compile)
}

/// `projectile-test-project` (`C-c p c t`).
pub(crate) fn test_project(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    lifecycle(cx, args, event, Phase::Test)
}

/// `projectile-install-project` (`C-c p c i`).
pub(crate) fn install_project(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    lifecycle(cx, args, event, Phase::Install)
}

/// `projectile-package-project` (`C-c p c p`).
pub(crate) fn package_project(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    lifecycle(cx, args, event, Phase::Package)
}

/// `projectile-run-project` (`C-c p c r`).
pub(crate) fn run_project(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    lifecycle(cx, args, event, Phase::Run)
}

/// `projectile-repeat-last-command`: "Run last projectile external command."
pub(crate) fn repeat_last_command(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let saved = std::fs::read_to_string(last_command_file()).unwrap_or_default();
    let Some((root, command)) = saved.split_once('\t') else {
        anyhow::bail!("projectile-repeat-last-command: nothing has been run yet");
    };
    let in_root = format!("cd {} && {command}", shell_quote(root));
    crate::commands::typed::run_compile_command(cx, &in_root)
}

/// `projectile-discard-command-cache`: "Discard the cached lifecycle commands for
/// the current project."
pub(crate) fn discard_command_cache(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root().to_string_lossy().into_owned();
    let mut rows = command_cache();
    let before = rows.len();
    rows.retain(|(r, _, _)| *r != root);
    let dropped = before - rows.len();
    write_command_cache(&rows)?;
    cx.editor.set_status(format!(
        "Discarded {dropped} cached command(s) for {root}"
    ));
    Ok(())
}

/// `projectile-run-shell-command-in-root` (`C-c p !`): "Invoke `shell-command' in
/// the project's root."
pub(crate) fn run_shell_command_in_root(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    if args.is_empty() {
        anyhow::bail!("projectile-run-shell-command-in-root: needs a command");
    }
    let root = project_root();
    let command: Vec<String> = args.into_iter().map(|a| a.to_string()).collect();
    let in_root = format!(
        "cd {} && {}",
        shell_quote(&root.to_string_lossy()),
        command.join(" ")
    );
    crate::commands::typed::run_compile_command(cx, &in_root)
}

/// The nearest subproject of the current file: the closest ancestor *below* the
/// project root that is itself a project — what
/// `projectile--run-subproject-phase` runs its command in.
pub(crate) fn nearest_subproject(file: &Path, root: &Path) -> Option<PathBuf> {
    let mut dir = file.parent()?;
    let mut found = None;
    while dir.starts_with(root) && dir != root {
        if is_project_root(dir) {
            found = Some(dir.to_path_buf());
            break;
        }
        dir = dir.parent()?;
    }
    found
}

/// The body of the `projectile-*-subproject` commands: the phase's command, run
/// in the nearest subproject rather than the project root.
fn subproject_lifecycle(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
    phase: Phase,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root();
    let file = zmax_view::doc!(cx.editor)
        .path()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| anyhow::anyhow!("projectile-{}-subproject: the buffer is not visiting a file", phase.name()))?;
    let sub = nearest_subproject(&file, &root).ok_or_else(|| {
        anyhow::anyhow!(
            "projectile-{}-subproject: {} is not inside a subproject",
            phase.name(),
            file.display()
        )
    })?;
    let command = match args.first() {
        Some(_) => args
            .into_iter()
            .map(|arg| arg.to_string())
            .collect::<Vec<_>>()
            .join(" "),
        None => phase_command(&sub, phase).ok_or_else(|| {
            anyhow::anyhow!(
                "projectile-{}-subproject: no {} command for a {} project — pass one",
                phase.name(),
                phase.name(),
                project_type(&sub)
            )
        })?,
    };
    run_phase(cx, &sub, phase, &command)
}

/// `projectile-configure-subproject` (`C-c p c m o`).
pub(crate) fn configure_subproject(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    subproject_lifecycle(cx, args, event, Phase::Configure)
}

/// `projectile-compile-subproject` (`C-c p c m c`).
pub(crate) fn compile_subproject(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    subproject_lifecycle(cx, args, event, Phase::Compile)
}

/// `projectile-test-subproject` (`C-c p c m t`).
pub(crate) fn test_subproject(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    subproject_lifecycle(cx, args, event, Phase::Test)
}

/// `projectile-install-subproject` (`C-c p c m i`).
pub(crate) fn install_subproject(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    subproject_lifecycle(cx, args, event, Phase::Install)
}

/// `projectile-package-subproject` (`C-c p c m p`).
pub(crate) fn package_subproject(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    subproject_lifecycle(cx, args, event, Phase::Package)
}

/// `projectile-run-subproject` (`C-c p c m r`).
pub(crate) fn run_subproject(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    subproject_lifecycle(cx, args, event, Phase::Run)
}

/// `projectile-find-file-in-subproject` (`C-c p c m f`): "Jump to a file in one of
/// the current project's subprojects."
pub(crate) fn find_file_in_subproject(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root();
    let subprojects = projects_in_directory(&root, 3);
    let mut files = Vec::new();
    for sub in subprojects.iter().filter(|p| **p != root) {
        files.extend(project_files(sub, false));
    }
    if files.is_empty() {
        anyhow::bail!("projectile-find-file-in-subproject: this project has no subprojects");
    }
    crate::commands::pick_paths(cx, "subproject file", root, files);
    Ok(())
}

/// Every file in the project, as `ignore`'s walker sees it. `all` includes what
/// the VCS and ignore files exclude, which is `projectile-find-file-all`'s
/// difference from `projectile-find-file`.
pub(crate) fn project_files(root: &Path, all: bool) -> Vec<PathBuf> {
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(!all)
        .git_ignore(!all)
        .git_global(!all)
        .git_exclude(!all)
        .ignore(!all);
    let mut files: Vec<PathBuf> = builder
        .build()
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|t| t.is_file()))
        .map(|entry| entry.into_path())
        .collect();
    files.sort();
    files
}

/// Whether `path` looks like a test file. Projectile decides with the project
/// type's `:test-prefix` / `:test-suffix`, so those are consulted first and the
/// shapes shared across its table are the fallback.
pub(crate) fn is_test_file(root: &Path, path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let stem = name.split('.').next().unwrap_or(name);
    if let Some(kind) = project_type_of(root) {
        if kind.test_prefix.is_some_and(|p| stem.starts_with(p))
            || kind.test_suffix.is_some_and(|s| stem.ends_with(s) || name.contains(s))
        {
            return true;
        }
    }
    let in_test_dir = path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("test" | "tests" | "spec" | "__tests__")
        )
    });
    in_test_dir
        || stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
        || stem.ends_with("Test")
        || stem.ends_with("Spec")
        || name.contains(".test.")
        || name.contains(".spec.")
}

/// `projectile-find-file`: "Jump to a project's file using completion."
pub(crate) fn find_file(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root();
    crate::commands::pick_paths(cx, "file", root.clone(), project_files(&root, false));
    Ok(())
}

/// `projectile-find-file-all`: "Jump to any file in the project, ignoring VCS and
/// projectile ignores."
pub(crate) fn find_file_all(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root();
    crate::commands::pick_paths(cx, "file (all)", root.clone(), project_files(&root, true));
    Ok(())
}

/// `projectile-find-test-file`: "Jump to a project's test file using completion."
pub(crate) fn find_test_file(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root();
    let files: Vec<PathBuf> = project_files(&root, false)
        .into_iter()
        .filter(|p| is_test_file(&root, p))
        .collect();
    if files.is_empty() {
        anyhow::bail!("projectile-find-test-file: no test files in this project");
    }
    crate::commands::pick_paths(cx, "test file", root, files);
    Ok(())
}

/// `projectile-find-changed-file`: "Jump to a file changed in the current
/// project" — what the VCS reports as modified, plus the untracked ones.
pub(crate) fn find_changed_file(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root();
    let git = |args: &[&str]| -> Vec<String> {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut files: Vec<PathBuf> = git(&["diff", "--name-only", "HEAD"])
        .into_iter()
        .chain(git(&["ls-files", "--others", "--exclude-standard"]))
        .map(|rel| root.join(rel))
        .collect();
    files.sort();
    files.dedup();
    if files.is_empty() {
        anyhow::bail!("projectile-find-changed-file: no changed files in this project");
    }
    crate::commands::pick_paths(cx, "changed file", root, files);
    Ok(())
}

/// `projectile-find-other-file`: "Switch between files with the same name but
/// different extensions" — the C `.h`/`.c` pairing, generalised.
pub(crate) fn find_other_file(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let here = zmax_view::doc!(cx.editor).path().map(|p| p.to_path_buf());
    let Some(here) = here else {
        anyhow::bail!("projectile-find-other-file: the buffer is not visiting a file");
    };
    let Some(stem) = here.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
        anyhow::bail!("projectile-find-other-file: no file name to match");
    };
    let root = project_root();
    let others: Vec<PathBuf> = project_files(&root, false)
        .into_iter()
        .filter(|p| *p != here && p.file_stem().and_then(|s| s.to_str()) == Some(stem.as_str()))
        .collect();
    match others.len() {
        0 => anyhow::bail!("projectile-find-other-file: no other file named {stem}"),
        1 => {
            let target = others[0].clone();
            if let Err(e) = cx.editor.open(&target, zmax_view::editor::Action::Replace) {
                anyhow::bail!("projectile-find-other-file: {e}");
            }
            Ok(())
        }
        _ => {
            crate::commands::pick_paths(cx, "other file", root, others);
            Ok(())
        }
    }
}

/// `projectile-find-dir`: "Jump to a project's directory using completion."
pub(crate) fn find_dir(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root();
    let mut dirs: Vec<PathBuf> = ignore::WalkBuilder::new(&root)
        .build()
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|t| t.is_dir()))
        .map(|entry| entry.into_path())
        .collect();
    dirs.sort();
    crate::commands::pick_paths(cx, "directory", root, dirs);
    Ok(())
}

/// `projectile-find-file-in-directory`: "Jump to a file in a (maybe regular)
/// DIRECTORY" — the directory need not be a project.
pub(crate) fn find_file_in_directory(
    cx: &mut compositor::Context,
    args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let Some(dir) = args.first() else {
        anyhow::bail!("projectile-find-file-in-directory: needs a directory");
    };
    let dir = expand_home(dir);
    if !dir.is_dir() {
        anyhow::bail!(
            "projectile-find-file-in-directory: {} is not a directory",
            dir.display()
        );
    }
    crate::commands::pick_paths(cx, "file", dir.clone(), project_files(&dir, false));
    Ok(())
}

/// `projectile-find-file-in-known-projects`: "Jump to a file in any of the known
/// projects."
pub(crate) fn find_file_in_known_projects(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let projects = known_projects();
    if projects.is_empty() {
        anyhow::bail!("projectile-find-file-in-known-projects: no known projects yet");
    }
    let mut files = Vec::new();
    for project in &projects {
        files.extend(project_files(Path::new(project), false));
    }
    // The paths come from several roots, so they are shown in full.
    crate::commands::pick_paths(cx, "file", PathBuf::new(), files);
    Ok(())
}

/// The buffers belonging to `root` — projectile's `projectile-project-buffers`.
pub(crate) fn project_buffer_ids(
    editor: &zmax_view::Editor,
    root: &Path,
) -> Vec<zmax_view::DocumentId> {
    editor
        .documents()
        .filter(|doc| doc.path().is_some_and(|path| path.starts_with(root)))
        .map(|doc| doc.id())
        .collect()
}

/// `projectile-kill-buffers`: "Kill project buffers."
pub(crate) fn kill_buffers(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root();
    let ids = project_buffer_ids(cx.editor, &root);
    if ids.is_empty() {
        anyhow::bail!("projectile-kill-buffers: no buffers in this project");
    }
    let mut killed = 0;
    let mut modified = 0;
    for id in ids {
        // A modified buffer is left alone rather than losing the edit — emacs
        // asks, and a `:` command has nobody to ask.
        if cx
            .editor
            .documents
            .get(&id)
            .is_some_and(|doc| doc.is_modified())
        {
            modified += 1;
            continue;
        }
        if cx.editor.close_document(id, false).is_ok() {
            killed += 1;
        }
    }
    cx.editor.set_status(match modified {
        0 => format!("Killed {killed} project buffer(s)"),
        n => format!("Killed {killed} project buffer(s); {n} modified buffer(s) left open"),
    });
    Ok(())
}

/// `projectile-save-project-buffers`: "Save all project buffers."
pub(crate) fn save_project_buffers(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root();
    let ids: Vec<zmax_view::DocumentId> = project_buffer_ids(cx.editor, &root)
        .into_iter()
        .filter(|id| {
            cx.editor
                .documents
                .get(id)
                .is_some_and(|doc| doc.is_modified())
        })
        .collect();
    if ids.is_empty() {
        cx.editor.set_status("No modified project buffers");
        return Ok(());
    }
    let saved = ids.len();
    for id in ids {
        cx.editor.save::<PathBuf>(id, None, false)?;
    }
    cx.editor
        .set_status(format!("Saved {saved} project buffer(s)"));
    Ok(())
}

/// Move to the next (or previous) buffer of the current project — projectile's
/// `projectile-next-project-buffer` / `projectile-previous-project-buffer`.
fn step_project_buffer(cx: &mut compositor::Context, forward: bool) -> anyhow::Result<()> {
    let root = project_root();
    let ids = project_buffer_ids(cx.editor, &root);
    if ids.len() < 2 {
        anyhow::bail!("This project has only one buffer");
    }
    let current = zmax_view::doc!(cx.editor).id();
    let at = ids.iter().position(|id| *id == current).unwrap_or(0);
    let next = if forward {
        (at + 1) % ids.len()
    } else {
        (at + ids.len() - 1) % ids.len()
    };
    cx.editor
        .switch(ids[next], zmax_view::editor::Action::Replace);
    Ok(())
}

/// `projectile-next-project-buffer`.
pub(crate) fn next_project_buffer(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    step_project_buffer(cx, true)
}

/// `projectile-previous-project-buffer`.
pub(crate) fn previous_project_buffer(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    step_project_buffer(cx, false)
}

/// `projectile-toggle-project-read-only`: "Toggle project read only" — every
/// buffer of the project at once.
pub(crate) fn toggle_project_read_only(
    cx: &mut compositor::Context,
    _args: zmax_core::command_line::Args,
    event: PromptEvent,
) -> anyhow::Result<()> {
    if event != PromptEvent::Validate {
        return Ok(());
    }
    let root = project_root();
    let ids = project_buffer_ids(cx.editor, &root);
    if ids.is_empty() {
        anyhow::bail!("projectile-toggle-project-read-only: no buffers in this project");
    }
    // The toggle follows the current buffer's state, so one press flips the whole
    // project the same way.
    let on = !zmax_view::doc!(cx.editor).readonly;
    for id in &ids {
        if let Some(doc) = cx.editor.documents.get_mut(id) {
            doc.readonly = on;
        }
    }
    cx.editor.set_status(format!(
        "{} project buffer(s) are now {}",
        ids.len(),
        if on { "read-only" } else { "writable" }
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The project-type table is projectile's own, all 97 rows of it, resolved in
    /// projectile's order (later registrations win). A Cargo project answers
    /// `cargo build` / `cargo test` / `cargo run`, which is what
    /// `projectile-register-project-type 'rust-cargo` registers.
    #[test]
    fn lifecycle_commands_come_from_projectiles_own_table() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("Cargo.toml"), "").expect("write");
        let kind = project_type_of(dir.path()).expect("a Cargo.toml is a rust-cargo project");
        assert_eq!(kind.name, "rust-cargo");
        assert_eq!(kind.compile, Some("cargo build"));
        assert_eq!(kind.test, Some("cargo test"));
        assert_eq!(kind.run, Some("cargo run"));
        assert_eq!(PROJECT_TYPES.len(), 97, "every registered type is in the table");
    }

    /// A `?*.ext` marker is projectile's wildcard: any file with that extension
    /// marks the project (`projectile-verify-file-wildcard`).
    #[test]
    fn wildcard_markers_match_by_extension() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("thing.csproj"), "").expect("write");
        let kind = project_type_of(dir.path()).expect("a .csproj marks a dotnet project");
        assert_eq!(kind.name, "dotnet");
        assert_eq!(kind.compile, Some("dotnet build"));
    }

    /// The nearest subproject is the closest project *below* the root, which is
    /// where `projectile-*-subproject` runs its command.
    #[test]
    fn nearest_subproject_is_the_closest_project_below_the_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join(".git")).expect("mkdir");
        std::fs::create_dir_all(root.join("crates/inner/src")).expect("mkdir");
        std::fs::write(root.join("crates/inner/Cargo.toml"), "").expect("write");
        let file = root.join("crates/inner/src/lib.rs");
        assert_eq!(
            nearest_subproject(&file, root),
            Some(root.join("crates/inner")),
            "the crate, not the workspace"
        );
        // A file directly under the root has no subproject.
        assert_eq!(nearest_subproject(&root.join("README.md"), root), None);
    }

    /// Test files are recognised by the project type's own `:test-prefix` /
    /// `:test-suffix` first, then by the shapes shared across projectile's table.
    #[test]
    fn test_files_follow_the_project_types_prefix_and_suffix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(root.join("go.mod"), "").expect("write");
        assert!(is_test_file(root, &root.join("thing_test.go")));
        assert!(!is_test_file(root, &root.join("thing.go")));
        // The shared fallbacks still apply.
        assert!(is_test_file(root, &root.join("tests/whatever.go")));
        assert!(is_test_file(root, &root.join("spec/thing_spec.rb")));
    }

    /// `projectile-project-type` is decided by the marker file at the root — the
    /// same signal projectile's project-type table keys off.
    #[test]
    fn project_type_comes_from_the_root_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(project_type(dir.path()), "generic");
        std::fs::write(dir.path().join("Cargo.toml"), "").expect("write");
        assert_eq!(project_type(dir.path()), "rust-cargo");
    }

    /// Discovery walks a level down and stops at the first project it finds — it
    /// does not descend into a project's own subdirectories.
    #[test]
    fn discovery_finds_projects_one_level_down_and_stops_there() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("alpha/.git")).expect("mkdir");
        std::fs::create_dir_all(root.join("alpha/nested/.git")).expect("mkdir");
        std::fs::create_dir_all(root.join("beta")).expect("mkdir");
        std::fs::write(root.join("beta/Cargo.toml"), "").expect("write");
        std::fs::create_dir_all(root.join("plain")).expect("mkdir");

        let found = projects_in_directory(root, 1);
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["alpha", "beta"], "{found:?}");
    }

    /// A directory with no marker is not a project root; one with `.git` is.
    #[test]
    fn project_roots_are_the_marker_bearing_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!is_project_root(dir.path()));
        std::fs::create_dir(dir.path().join(".git")).expect("mkdir");
        assert!(is_project_root(dir.path()));
    }
}
