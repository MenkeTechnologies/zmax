//! Run targets (JetBrains "Run Targets"): where a run configuration's
//! command executes — here, on a host over SSH, in a fresh Docker container
//! of an image, or in a running container.
//!
//! The targets are kept for every project in `run-targets.toml` in the config
//! directory; a run configuration names the one it uses (`RunConfig::target`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TargetKind {
    /// `ssh <host>`, running in `dir` there, or in the same path as here.
    Ssh { host: String, dir: Option<String> },
    /// `docker run` of `image`, the workspace mounted at the same path.
    Docker { image: String },
    /// `docker exec` in the running container `name`.
    Container { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunTarget {
    pub name: String,
    #[serde(flatten)]
    pub kind: TargetKind,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Targets {
    #[serde(rename = "target")]
    targets: Vec<RunTarget>,
}

fn store() -> PathBuf {
    zmax_loader::config_dir().join("run-targets.toml")
}

pub fn load() -> Vec<RunTarget> {
    std::fs::read_to_string(store())
        .ok()
        .and_then(|t| toml::from_str::<Targets>(&t).ok())
        .map(|t| t.targets)
        .unwrap_or_default()
}

pub fn save(targets: Vec<RunTarget>) {
    let Ok(text) = toml::to_string_pretty(&Targets { targets }) else {
        return;
    };
    let path = store();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, text);
}

pub fn find(name: &str) -> Option<RunTarget> {
    load().into_iter().find(|t| t.name == name)
}

/// Parse a target as the manager's prompt takes it: `NAME ssh HOST[:DIR]`,
/// `NAME docker IMAGE` or `NAME container NAME`.
pub fn parse(spec: &str) -> Result<RunTarget, String> {
    let words: Vec<&str> = spec.split_whitespace().collect();
    let [name, kind, value] = words[..] else {
        return Err("give NAME ssh HOST[:DIR], NAME docker IMAGE or NAME container NAME".into());
    };
    let kind = match kind {
        "ssh" => match value.split_once(':') {
            Some((host, dir)) => TargetKind::Ssh { host: host.into(), dir: Some(dir.into()) },
            None => TargetKind::Ssh { host: value.into(), dir: None },
        },
        "docker" => TargetKind::Docker { image: value.into() },
        "container" => TargetKind::Container { name: value.into() },
        other => return Err(format!("unknown target kind {other}: ssh, docker or container")),
    };
    Ok(RunTarget { name: name.into(), kind })
}

impl RunTarget {
    pub fn describe(&self) -> String {
        match &self.kind {
            TargetKind::Ssh { host, dir: Some(dir) } => format!("ssh {host}:{dir}"),
            TargetKind::Ssh { host, dir: None } => format!("ssh {host}"),
            TargetKind::Docker { image } => format!("docker {image}"),
            TargetKind::Container { name } => format!("container {name}"),
        }
    }

    /// `command`, run in `dir`, as a local command line that runs it on this
    /// target.
    pub fn wrap(&self, root: &Path, dir: &Path, command: &str) -> String {
        let inner = |dir: &str| format!("cd {} && {command}", quote(dir));
        let here = dir.to_string_lossy();
        match &self.kind {
            TargetKind::Ssh { host, dir: remote } => {
                // A remote directory replaces the workspace root in `dir`.
                let there = match remote {
                    Some(remote) => match dir.strip_prefix(root) {
                        Ok(rel) if rel.as_os_str().is_empty() => remote.clone(),
                        Ok(rel) => format!("{remote}/{}", rel.display()),
                        Err(_) => remote.clone(),
                    },
                    None => here.into_owned(),
                };
                format!("ssh -t {host} {}", quote(&inner(&there)))
            }
            TargetKind::Docker { image } => format!(
                "docker run --rm -i -v {root}:{root} -w {dir} {image} sh -c {cmd}",
                root = quote(&root.to_string_lossy()),
                dir = quote(&here),
                cmd = quote(command),
            ),
            TargetKind::Container { name } => format!(
                "docker exec -i -w {dir} {name} sh -c {cmd}",
                dir = quote(&here),
                cmd = quote(command),
            ),
        }
    }
}

/// Single-quote `s` for the shell.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_wrap() {
        let root = Path::new("/w");
        let ssh = parse("box ssh dev@host:/srv/app").unwrap();
        assert_eq!("ssh -t dev@host 'cd '\\''/srv/app/sub'\\'' && make'", ssh.wrap(root, Path::new("/w/sub"), "make"));
        let docker = parse("ci docker rust:1").unwrap();
        assert_eq!(
            "docker run --rm -i -v '/w':'/w' -w '/w' rust:1 sh -c 'cargo test'",
            docker.wrap(root, root, "cargo test")
        );
        assert!(parse("x ftp y").is_err());
        assert!(parse("lonely").is_err());
    }
}
