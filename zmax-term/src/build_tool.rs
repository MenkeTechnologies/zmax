//! The build-tool window (JetBrains Gradle / Maven / … tool windows, the
//! `ExternalSystem.*` actions) over the build files zmax knows: Cargo, npm,
//! make, just, Gradle, Maven, CMake.
//!
//! A project is a build file at the workspace root, with the workspace members
//! it lists (Cargo `[workspace] members`, npm `workspaces`) as projects of its
//! own. Each offers tasks — the shell commands of its build tool — grouped as
//! the IDE groups them (build, verification, application, other).
//!
//! What the IDE keeps per project is kept in `build-tool.toml` in the
//! project's state directory (`run_config::project_dir`): the projects
//! detached or ignored, the members chosen for import, the task triggers
//! (before/after build, rebuild and sync), the auto-sync setting and the
//! window's display options.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The build files a project root holds, in the order the IDE's build-tool
/// panel would list their projects.
pub const BUILD_FILES: [&str; 8] = [
    "Cargo.toml",
    "package.json",
    "Makefile",
    "justfile",
    "build.gradle.kts",
    "build.gradle",
    "pom.xml",
    "CMakeLists.txt",
];

/// The tasks a build file offers, as the shell commands that run them.
/// `contents` is the file's text, read where a tool's own listing would be
/// slow or need the tool installed. Pure — unit tested.
pub fn build_file_tasks(file: &str, contents: &str) -> Vec<String> {
    let each = |tool: &str, tasks: &[&str]| tasks.iter().map(|t| format!("{tool} {t}")).collect::<Vec<_>>();
    match file {
        "Cargo.toml" => each("cargo", &["build", "check", "test", "run", "clippy", "doc", "bench", "clean"]),
        "package.json" => serde_json::from_str::<serde_json::Value>(contents)
            .ok()
            .and_then(|json| json["scripts"].as_object().cloned())
            .map(|scripts| scripts.keys().map(|s| format!("npm run {s}")).collect())
            .unwrap_or_default(),
        "Makefile" => contents
            .lines()
            .filter_map(|line| {
                let (target, _) = line.split_once(':')?;
                let valid = !target.is_empty()
                    && !target.starts_with(['.', '\t', ' ', '#'])
                    && !line[target.len()..].starts_with(":=")
                    && target.chars().all(|c| c.is_alphanumeric() || "-_/.".contains(c));
                valid.then(|| format!("make {target}"))
            })
            .collect(),
        "justfile" => contents
            .lines()
            .filter_map(|line| {
                let name = line.split([':', ' ']).next()?;
                let valid = !name.is_empty()
                    && line.contains(':')
                    && !line.contains(":=")
                    && name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
                valid.then(|| format!("just {name}"))
            })
            .collect(),
        "build.gradle.kts" | "build.gradle" => each("./gradlew", &["build", "test", "clean", "assemble", "check"]),
        "pom.xml" => each("mvn", &["compile", "test", "package", "verify", "install", "clean"]),
        "CMakeLists.txt" => vec!["cmake -S . -B build".into(), "cmake --build build".into()],
        _ => Vec::new(),
    }
}


/// A build-tool project: a build file and what it offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildProject {
    /// The build file.
    pub file: PathBuf,
    /// The tasks, as the shell commands that run them, in the file's order.
    pub tasks: Vec<String>,
    /// Workspace members, each a project of its own.
    pub members: Vec<BuildProject>,
}

impl BuildProject {
    pub fn dir(&self) -> &Path {
        self.file.parent().unwrap_or(Path::new("."))
    }
}

/// The group a task belongs to, as the IDE's "Group Tasks" shows them.
pub fn task_group(task: &str) -> &'static str {
    let name = task.rsplit(' ').next().unwrap_or(task);
    match name {
        "build" | "check" | "compile" | "assemble" | "clean" | "doc" | "package" | "install" => "build",
        "test" | "clippy" | "bench" | "verify" | "lint" => "verification",
        "run" | "start" | "dev" | "serve" => "application",
        _ if name.starts_with("test") || name.starts_with("lint") => "verification",
        _ if name.starts_with("build") => "build",
        _ => "other",
    }
}

/// The member directories a build file names: Cargo `[workspace] members`
/// and npm `workspaces`, with a trailing `/*` taken as every subdirectory.
pub fn workspace_members(file: &Path) -> Vec<PathBuf> {
    let dir = file.parent().unwrap_or(Path::new("."));
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let patterns: Vec<String> = match file.file_name().and_then(|n| n.to_str()) {
        Some("Cargo.toml") => toml::from_str::<toml::Value>(&text)
            .ok()
            .and_then(|v| v.get("workspace")?.get("members")?.as_array().cloned())
            .map(|a| a.iter().filter_map(|m| m.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        Some("package.json") => serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("workspaces")?.as_array().cloned())
            .map(|a| a.iter().filter_map(|m| m.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for pattern in patterns {
        match pattern.strip_suffix("/*") {
            Some(parent) => {
                let mut subdirs: Vec<PathBuf> = std::fs::read_dir(dir.join(parent))
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect();
                subdirs.sort();
                out.extend(subdirs);
            }
            None => out.push(dir.join(pattern)),
        }
    }
    out
}

/// The projects under `root`: its build files, each with its members.
pub fn discover(root: &Path) -> Vec<BuildProject> {
    BUILD_FILES
        .iter()
        .map(|name| root.join(name))
        .filter(|file| file.is_file())
        .map(|file| load_project(&file, 0))
        .collect()
}

fn load_project(file: &Path, depth: usize) -> BuildProject {
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let tasks = std::fs::read_to_string(file).map(|t| build_file_tasks(name, &t)).unwrap_or_default();
    // A member's own members are not followed further: build tools do not
    // nest workspaces.
    let members = if depth == 0 {
        workspace_members(file)
            .into_iter()
            .map(|dir| dir.join(name))
            .filter(|f| f.is_file())
            .map(|f| load_project(&f, 1))
            .collect()
    } else {
        Vec::new()
    };
    BuildProject { file: file.to_path_buf(), tasks, members }
}

/// When a triggered task runs (JetBrains "Execute Before / After …").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum When {
    BeforeBuild,
    AfterBuild,
    BeforeRebuild,
    AfterRebuild,
    BeforeSync,
    AfterSync,
}

impl When {
    pub const ALL: [When; 6] = [
        When::BeforeBuild,
        When::AfterBuild,
        When::BeforeRebuild,
        When::AfterRebuild,
        When::BeforeSync,
        When::AfterSync,
    ];

    pub fn label(self) -> &'static str {
        match self {
            When::BeforeBuild => "before build",
            When::AfterBuild => "after build",
            When::BeforeRebuild => "before rebuild",
            When::AfterRebuild => "after rebuild",
            When::BeforeSync => "before sync",
            When::AfterSync => "after sync",
        }
    }
}

/// A task set to run at a moment, in its project's directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub when: When,
    pub dir: PathBuf,
    pub task: String,
}

/// When build-file changes reload the projects (JetBrains "Auto-Sync").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoSync {
    /// Never: the change is announced and `build_sync` reloads.
    #[default]
    Off,
    /// On every change to a build file.
    Any,
}

/// The build-tool window's state for a project.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildState {
    pub detached: Vec<PathBuf>,
    pub ignored: Vec<PathBuf>,
    /// The members chosen for import per root build file; a root not listed
    /// imports all of them.
    pub imported: Vec<(PathBuf, Vec<PathBuf>)>,
    pub triggers: Vec<Trigger>,
    pub auto_sync: AutoSync,
    pub group_modules: bool,
    pub group_tasks: bool,
    pub show_ignored: bool,
    pub show_inherited: bool,
    /// The build files' change times at the last sync, to notice a change.
    pub synced: Vec<(PathBuf, u64)>,
}

fn state_path() -> PathBuf {
    crate::run_config::project_dir().join("build-tool.toml")
}

pub fn load() -> BuildState {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(state: &BuildState) {
    let Ok(text) = toml::to_string_pretty(state) else {
        return;
    };
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, text);
}

/// Change the saved state.
pub fn update<R>(f: impl FnOnce(&mut BuildState) -> R) -> R {
    let mut state = load();
    let r = f(&mut state);
    save(&state);
    r
}

impl BuildState {
    /// The projects a run uses: discovered, less the detached and ignored ones
    /// and the members not chosen for import.
    pub fn active_projects(&self, root: &Path) -> Vec<BuildProject> {
        let mut projects = discover(root);
        projects.retain(|p| !self.detached.contains(&p.file) && !self.ignored.contains(&p.file));
        for project in &mut projects {
            let chosen = self.imported.iter().find(|(f, _)| *f == project.file).map(|(_, m)| m.clone());
            project.members.retain(|m| {
                !self.ignored.contains(&m.file)
                    && !self.detached.contains(&m.file)
                    && chosen.as_ref().is_none_or(|c| c.contains(&m.file))
            });
        }
        projects
    }

    /// The commands triggered at `when`, each run in its project's directory.
    pub fn commands_at(&self, when: When) -> Vec<String> {
        self.triggers
            .iter()
            .filter(|t| t.when == when)
            .map(|t| format!("(cd {} && {})", shell_quote(&t.dir), t.task))
            .collect()
    }

    /// `command` with the tasks triggered before and after it, all stopping at
    /// the first failure.
    pub fn wrap(&self, before: When, command: &str, after: When) -> String {
        let mut steps = self.commands_at(before);
        steps.push(command.to_string());
        steps.extend(self.commands_at(after));
        steps.join(" && ")
    }
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

/// The modification time of `file`, in seconds.
fn mtime(file: &Path) -> u64 {
    std::fs::metadata(file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// Every build file of the projects under `root`, members included.
pub fn build_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for project in discover(root) {
        out.extend(project.members.iter().map(|m| m.file.clone()));
        out.push(project.file);
    }
    out
}

/// The build files changed since the last sync.
pub fn changed_since_sync(state: &BuildState, root: &Path) -> Vec<PathBuf> {
    build_files(root)
        .into_iter()
        .filter(|f| state.synced.iter().find(|(p, _)| p == f).is_none_or(|(_, t)| *t != mtime(f)))
        .collect()
}

/// Record the build files' times as synced.
pub fn mark_synced(state: &mut BuildState, root: &Path) {
    state.synced = build_files(root).into_iter().map(|f| {
        let t = mtime(&f);
        (f, t)
    }).collect();
}
#[cfg(test)]
mod build_task_tests {
    use super::build_file_tasks;

    #[test]
    fn npm_scripts_make_targets_and_just_recipes() {
        assert_eq!(
            vec!["npm run build", "npm run test"],
            build_file_tasks("package.json", r#"{"scripts":{"build":"tsc","test":"jest"}}"#)
        );
        let makefile = "CC := cc\nall: app\n\tcc -o app\n.PHONY: all\nclean:\n\trm app\n";
        assert_eq!(vec!["make all", "make clean"], build_file_tasks("Makefile", makefile));
        let justfile = "set shell := [\"zsh\"]\ntest arg:\n    cargo test {{arg}}\nfmt:\n    cargo fmt\n";
        assert_eq!(vec!["just test", "just fmt"], build_file_tasks("justfile", justfile));
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_groups() {
        assert_eq!("build", task_group("cargo build"));
        assert_eq!("verification", task_group("npm run test:unit"));
        assert_eq!("application", task_group("npm run dev"));
        assert_eq!("other", task_group("make fmt"));
    }

    #[test]
    fn workspace_members_and_triggers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/*\", \"tool\"]\n").unwrap();
        for member in ["crates/a", "crates/b", "tool"] {
            std::fs::create_dir_all(root.join(member)).unwrap();
            std::fs::write(root.join(member).join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        }
        let projects = discover(root);
        assert_eq!(1, projects.len());
        let members: Vec<_> = projects[0].members.iter().map(|m| m.dir().strip_prefix(root).unwrap().to_path_buf()).collect();
        assert_eq!(vec![PathBuf::from("crates/a"), PathBuf::from("crates/b"), PathBuf::from("tool")], members);

        let mut state = BuildState::default();
        state.ignored.push(root.join("tool/Cargo.toml"));
        assert_eq!(2, state.active_projects(root)[0].members.len(), "an ignored member is left out");
        state.triggers.push(Trigger { when: When::BeforeBuild, dir: root.join("tool"), task: "cargo fmt".into() });
        let wrapped = state.wrap(When::BeforeBuild, "cargo build", When::AfterBuild);
        assert!(wrapped.ends_with("&& cargo fmt) && cargo build"), "{wrapped}");
    }
}
