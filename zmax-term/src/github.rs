//! GitHub transport and models — the data layer behind the `:github` browser
//! ([`crate::ui::github`]) and the CI status panel ([`crate::ci`]).
//!
//! Every call here is **blocking**: run it on `tokio::task::spawn_blocking`,
//! never on the UI thread.
//!
//! Transport is `gh api` first, because the CLI already holds the user's
//! credentials (including SSO-authorised tokens) and refreshes them; a direct
//! `ureq` call is the fallback for machines without `gh`, authenticated with
//! `GITHUB_TOKEN`/`GH_TOKEN`. Both honour `GH_HOST`, so a non-default instance
//! is reached by setting the same variable `gh` itself reads. Both
//! paths return the same `serde_json::Value`, so callers never branch on which
//! one served the request.
//!
//! The models are plain structs built by pure `from_json` parsers, so the
//! response shapes are unit-testable without a network.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde_json::Value;

// ── transport ────────────────────────────────────────────────────────────────

/// Is the `gh` CLI on `PATH`? Probed once per process.
pub fn gh_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("gh")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

/// The API token for the `ureq` fallback: the usual environment variables
/// first, then whatever `gh auth token` prints. `None` means unauthenticated
/// (public data only, 60 requests/hour).
fn token() -> Option<String> {
    static TOKEN: OnceLock<Option<String>> = OnceLock::new();
    TOKEN
        .get_or_init(|| {
            if let Some(t) = std::env::var("GITHUB_TOKEN")
                .ok()
                .or_else(|| std::env::var("GH_TOKEN").ok())
                .filter(|t| !t.is_empty())
            {
                return Some(t);
            }
            let out = Command::new("gh").args(["auth", "token"]).output().ok()?;
            let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (out.status.success() && !t.is_empty()).then_some(t)
        })
        .clone()
}

/// Trimmed stderr from a failed `gh` invocation, or a generic message.
fn gh_error(out: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if stderr.is_empty() {
        "gh: request failed".to_string()
    } else {
        stderr
    }
}

/// `GET <path>` against the API, decoded as JSON.
///
/// `path` is API-relative and may carry a query string, e.g.
/// `repos/o/r/actions/runs?per_page=50`.
pub fn api(path: &str) -> Result<Value, String> {
    request("GET", path, None)
}

/// `<method> <path>` with an optional JSON body — the mutating half of the API
/// (rerun, cancel, dispatch, comment, merge, mark-read).
///
/// Endpoints that answer `204 No Content` yield [`Value::Null`].
pub fn request(method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
    let text = request_text(method, path, body, false)?;
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| format!("parse: {e}"))
}

/// `GET <path>` returning the raw response body — Actions logs are plain text,
/// not JSON, and carry ANSI escapes and a UTF-8 BOM.
pub fn api_text(path: &str) -> Result<String, String> {
    let text = request_text("GET", path, None, true)?;
    // Actions log downloads begin with a UTF-8 BOM.
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_string())
}

/// Shared transport. `raw` marks a response that may contain terminal escape
/// sequences, which `gh` ≥ 2.100 refuses to print without an opt-in flag.
fn request_text(
    method: &str,
    path: &str,
    body: Option<&Value>,
    raw: bool,
) -> Result<String, String> {
    if gh_available() {
        return gh_request(method, path, body, raw);
    }
    http_request(method, path, body)
}

/// Run the request through `gh api`.
fn gh_request(method: &str, path: &str, body: Option<&Value>, raw: bool) -> Result<String, String> {
    let run = |escapes: bool| -> Result<std::process::Output, String> {
        let mut cmd = Command::new("gh");
        cmd.arg("api")
            .args(["-H", "Accept: application/vnd.github+json"])
            .args(["-H", "X-GitHub-Api-Version: 2022-11-28"]);
        if escapes {
            cmd.arg("--allow-escape-sequences");
        }
        if method != "GET" {
            cmd.args(["-X", method]);
        }
        if body.is_some() {
            cmd.args(["--input", "-"]);
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }
        cmd.arg(path).stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("gh: {e}"))?;
        if let (Some(b), Some(mut stdin)) = (body, child.stdin.take()) {
            let json = b.to_string();
            stdin
                .write_all(json.as_bytes())
                .map_err(|e| format!("gh: {e}"))?;
        }
        child.wait_with_output().map_err(|e| format!("gh: {e}"))
    };

    let mut out = run(raw)?;
    // `--allow-escape-sequences` landed in gh 2.100; older builds reject it.
    if raw && !out.status.success() && String::from_utf8_lossy(&out.stderr).contains("unknown flag")
    {
        out = run(false)?;
    }
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(gh_error(&out))
    }
}

/// The REST base for the fallback transport. `GH_HOST` is the variable `gh`
/// itself reads, so both transports target the same instance.
fn api_base() -> String {
    match std::env::var("GH_HOST")
        .ok()
        .filter(|h| !h.is_empty() && h != "github.com")
    {
        Some(host) => format!("https://{host}/api/v3"),
        None => "https://api.github.com".to_string(),
    }
}

/// Fallback transport: talk to the REST API directly.
fn http_request(method: &str, path: &str, body: Option<&Value>) -> Result<String, String> {
    let url = format!("{}/{}", api_base(), path.trim_start_matches('/'));
    let mut req = ureq::request(method, &url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "zmax-github")
        .set("X-GitHub-Api-Version", "2022-11-28");
    if let Some(tok) = token() {
        req = req.set("Authorization", &format!("Bearer {tok}"));
    }
    let resp = match body {
        Some(b) => req.send_string(&b.to_string()),
        None => req.call(),
    };
    match resp {
        Ok(r) => r.into_string().map_err(|e| e.to_string()),
        // A 4xx/5xx carries the API's own error message; surface that rather
        // than ureq's "status code 422".
        Err(ureq::Error::Status(code, r)) => {
            let text = r.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
                .unwrap_or_else(|| text.chars().take(200).collect());
            Err(format!("HTTP {code}: {msg}"))
        }
        Err(e) => Err(e.to_string()),
    }
}

// ── repository identity ──────────────────────────────────────────────────────

/// `owner/repo` for the repository containing `dir`, from its `origin` remote.
pub fn repo_slug(dir: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err("no git origin remote".into());
    }
    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    parse_slug(&url).ok_or_else(|| format!("can't parse owner/repo from {url}"))
}

/// The branch checked out in `dir`, or `None` on a detached HEAD.
pub fn current_branch(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !name.is_empty() && name != "HEAD").then_some(name)
}

/// Parse `owner/repo` out of an https, ssh or `git@` GitHub remote URL.
///
/// Any host is accepted as long as the path has the usual `owner/repo` shape;
/// only the two-segment tail is kept, and `GH_HOST` decides which instance the
/// slug is then queried against.
pub fn parse_slug(url: &str) -> Option<String> {
    let s = url.trim().trim_end_matches('/').trim_end_matches(".git");
    // `git@host:owner/repo` — scp-like syntax has no scheme.
    let rest = if let Some((_, tail)) = s.split_once("://") {
        // Drop `user@host/`.
        tail.split_once('/').map(|(_, r)| r)?
    } else if let Some((_, tail)) = s.split_once(':') {
        tail
    } else {
        s
    };
    let mut parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    let repo = parts.pop()?;
    let owner = parts.pop()?;
    (!owner.is_empty() && !repo.is_empty()).then(|| format!("{owner}/{repo}"))
}

// ── JSON helpers ─────────────────────────────────────────────────────────────

/// String field, `""` when absent or null.
fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// String field, `None` when absent, null or empty.
fn os(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(String::from)
}

/// Unsigned field, `0` when absent or null.
fn u(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// Signed field, `0` when absent or null.
fn i(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

/// Boolean field, `false` when absent or null.
fn b(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// `login` of a nested user object (`actor`, `user`, `owner`, …).
fn login(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|o| o.get("login"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Elements of an array field, or an empty slice.
fn arr<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map_or(&[], |a| a)
}

/// Map an array field through a parser.
fn list<T>(v: &Value, key: &str, f: impl Fn(&Value) -> T) -> Vec<T> {
    arr(v, key).iter().map(f).collect()
}

/// The top-level array of a response that is either a bare array or an object
/// wrapping one under `key` (the Actions endpoints do the latter).
fn envelope<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    match v.as_array() {
        Some(a) => a,
        None => arr(v, key),
    }
}

// ── status vocabulary ────────────────────────────────────────────────────────

/// A run/job/check outcome reduced to what the browser draws: a glyph and the
/// theme scope that colours it.
///
/// `status` is GitHub's lifecycle (`queued`/`in_progress`/`completed`);
/// `conclusion` is only set once the lifecycle is `completed`.
pub fn status_icon(status: &str, conclusion: Option<&str>) -> (&'static str, &'static str) {
    match status {
        "queued" | "waiting" | "pending" | "requested" => ("◌", "comment"),
        "in_progress" => ("●", "warning"),
        _ => match conclusion {
            Some("success") => ("✓", "diff.plus"),
            Some("failure") | Some("timed_out") | Some("startup_failure") => ("✗", "error"),
            Some("cancelled") => ("⊘", "comment"),
            Some("skipped") | Some("neutral") => ("○", "comment"),
            Some("action_required") => ("!", "warning"),
            Some(_) => ("·", "comment"),
            None => ("·", "comment"),
        },
    }
}

/// The word shown next to the glyph: the conclusion once there is one, else the
/// lifecycle status.
pub fn status_word(status: &str, conclusion: Option<&str>) -> String {
    match conclusion {
        Some(c) if status == "completed" => c.to_string(),
        _ => status.to_string(),
    }
}

// ── time ─────────────────────────────────────────────────────────────────────

/// `YYYY-MM-DDTHH:MM:SSZ` → unix seconds (UTC, proleptic Gregorian).
pub fn parse_epoch(rfc3339: &str) -> Option<i64> {
    let b = rfc3339.as_bytes();
    if b.len() < 20 {
        return None;
    }
    let num = |a: usize, z: usize| rfc3339.get(a..z)?.parse::<i64>().ok();
    let (y, mo, da) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    // Days since the epoch, via Howard Hinnant's civil-calendar algorithm.
    let y = if mo <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + se)
}

/// "2m ago" / "3h ago" for an RFC3339 timestamp; `""` when it can't be parsed.
pub fn age_of(rfc3339: &str) -> String {
    let Some(epoch) = parse_epoch(rfc3339) else {
        return String::new();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let d = now - epoch;
    if d < 0 {
        "now".into()
    } else if d < 60 {
        format!("{d}s ago")
    } else if d < 3600 {
        format!("{}m ago", d / 60)
    } else if d < 86400 {
        format!("{}h ago", d / 3600)
    } else {
        format!("{}d ago", d / 86400)
    }
}

/// `1h 04m` / `3m 12s` / `41s` for a span in seconds.
pub fn fmt_duration(secs: i64) -> String {
    if secs < 0 {
        return String::new();
    }
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// Elapsed time between two RFC3339 stamps. An absent `end` (a step or job
/// still running) is measured against now, so a live run's timer advances.
pub fn elapsed(start: &str, end: Option<&str>) -> String {
    let Some(from) = parse_epoch(start) else {
        return String::new();
    };
    let to = match end.and_then(parse_epoch) {
        Some(t) => t,
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(from),
    };
    fmt_duration(to - from)
}

/// Bytes as `1.4 MB`, for release assets and Actions artifacts.
pub fn fmt_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

// ── models ───────────────────────────────────────────────────────────────────

/// `GET /repos/{slug}` — the overview tab.
#[derive(Clone, Debug, Default)]
pub struct Repo {
    pub full_name: String,
    pub description: String,
    pub homepage: String,
    pub html_url: String,
    pub default_branch: String,
    pub language: String,
    pub license: String,
    pub visibility: String,
    pub topics: Vec<String>,
    pub stars: u64,
    pub forks: u64,
    pub watchers: u64,
    pub open_issues: u64,
    pub size_kb: u64,
    pub archived: bool,
    pub pushed_at: String,
    pub created_at: String,
}

impl Repo {
    pub fn from_json(v: &Value) -> Self {
        Repo {
            full_name: s(v, "full_name"),
            description: s(v, "description"),
            homepage: s(v, "homepage"),
            html_url: s(v, "html_url"),
            default_branch: s(v, "default_branch"),
            language: s(v, "language"),
            license: v
                .get("license")
                .and_then(|l| l.get("spdx_id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            visibility: s(v, "visibility"),
            topics: arr(v, "topics")
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect(),
            stars: u(v, "stargazers_count"),
            forks: u(v, "forks_count"),
            watchers: u(v, "subscribers_count"),
            open_issues: u(v, "open_issues_count"),
            size_kb: u(v, "size"),
            archived: b(v, "archived"),
            pushed_at: s(v, "pushed_at"),
            created_at: s(v, "created_at"),
        }
    }
}

/// One workflow run — a CI pipeline execution.
#[derive(Clone, Debug)]
pub struct Run {
    pub id: u64,
    pub workflow_id: u64,
    pub workflow: String,
    pub title: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub branch: String,
    pub sha: String,
    pub event: String,
    pub actor: String,
    pub number: u64,
    pub attempt: u64,
    pub path: String,
    pub created_at: String,
    pub started_at: String,
    pub updated_at: String,
    pub html_url: String,
}

impl Run {
    pub fn from_json(v: &Value) -> Self {
        let created = s(v, "created_at");
        Run {
            id: u(v, "id"),
            workflow_id: u(v, "workflow_id"),
            workflow: s(v, "name"),
            title: s(v, "display_title"),
            status: s(v, "status"),
            conclusion: os(v, "conclusion"),
            branch: s(v, "head_branch"),
            sha: s(v, "head_sha"),
            event: s(v, "event"),
            actor: login(v, "actor"),
            number: u(v, "run_number"),
            attempt: u(v, "run_attempt"),
            path: s(v, "path"),
            started_at: os(v, "run_started_at").unwrap_or_else(|| created.clone()),
            created_at: created,
            updated_at: s(v, "updated_at"),
            html_url: s(v, "html_url"),
        }
    }

    /// All runs out of a `{"workflow_runs": [...]}` response.
    pub fn parse_list(v: &Value) -> Vec<Run> {
        envelope(v, "workflow_runs")
            .iter()
            .map(Run::from_json)
            .collect()
    }

    pub fn icon(&self) -> (&'static str, &'static str) {
        status_icon(&self.status, self.conclusion.as_deref())
    }

    pub fn short_sha(&self) -> String {
        self.sha.chars().take(7).collect()
    }

    /// "3m ago" for when the run was created.
    pub fn age(&self) -> String {
        age_of(&self.created_at)
    }

    /// Wall-clock duration; still ticking while the run is in flight.
    pub fn duration(&self) -> String {
        let end = (self.status == "completed").then_some(self.updated_at.as_str());
        elapsed(&self.started_at, end)
    }

    /// Is this run still queued or executing?
    pub fn active(&self) -> bool {
        self.status != "completed"
    }
}

/// One job inside a run — a single runner executing a list of steps.
#[derive(Clone, Debug)]
pub struct Job {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub runner: String,
    pub labels: Vec<String>,
    pub html_url: String,
    pub steps: Vec<Step>,
}

impl Job {
    pub fn from_json(v: &Value) -> Self {
        Job {
            id: u(v, "id"),
            name: s(v, "name"),
            status: s(v, "status"),
            conclusion: os(v, "conclusion"),
            started_at: s(v, "started_at"),
            completed_at: os(v, "completed_at"),
            runner: os(v, "runner_name").unwrap_or_default(),
            labels: arr(v, "labels")
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect(),
            html_url: s(v, "html_url"),
            steps: list(v, "steps", Step::from_json),
        }
    }

    /// All jobs out of a `{"jobs": [...]}` response.
    pub fn parse_list(v: &Value) -> Vec<Job> {
        envelope(v, "jobs").iter().map(Job::from_json).collect()
    }

    pub fn icon(&self) -> (&'static str, &'static str) {
        status_icon(&self.status, self.conclusion.as_deref())
    }

    pub fn duration(&self) -> String {
        elapsed(&self.started_at, self.completed_at.as_deref())
    }

    pub fn failed(&self) -> bool {
        matches!(
            self.conclusion.as_deref(),
            Some("failure") | Some("timed_out") | Some("startup_failure")
        )
    }
}

/// One step of a job.
#[derive(Clone, Debug)]
pub struct Step {
    pub number: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

impl Step {
    pub fn from_json(v: &Value) -> Self {
        Step {
            number: u(v, "number"),
            name: s(v, "name"),
            status: s(v, "status"),
            conclusion: os(v, "conclusion"),
            started_at: s(v, "started_at"),
            completed_at: os(v, "completed_at"),
        }
    }

    pub fn icon(&self) -> (&'static str, &'static str) {
        status_icon(&self.status, self.conclusion.as_deref())
    }

    pub fn duration(&self) -> String {
        elapsed(&self.started_at, self.completed_at.as_deref())
    }
}

/// A workflow definition (`.github/workflows/*.yml`).
#[derive(Clone, Debug)]
pub struct Workflow {
    pub id: u64,
    pub name: String,
    pub path: String,
    pub state: String,
    pub html_url: String,
}

impl Workflow {
    pub fn from_json(v: &Value) -> Self {
        Workflow {
            id: u(v, "id"),
            name: s(v, "name"),
            path: s(v, "path"),
            state: s(v, "state"),
            html_url: s(v, "html_url"),
        }
    }

    pub fn parse_list(v: &Value) -> Vec<Workflow> {
        envelope(v, "workflows")
            .iter()
            .map(Workflow::from_json)
            .collect()
    }

    /// Workflows GitHub synthesises (Dependabot, Pages) live under `dynamic/`
    /// and have no file to open or dispatch.
    pub fn is_file(&self) -> bool {
        self.path.starts_with(".github/")
    }
}

/// An artifact uploaded by a run.
#[derive(Clone, Debug)]
pub struct Artifact {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub expired: bool,
    pub created_at: String,
}

impl Artifact {
    pub fn from_json(v: &Value) -> Self {
        Artifact {
            id: u(v, "id"),
            name: s(v, "name"),
            size: u(v, "size_in_bytes"),
            expired: b(v, "expired"),
            created_at: s(v, "created_at"),
        }
    }

    pub fn parse_list(v: &Value) -> Vec<Artifact> {
        envelope(v, "artifacts")
            .iter()
            .map(Artifact::from_json)
            .collect()
    }
}

/// An issue or a pull request. The two endpoints share almost every field, so
/// one row type serves both lists.
#[derive(Clone, Debug)]
pub struct Topic {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub author: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub comments: u64,
    pub created_at: String,
    pub updated_at: String,
    pub html_url: String,
    pub is_pr: bool,
    pub draft: bool,
    pub merged: bool,
    pub head: String,
    pub base: String,
}

impl Topic {
    /// Parse an entry from `/pulls` (`pr = true`) or `/issues` (`pr = false`).
    pub fn from_json(v: &Value, pr: bool) -> Self {
        // `/issues` returns pull requests too, tagged by a `pull_request` key.
        let is_pr = pr || v.get("pull_request").is_some();
        Topic {
            number: u(v, "number"),
            title: s(v, "title"),
            state: s(v, "state"),
            author: login(v, "user"),
            labels: arr(v, "labels")
                .iter()
                .map(|l| l.get("name").and_then(Value::as_str).unwrap_or("").into())
                .collect(),
            assignees: arr(v, "assignees")
                .iter()
                .map(|a| a.get("login").and_then(Value::as_str).unwrap_or("").into())
                .collect(),
            comments: u(v, "comments"),
            created_at: s(v, "created_at"),
            updated_at: s(v, "updated_at"),
            html_url: s(v, "html_url"),
            is_pr,
            draft: b(v, "draft"),
            merged: v.get("merged_at").is_some_and(|m| !m.is_null()),
            head: v.get("head").map(|h| s(h, "ref")).unwrap_or_default(),
            base: v.get("base").map(|h| s(h, "ref")).unwrap_or_default(),
        }
    }

    pub fn parse_list(v: &Value, pr: bool) -> Vec<Topic> {
        v.as_array()
            .map(|a| a.iter().map(|e| Topic::from_json(e, pr)).collect())
            .unwrap_or_default()
    }

    /// `#12` prefixed by the state glyph.
    pub fn icon(&self) -> (&'static str, &'static str) {
        if self.merged {
            ("◈", "constant")
        } else if self.state == "closed" {
            ("✗", "error")
        } else if self.draft {
            ("◌", "comment")
        } else {
            ("●", "diff.plus")
        }
    }
}

/// A comment on an issue or pull request.
#[derive(Clone, Debug)]
pub struct Comment {
    pub author: String,
    pub created_at: String,
    pub body: String,
}

impl Comment {
    pub fn from_json(v: &Value) -> Self {
        Comment {
            author: login(v, "user"),
            created_at: s(v, "created_at"),
            body: s(v, "body"),
        }
    }

    pub fn parse_list(v: &Value) -> Vec<Comment> {
        v.as_array()
            .map(|a| a.iter().map(Comment::from_json).collect())
            .unwrap_or_default()
    }
}

/// A review on a pull request.
#[derive(Clone, Debug)]
pub struct Review {
    pub author: String,
    pub state: String,
    pub body: String,
    pub submitted_at: String,
}

impl Review {
    pub fn from_json(v: &Value) -> Self {
        Review {
            author: login(v, "user"),
            state: s(v, "state"),
            body: s(v, "body"),
            submitted_at: s(v, "submitted_at"),
        }
    }

    pub fn parse_list(v: &Value) -> Vec<Review> {
        v.as_array()
            .map(|a| a.iter().map(Review::from_json).collect())
            .unwrap_or_default()
    }
}

/// One file in a pull request or commit diff.
#[derive(Clone, Debug)]
pub struct FileChange {
    pub filename: String,
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    pub patch: Option<String>,
}

impl FileChange {
    pub fn from_json(v: &Value) -> Self {
        FileChange {
            filename: s(v, "filename"),
            status: s(v, "status"),
            additions: u(v, "additions"),
            deletions: u(v, "deletions"),
            patch: os(v, "patch"),
        }
    }

    pub fn parse_list(v: &Value) -> Vec<FileChange> {
        v.as_array()
            .map(|a| a.iter().map(FileChange::from_json).collect())
            .unwrap_or_default()
    }
}

/// A check run attached to a commit (the per-commit view of CI, including
/// checks from apps that are not GitHub Actions).
#[derive(Clone, Debug)]
pub struct Check {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub details_url: String,
    pub started_at: String,
    pub completed_at: Option<String>,
}

impl Check {
    pub fn from_json(v: &Value) -> Self {
        Check {
            name: s(v, "name"),
            status: s(v, "status"),
            conclusion: os(v, "conclusion"),
            details_url: s(v, "details_url"),
            started_at: s(v, "started_at"),
            completed_at: os(v, "completed_at"),
        }
    }

    pub fn parse_list(v: &Value) -> Vec<Check> {
        envelope(v, "check_runs")
            .iter()
            .map(Check::from_json)
            .collect()
    }

    pub fn icon(&self) -> (&'static str, &'static str) {
        status_icon(&self.status, self.conclusion.as_deref())
    }
}

/// A published (or draft) release.
#[derive(Clone, Debug)]
pub struct Release {
    pub id: u64,
    pub tag: String,
    pub name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub published_at: String,
    pub author: String,
    pub body: String,
    pub html_url: String,
    pub assets: Vec<ReleaseAsset>,
}

/// A binary attached to a release.
#[derive(Clone, Debug)]
pub struct ReleaseAsset {
    pub name: String,
    pub size: u64,
    pub downloads: u64,
}

impl Release {
    pub fn from_json(v: &Value) -> Self {
        Release {
            id: u(v, "id"),
            tag: s(v, "tag_name"),
            name: s(v, "name"),
            draft: b(v, "draft"),
            prerelease: b(v, "prerelease"),
            published_at: s(v, "published_at"),
            author: login(v, "author"),
            body: s(v, "body"),
            html_url: s(v, "html_url"),
            assets: list(v, "assets", |a| ReleaseAsset {
                name: s(a, "name"),
                size: u(a, "size"),
                downloads: u(a, "download_count"),
            }),
        }
    }

    pub fn parse_list(v: &Value) -> Vec<Release> {
        v.as_array()
            .map(|a| a.iter().map(Release::from_json).collect())
            .unwrap_or_default()
    }

    /// Total downloads across every asset.
    pub fn downloads(&self) -> u64 {
        self.assets.iter().map(|a| a.downloads).sum()
    }
}

/// A branch, with its protection flag and tip.
#[derive(Clone, Debug)]
pub struct Branch {
    pub name: String,
    pub sha: String,
    pub protected: bool,
}

impl Branch {
    pub fn from_json(v: &Value) -> Self {
        Branch {
            name: s(v, "name"),
            sha: v.get("commit").map(|c| s(c, "sha")).unwrap_or_default(),
            protected: b(v, "protected"),
        }
    }

    pub fn parse_list(v: &Value) -> Vec<Branch> {
        v.as_array()
            .map(|a| a.iter().map(Branch::from_json).collect())
            .unwrap_or_default()
    }
}

/// A commit row from `/commits`.
#[derive(Clone, Debug)]
pub struct CommitRow {
    pub sha: String,
    pub summary: String,
    pub author: String,
    pub date: String,
    pub html_url: String,
}

impl CommitRow {
    pub fn from_json(v: &Value) -> Self {
        let commit = v.get("commit").cloned().unwrap_or(Value::Null);
        let message = s(&commit, "message");
        let author = commit
            .get("author")
            .map(|a| s(a, "name"))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| login(v, "author"));
        let date = commit
            .get("author")
            .map(|a| s(a, "date"))
            .unwrap_or_default();
        CommitRow {
            sha: s(v, "sha"),
            summary: message.lines().next().unwrap_or("").to_string(),
            author,
            date,
            html_url: s(v, "html_url"),
        }
    }

    pub fn parse_list(v: &Value) -> Vec<CommitRow> {
        v.as_array()
            .map(|a| a.iter().map(CommitRow::from_json).collect())
            .unwrap_or_default()
    }

    pub fn short_sha(&self) -> String {
        self.sha.chars().take(7).collect()
    }
}

/// The full body of one commit, with its diff.
#[derive(Clone, Debug)]
pub struct CommitDetail {
    pub sha: String,
    pub message: String,
    pub author: String,
    pub date: String,
    pub additions: u64,
    pub deletions: u64,
    pub files: Vec<FileChange>,
}

impl CommitDetail {
    pub fn from_json(v: &Value) -> Self {
        let commit = v.get("commit").cloned().unwrap_or(Value::Null);
        let stats = v.get("stats").cloned().unwrap_or(Value::Null);
        CommitDetail {
            sha: s(v, "sha"),
            message: s(&commit, "message"),
            author: commit
                .get("author")
                .map(|a| s(a, "name"))
                .unwrap_or_default(),
            date: commit
                .get("author")
                .map(|a| s(a, "date"))
                .unwrap_or_default(),
            additions: u(&stats, "additions"),
            deletions: u(&stats, "deletions"),
            files: v
                .get("files")
                .map(FileChange::parse_list)
                .unwrap_or_default(),
        }
    }
}

/// An entry in the notification inbox.
#[derive(Clone, Debug)]
pub struct Notification {
    pub id: String,
    pub reason: String,
    pub unread: bool,
    pub title: String,
    pub kind: String,
    pub repo: String,
    pub updated_at: String,
    /// API URL of the subject; the web URL is derived from it on demand.
    pub subject_url: String,
}

impl Notification {
    pub fn from_json(v: &Value) -> Self {
        let subject = v.get("subject").cloned().unwrap_or(Value::Null);
        Notification {
            id: s(v, "id"),
            reason: s(v, "reason"),
            unread: b(v, "unread"),
            title: s(&subject, "title"),
            kind: s(&subject, "type"),
            repo: v
                .get("repository")
                .map(|r| s(r, "full_name"))
                .unwrap_or_default(),
            updated_at: s(v, "updated_at"),
            subject_url: s(&subject, "url"),
        }
    }

    pub fn parse_list(v: &Value) -> Vec<Notification> {
        v.as_array()
            .map(|a| a.iter().map(Notification::from_json).collect())
            .unwrap_or_default()
    }

    /// `https://github.com/o/r/pull/12` from the subject's API URL, which is
    /// the only link the notifications endpoint gives.
    pub fn web_url(&self) -> String {
        let Some(tail) = self.subject_url.split("/repos/").nth(1) else {
            return format!("https://github.com/{}", self.repo);
        };
        let path = tail
            .replace("/pulls/", "/pull/")
            .replace("/commits/", "/commit/");
        format!("https://github.com/{path}")
    }
}

/// The API rate-limit budget, shown in the browser's footer so a burst of
/// polling is visible rather than mysterious.
#[derive(Clone, Copy, Debug, Default)]
pub struct RateLimit {
    pub remaining: i64,
    pub limit: i64,
    pub reset: i64,
}

impl RateLimit {
    pub fn from_json(v: &Value) -> Self {
        let core = v
            .get("resources")
            .and_then(|r| r.get("core"))
            .cloned()
            .unwrap_or(Value::Null);
        RateLimit {
            remaining: i(&core, "remaining"),
            limit: i(&core, "limit"),
            reset: i(&core, "reset"),
        }
    }
}

// ── Actions logs ─────────────────────────────────────────────────────────────

/// What a log line is, which decides how it is coloured and whether it folds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogKind {
    Plain,
    Group,
    Command,
    Error,
    Warning,
    Notice,
    Debug,
}

impl LogKind {
    /// Theme scope for the line.
    pub fn scope(self) -> &'static str {
        match self {
            LogKind::Group => "ui.text.focus",
            LogKind::Command => "constant",
            LogKind::Error => "error",
            LogKind::Warning => "warning",
            LogKind::Notice => "diff.plus",
            LogKind::Debug => "comment",
            LogKind::Plain => "ui.text",
        }
    }
}

/// One parsed line of an Actions log.
#[derive(Clone, Debug)]
pub struct LogLine {
    /// The runner's `HH:MM:SS`, stripped off the front of the raw line.
    pub time: String,
    pub text: String,
    pub kind: LogKind,
    /// Index of the enclosing `##[group]` header line, when inside one.
    pub group: Option<usize>,
}

/// Drop ANSI SGR/CSI escape sequences, which runners emit freely and the
/// surface cannot render.
pub fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // CSI: `ESC [ … <final byte in @..~>`; anything else: skip one char.
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                // OSC runs to BEL or ST.
                for c in chars.by_ref() {
                    if c == '\u{7}' || c == '\u{1b}' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Parse a downloaded job log into displayable lines.
///
/// Each raw line is `<RFC3339 timestamp> <text>`; the workflow-command markers
/// (`##[group]`, `##[error]`, …) classify it, and every line between a
/// `##[group]` and its `##[endgroup]` records the header's index so the viewer
/// can fold it.
pub fn parse_log(raw: &str) -> Vec<LogLine> {
    let mut out: Vec<LogLine> = Vec::new();
    let mut open_group: Option<usize> = None;
    for raw_line in raw.lines() {
        let line = strip_ansi(raw_line.trim_end_matches('\r'));
        // Split the leading timestamp off; keep only the clock part.
        let (time, rest) = match line.split_once(' ') {
            Some((stamp, rest)) if parse_epoch(stamp).is_some() => (
                stamp.get(11..19).unwrap_or("").to_string(),
                rest.to_string(),
            ),
            _ => (String::new(), line.clone()),
        };

        let marker = rest.trim_start();
        let (kind, text) = if let Some(t) = marker.strip_prefix("##[group]") {
            (LogKind::Group, t.to_string())
        } else if marker.starts_with("##[endgroup]") {
            open_group = None;
            continue;
        } else if let Some(t) = marker.strip_prefix("##[error]") {
            (LogKind::Error, t.to_string())
        } else if let Some(t) = marker.strip_prefix("##[warning]") {
            (LogKind::Warning, t.to_string())
        } else if let Some(t) = marker.strip_prefix("##[notice]") {
            (LogKind::Notice, t.to_string())
        } else if let Some(t) = marker.strip_prefix("##[debug]") {
            (LogKind::Debug, t.to_string())
        } else if let Some(t) = marker.strip_prefix("##[command]") {
            (LogKind::Command, t.to_string())
        } else if let Some(t) = marker.strip_prefix("[command]") {
            (LogKind::Command, t.to_string())
        } else {
            (LogKind::Plain, rest.clone())
        };

        if kind == LogKind::Group {
            out.push(LogLine {
                time,
                text,
                kind,
                group: None,
            });
            open_group = Some(out.len() - 1);
            continue;
        }
        out.push(LogLine {
            time,
            text,
            kind,
            group: open_group,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_from_every_remote_form() {
        assert_eq!(
            parse_slug("https://github.com/o/r.git").as_deref(),
            Some("o/r")
        );
        assert_eq!(parse_slug("git@github.com:o/r.git").as_deref(), Some("o/r"));
        assert_eq!(
            parse_slug("ssh://git@github.com/o/r").as_deref(),
            Some("o/r")
        );
        // Self-hosted instance, and a trailing slash.
        assert_eq!(
            parse_slug("https://git.corp.example/o/r/").as_deref(),
            Some("o/r")
        );
        assert_eq!(parse_slug("not-a-url"), None);
    }

    #[test]
    fn epoch_and_durations() {
        assert_eq!(parse_epoch("2021-01-01T00:00:00Z"), Some(1609459200));
        assert_eq!(parse_epoch("nope"), None);
        assert_eq!(fmt_duration(41), "41s");
        assert_eq!(fmt_duration(192), "3m 12s");
        assert_eq!(fmt_duration(3840), "1h 04m");
        assert_eq!(
            elapsed("2021-01-01T00:00:00Z", Some("2021-01-01T00:03:12Z")),
            "3m 12s"
        );
    }

    #[test]
    fn bytes_are_scaled() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1536), "1.5 KB");
        assert_eq!(fmt_bytes(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn run_list_parses_the_actions_envelope() {
        // Field-for-field the shape `GET /repos/{slug}/actions/runs` returns.
        let v: Value = serde_json::from_str(
            r#"{"workflow_runs":[{
                "id":33985906855,"workflow_id":301837112,"name":"Release",
                "display_title":"release: v0.4.73","status":"completed",
                "conclusion":"success","head_branch":"v0.4.73",
                "head_sha":"12e2a701f7be00dd9c7337d2f6b1b3babdbd3cb3",
                "event":"push","actor":{"login":"octocat"},"run_number":113,
                "run_attempt":1,"path":".github/workflows/release.yml",
                "created_at":"2026-09-05T19:02:45Z",
                "run_started_at":"2026-09-05T19:02:45Z",
                "updated_at":"2026-09-05T19:59:16Z",
                "html_url":"https://github.com/o/r/actions/runs/33985906855"}]}"#,
        )
        .unwrap();
        let runs = Run::parse_list(&v);
        assert_eq!(runs.len(), 1);
        let r = &runs[0];
        assert_eq!(r.workflow, "Release");
        assert_eq!(r.actor, "octocat");
        assert_eq!(r.short_sha(), "12e2a70");
        assert_eq!(r.duration(), "56m 31s");
        assert!(!r.active());
        assert_eq!(r.icon().0, "✓");
    }

    #[test]
    fn job_steps_and_running_state() {
        let v: Value = serde_json::from_str(
            r#"{"jobs":[{"id":101,"name":"Build","status":"in_progress",
                "conclusion":null,"started_at":"2026-09-05T19:02:50Z",
                "completed_at":null,"runner_name":"GitHub Actions 1",
                "labels":["ubuntu-latest"],"html_url":"https://example/job",
                "steps":[{"number":1,"name":"Set up job","status":"completed",
                    "conclusion":"success","started_at":"2026-09-05T19:02:51Z",
                    "completed_at":"2026-09-05T19:02:52Z"}]}]}"#,
        )
        .unwrap();
        let jobs = Job::parse_list(&v);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].icon().0, "●"); // running, no conclusion yet
        assert_eq!(jobs[0].steps[0].duration(), "1s");
        assert!(!jobs[0].failed());
    }

    #[test]
    fn issues_endpoint_marks_pull_requests() {
        let v: Value = serde_json::from_str(
            r#"[{"number":7,"title":"fix","state":"open","user":{"login":"o"},
                "labels":[{"name":"bug"}],"assignees":[],"comments":2,
                "created_at":"2026-09-01T00:00:00Z",
                "updated_at":"2026-09-02T00:00:00Z","html_url":"u",
                "pull_request":{"url":"x"}}]"#,
        )
        .unwrap();
        let topics = Topic::parse_list(&v, false);
        assert!(topics[0].is_pr, "an /issues row with pull_request is a PR");
        assert_eq!(topics[0].labels, vec!["bug"]);
    }

    #[test]
    fn ansi_is_stripped() {
        assert_eq!(strip_ansi("\u{1b}[0;32mok\u{1b}[0m"), "ok");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn log_groups_and_markers() {
        let raw = "\u{feff}2026-09-05T19:02:51.1304499Z ##[group]Setup\n\
                   2026-09-05T19:02:52.0000000Z inside\n\
                   2026-09-05T19:02:53.0000000Z ##[endgroup]\n\
                   2026-09-05T19:02:54.0000000Z ##[error]boom\n";
        let lines = parse_log(raw.strip_prefix('\u{feff}').unwrap());
        assert_eq!(lines.len(), 3, "endgroup is consumed, not rendered");
        assert_eq!(lines[0].kind, LogKind::Group);
        assert_eq!(lines[0].text, "Setup");
        assert_eq!(lines[0].time, "19:02:51");
        assert_eq!(lines[1].group, Some(0), "body line folds under the header");
        assert_eq!(lines[2].kind, LogKind::Error);
        assert_eq!(lines[2].group, None, "endgroup closed the group");
    }

    #[test]
    fn notification_web_url_from_api_url() {
        let v: Value = serde_json::from_str(
            r#"[{"id":"1","reason":"mention","unread":true,
                "updated_at":"2026-09-01T00:00:00Z",
                "repository":{"full_name":"o/r"},
                "subject":{"title":"t","type":"PullRequest",
                    "url":"https://api.github.com/repos/o/r/pulls/12"}}]"#,
        )
        .unwrap();
        let n = &Notification::parse_list(&v)[0];
        assert_eq!(n.web_url(), "https://github.com/o/r/pull/12");
    }
}
