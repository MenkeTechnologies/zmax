//! The GitHub browser — a full-screen overlay over the whole forge, opened with
//! `:github` (aliases `:gh`, `:hub`).
//!
//! Nine tabs cover the repository the focused buffer lives in:
//!
//! | Tab | Endpoint | Drill-down |
//! |-----|----------|------------|
//! | Repo | `/repos/{slug}` | — |
//! | Runs | `/actions/runs` | run → jobs → steps → **job log** |
//! | Workflows | `/actions/workflows` | filter Runs to that workflow |
//! | PRs | `/pulls` | body, checks, files, reviews, comments |
//! | Issues | `/issues` | body, comments |
//! | Releases | `/releases` | notes + assets |
//! | Branches | `/branches` | filter Commits to that branch |
//! | Commits | `/commits` | commit → full diff |
//! | Inbox | `/notifications` | open the subject |
//!
//! Every request is issued on a blocking task and delivered back through a job
//! callback keyed by a monotonic request id, so a slow response can never block
//! the UI thread and a stale one is dropped rather than overwriting fresher
//! data. Transport, models and parsing live in [`crate::github`].
//!
//! The CI pipelines are the centre of gravity: the Runs tab filters by branch,
//! status and workflow, `a` turns on an 8-second auto-refresh so a running
//! pipeline updates in place, and a run's jobs expand to their steps with live
//! timers. `Enter` on a job downloads its log into a viewer that folds
//! `##[group]` sections, colours `##[error]`/`##[warning]` lines and searches.
//!
//! Mutations are the ones a pipeline actually needs — rerun, rerun-failed-jobs,
//! cancel, delete, workflow dispatch, enable/disable — plus the topic actions
//! (comment, close/reopen, merge, checkout) and inbox mark-as-read. Destructive
//! ones (delete a run, merge a PR) are armed by pressing the key twice.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use tui::buffer::Buffer as Surface;
use zmax_view::graphics::{Modifier, Rect, Style};
use zmax_view::input::KeyEvent;
use zmax_view::keyboard::{KeyCode, KeyModifiers};

use crate::compositor::{Callback, Component, Compositor, Context, Event, EventResult};
use crate::github::{
    self, Artifact, Branch, Check, Comment, CommitDetail, CommitRow, FileChange, Job, LogKind,
    LogLine, Notification, RateLimit, Release, Repo, Review, Run, Topic, Workflow,
};
use crate::job::{self, Jobs};
use crate::{alt, ctrl, key};

/// Seconds between polls while auto-refresh (`a`) is on.
const AUTO_SECS: u64 = 8;
/// Rows fetched per list. GitHub caps `per_page` at 100.
const PAGE: usize = 50;

fn bold(style: Style) -> Style {
    style.add_modifier(Modifier::BOLD)
}

/// Truncate `text` to `n` columns, marking the cut with `…`.
fn trunc(text: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    if text.chars().count() <= n {
        return text.to_string();
    }
    let mut out: String = text.chars().take(n.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Truncate to `n` columns, then pad with spaces to exactly `n` — the column
/// builder every list row is assembled from.
fn pad(text: &str, n: usize) -> String {
    let mut out = trunc(text, n);
    let width = out.chars().count();
    for _ in width..n {
        out.push(' ');
    }
    out
}

/// One line of an asynchronously loaded page, with the theme scope to draw it in.
#[derive(Clone, Debug)]
pub struct TextLine {
    pub text: String,
    pub scope: &'static str,
}

impl TextLine {
    fn new(text: impl Into<String>, scope: &'static str) -> Self {
        TextLine {
            text: text.into(),
            scope,
        }
    }

    fn plain(text: impl Into<String>) -> Self {
        TextLine::new(text, "ui.text")
    }

    fn dim(text: impl Into<String>) -> Self {
        TextLine::new(text, "comment")
    }

    fn head(text: impl Into<String>) -> Self {
        TextLine::new(text, "ui.text.focus")
    }
}

/// One asynchronously fetched value: what it holds, whether a request is in
/// flight, and the error the last attempt failed with.
struct Slot<T> {
    data: Option<T>,
    error: Option<String>,
    /// Id of the in-flight request, if any. A reply whose id doesn't match is
    /// stale (the user re-filtered or refreshed) and is discarded.
    req: Option<u64>,
    /// Has a request ever completed? Distinguishes "empty" from "not asked".
    settled: bool,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Slot {
            data: None,
            error: None,
            req: None,
            settled: false,
        }
    }
}

impl<T> Slot<T> {
    fn begin(&mut self, req: u64) {
        self.req = Some(req);
    }

    fn deliver(&mut self, req: u64, out: Result<T, String>) {
        if self.req != Some(req) {
            return; // superseded by a newer request
        }
        self.req = None;
        self.settled = true;
        match out {
            Ok(data) => {
                self.data = Some(data);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn loading(&self) -> bool {
        self.req.is_some()
    }

    /// Does this slot need a fetch — never asked, and nothing in flight?
    fn idle(&self) -> bool {
        !self.settled && self.req.is_none()
    }

    /// The status line shown in place of rows while there is nothing to draw.
    fn placeholder(&self, empty: &str) -> String {
        if self.loading() {
            "loading…".to_string()
        } else if let Some(e) = &self.error {
            format!("error: {e}")
        } else {
            empty.to_string()
        }
    }
}

/// Run `work` on a blocking task and hand the result to the `C` component that
/// is still on the compositor when it lands.
///
/// `req` is the caller's request id; the delivery closure is expected to drop
/// the payload when its slot has moved on.
fn spawn<C, T>(
    jobs: &mut Jobs,
    req: u64,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
    deliver: impl FnOnce(&mut C, u64, Result<T, String>) + Send + 'static,
) where
    C: Component + 'static,
    T: Send + 'static,
{
    jobs.callback(async move {
        let out = tokio::task::spawn_blocking(work)
            .await
            .unwrap_or_else(|e| Err(format!("task failed: {e}")));
        let call: job::Callback = job::Callback::EditorCompositor(Box::new(
            move |_editor, compositor: &mut Compositor| {
                if let Some(component) = compositor.find::<C>() {
                    deliver(component, req, out);
                }
            },
        ));
        Ok(call)
    });
}

/// Sleep, then wake the component up for the next auto-refresh poll.
fn arm_timer<C: Component + 'static>(jobs: &mut Jobs, fire: impl FnOnce(&mut C) + Send + 'static) {
    jobs.callback(async move {
        tokio::time::sleep(Duration::from_secs(AUTO_SECS)).await;
        let call: job::Callback = job::Callback::EditorCompositor(Box::new(
            move |_editor, compositor: &mut Compositor| {
                if let Some(component) = compositor.find::<C>() {
                    fire(component);
                }
                zmax_event::request_redraw();
            },
        ));
        Ok(call)
    });
}

/// Push a component onto the compositor from inside an event handler.
fn push(component: impl Component + 'static) -> Callback {
    Box::new(move |compositor: &mut Compositor, _cx: &mut Context| {
        compositor.push(Box::new(component));
    })
}

/// Pop the top layer — every page's `q` / `Esc`.
fn close() -> Callback {
    Box::new(|compositor: &mut Compositor, _cx: &mut Context| {
        compositor.pop();
    })
}

/// Copy a URL to the kill ring, or explain that the row has none.
fn yank(url: &str, cx: &mut Context) {
    if url.is_empty() {
        cx.editor.set_status("no url for this row");
    } else {
        crate::emacs_kill::record(url.to_string());
        cx.editor.set_status(format!("copied {url}"));
    }
}

/// Open a URL in the system browser.
fn browse(url: &str, cx: &mut Context) {
    if url.is_empty() {
        cx.editor.set_status("no url for this row");
        return;
    }
    match crate::commands::open_in_browser(url) {
        Ok(()) => cx.editor.set_status(format!("opening {url}")),
        Err(e) => cx.editor.set_error(format!("failed to open browser: {e}")),
    }
}

/// Render a page frame: title bar, right-aligned hint, and the body rectangle
/// left over for rows.
fn frame(
    surface: &mut Surface,
    area: Rect,
    theme: &zmax_view::Theme,
    transparent: bool,
    title: &str,
    right: &str,
    hint: &str,
) -> Rect {
    let mut bg = theme.get("ui.background");
    if transparent {
        bg.bg = None;
    }
    surface.clear_with(area, bg);
    if area.width < 8 || area.height < 4 {
        return Rect::new(area.x, area.y, area.width, 0);
    }
    let head = bold(theme.get("ui.text.focus"));
    let info = theme.get("ui.linenr");

    surface.set_stringn(area.x, area.y, title, area.width as usize, head);
    let title_w = title.chars().count();
    if !right.is_empty() {
        let w = right.chars().count();
        if title_w + w + 3 < area.width as usize {
            surface.set_stringn(area.x + area.width - w as u16 - 1, area.y, right, w, info);
        }
    }
    // The key hint occupies the last row, so the body is everything between.
    let hint_y = area.y + area.height - 1;
    surface.set_stringn(
        area.x,
        hint_y,
        &trunc(hint, area.width.saturating_sub(1) as usize),
        area.width as usize,
        info,
    );
    Rect::new(
        area.x,
        area.y + 2,
        area.width,
        area.height.saturating_sub(3),
    )
}

/// Fold a shifted character key into the bare uppercase event the `key!` macro
/// produces.
///
/// A terminal speaking the enhanced keyboard protocol (kitty, ghostty, foot,
/// tmux ≥ 3.7 passing it through) reports `E` as *shift* + `E`, while a legacy
/// terminal sends the byte alone. `KeyEvent`'s `FromStr` already normalizes the
/// two forms to one — "so that characters like C-S-r and C-R are represented by
/// equal KeyEvents" — but incoming events skip that path, so every page here
/// normalizes before matching. Without it `E`, `R`, `X`, `D` and the rest of the
/// uppercase keys silently do nothing on those terminals.
fn normalize(key: KeyEvent) -> KeyEvent {
    match key.code {
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::SHIFT) => {
            let mut modifiers = key.modifiers;
            modifiers.remove(KeyModifiers::SHIFT);
            KeyEvent {
                code: KeyCode::Char(c.to_ascii_uppercase()),
                modifiers,
            }
        }
        _ => key,
    }
}

/// Width for a stretchy column: whatever is left after the fixed columns, but
/// never so wide that the metadata behind it drifts to the far edge of an
/// ultra-wide terminal, and never narrower than a readable stub.
fn flex(width: usize, fixed: usize, max: usize) -> usize {
    width.saturating_sub(fixed).min(max).max(12)
}

/// Keep `selected` inside the window that starts at `scroll` and is `height`
/// rows tall, returning the (possibly moved) scroll offset.
fn scroll_into_view(selected: usize, scroll: usize, height: usize) -> usize {
    if height == 0 {
        return 0;
    }
    if selected < scroll {
        selected
    } else if selected >= scroll + height {
        selected - height + 1
    } else {
        scroll
    }
}

/// Percent-encode a query-string value. Branch names carry `/` and can carry
/// `#`, so they cannot be interpolated raw.
fn urlq(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ── tabs ─────────────────────────────────────────────────────────────────────

/// The browser's top-level sections, in tab-bar order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Repo,
    Runs,
    Workflows,
    Pulls,
    Issues,
    Releases,
    Branches,
    Commits,
    Inbox,
}

const TABS: [Tab; 9] = [
    Tab::Repo,
    Tab::Runs,
    Tab::Workflows,
    Tab::Pulls,
    Tab::Issues,
    Tab::Releases,
    Tab::Branches,
    Tab::Commits,
    Tab::Inbox,
];

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Tab::Repo => "Repo",
            Tab::Runs => "Runs",
            Tab::Workflows => "Workflows",
            Tab::Pulls => "PRs",
            Tab::Issues => "Issues",
            Tab::Releases => "Releases",
            Tab::Branches => "Branches",
            Tab::Commits => "Commits",
            Tab::Inbox => "Inbox",
        }
    }

    fn index(self) -> usize {
        TABS.iter().position(|t| *t == self).unwrap_or(0)
    }

    /// Name accepted as the `:github <tab>` argument.
    pub fn from_name(name: &str) -> Option<Tab> {
        match name.to_ascii_lowercase().as_str() {
            "repo" | "overview" => Some(Tab::Repo),
            "runs" | "ci" | "actions" | "pipelines" => Some(Tab::Runs),
            "workflows" | "workflow" => Some(Tab::Workflows),
            "prs" | "pr" | "pulls" | "pull" => Some(Tab::Pulls),
            "issues" | "issue" => Some(Tab::Issues),
            "releases" | "release" => Some(Tab::Releases),
            "branches" | "branch" => Some(Tab::Branches),
            "commits" | "commit" | "log" => Some(Tab::Commits),
            "inbox" | "notifications" | "notifs" => Some(Tab::Inbox),
            _ => None,
        }
    }

    /// Every tab title, lower-cased, for `:github <tab>` completion.
    pub fn names() -> Vec<String> {
        TABS.iter().map(|t| t.title().to_lowercase()).collect()
    }

    /// The per-tab half of the key hint line.
    fn hint(self) -> &'static str {
        match self {
            Tab::Repo => "",
            Tab::Runs => "R rerun  F rerun-failed  X cancel  D delete  b branch  S status  w clear-workflow  a auto",
            Tab::Workflows => "Enter runs  d dispatch  e enable/disable",
            Tab::Pulls => "Enter open  C checkout  c comment  s close/reopen  M merge  S state",
            Tab::Issues => "Enter open  c comment  s close/reopen  S state",
            Tab::Releases => "Enter notes",
            Tab::Branches => "Enter commits",
            Tab::Commits => "Enter diff  b branch",
            Tab::Inbox => "Enter open  m read  M read-all  S all/unread",
        }
    }
}

/// The Runs tab's status filter, cycled with `S`.
const RUN_STATUS: [Option<&str>; 6] = [
    None,
    Some("queued"),
    Some("in_progress"),
    Some("completed"),
    Some("success"),
    Some("failure"),
];

/// The PR/Issue tab's state filter, cycled with `S`.
const TOPIC_STATE: [&str; 3] = ["open", "closed", "all"];

// ── rows ─────────────────────────────────────────────────────────────────────

/// What a list row points at, so `Enter` and the action keys can find it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RowKey {
    None,
    Run(usize),
    Workflow(usize),
    Topic(usize),
    Release(usize),
    Branch(usize),
    Commit(usize),
    Notification(usize),
}

/// One rendered list row: a status glyph, the text, and what it refers to.
struct Row {
    glyph: &'static str,
    glyph_scope: &'static str,
    text: String,
    /// Draw the text dimmed (closed topics, read notifications, disabled
    /// workflows).
    dim: bool,
    key: RowKey,
}

impl Row {
    fn new(glyph: &'static str, glyph_scope: &'static str, text: String, key: RowKey) -> Self {
        Row {
            glyph,
            glyph_scope,
            text,
            dim: false,
            key,
        }
    }

    /// A non-selectable informational line (the Repo tab is built from these).
    fn info(text: impl Into<String>) -> Self {
        Row {
            glyph: " ",
            glyph_scope: "ui.text",
            text: text.into(),
            dim: false,
            key: RowKey::None,
        }
    }

    fn dimmed(mut self) -> Self {
        self.dim = true;
        self
    }
}

// ── modal input ──────────────────────────────────────────────────────────────

/// What the one-line minibuffer at the top of the browser is collecting.
enum Input {
    /// Live substring filter over the visible rows.
    Search(String),
    /// Branch filter for the Runs and Commits tabs (empty clears it).
    Branch(String),
    /// `git ref` to dispatch a workflow against.
    Dispatch { id: u64, name: String, buf: String },
    /// A comment body for the selected issue or pull request.
    Comment { number: u64, buf: String },
}

impl Input {
    fn label(&self) -> String {
        match self {
            Input::Search(_) => "search".into(),
            Input::Branch(_) => "branch (empty = all)".into(),
            Input::Dispatch { name, .. } => format!("run {name} on ref"),
            Input::Comment { number, .. } => format!("comment on #{number}"),
        }
    }

    fn buffer(&mut self) -> &mut String {
        match self {
            Input::Search(s) | Input::Branch(s) => s,
            Input::Dispatch { buf, .. } | Input::Comment { buf, .. } => buf,
        }
    }

    fn text(&self) -> &str {
        match self {
            Input::Search(s) | Input::Branch(s) => s,
            Input::Dispatch { buf, .. } | Input::Comment { buf, .. } => buf,
        }
    }
}

/// A destructive action waiting for its key to be pressed a second time.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Armed {
    DeleteRun(u64),
    MergePull(u64),
}

// ── the browser ──────────────────────────────────────────────────────────────

/// The root component: tab bar over one list per tab.
pub struct GithubBrowser {
    slug: String,
    dir: PathBuf,
    tab: Tab,

    repo: Slot<Repo>,
    runs: Slot<Vec<Run>>,
    workflows: Slot<Vec<Workflow>>,
    pulls: Slot<Vec<Topic>>,
    issues: Slot<Vec<Topic>>,
    releases: Slot<Vec<Release>>,
    branches: Slot<Vec<Branch>>,
    commits: Slot<Vec<CommitRow>>,
    inbox: Slot<Vec<Notification>>,
    rate: Slot<RateLimit>,

    /// Cursor and scroll offset per tab, so switching tabs keeps your place.
    sel: [usize; TABS.len()],
    scroll: [usize; TABS.len()],
    viewport: usize,
    /// Body width and height from the last render — the key handler needs the
    /// same row set the user is looking at.
    width: usize,

    search: String,
    filter_branch: Option<String>,
    filter_workflow: Option<(u64, String)>,
    run_status: usize,
    topic_state: usize,
    inbox_all: bool,

    auto: bool,
    /// A timer is already scheduled; don't stack more.
    auto_armed: bool,
    /// The timer fired: refresh on the next render, which has `jobs`.
    auto_due: bool,
    /// A mutation finished; re-read the current tab.
    refresh_due: bool,

    next_req: u64,
    input: Option<Input>,
    armed: Option<Armed>,
    /// Result of the last mutation. Set from a job callback, which reaches the
    /// component but not the editor, so it is drawn in the header rather than
    /// pushed to the status line.
    pending_status: Option<String>,
}

impl GithubBrowser {
    /// Build a browser for the repository containing `start`, or explain why
    /// there isn't one.
    pub fn new(start: &Path, tab: Tab) -> Result<Self, String> {
        let dir = if start.is_dir() {
            start.to_path_buf()
        } else {
            start.parent().unwrap_or(Path::new(".")).to_path_buf()
        };
        let slug = github::repo_slug(&dir)?;
        Ok(GithubBrowser {
            slug,
            dir,
            tab,
            repo: Slot::default(),
            runs: Slot::default(),
            workflows: Slot::default(),
            pulls: Slot::default(),
            issues: Slot::default(),
            releases: Slot::default(),
            branches: Slot::default(),
            commits: Slot::default(),
            inbox: Slot::default(),
            rate: Slot::default(),
            sel: [0; TABS.len()],
            scroll: [0; TABS.len()],
            viewport: 1,
            width: 80,
            search: String::new(),
            filter_branch: None,
            filter_workflow: None,
            run_status: 0,
            topic_state: 0,
            inbox_all: false,
            auto: false,
            auto_armed: false,
            auto_due: false,
            refresh_due: false,
            next_req: 0,
            input: None,
            armed: None,
            pending_status: None,
        })
    }

    fn req(&mut self) -> u64 {
        self.next_req += 1;
        self.next_req
    }

    // ── endpoints ────────────────────────────────────────────────────────────

    fn runs_path(&self) -> String {
        let mut path = match &self.filter_workflow {
            Some((id, _)) => format!(
                "repos/{}/actions/workflows/{id}/runs?per_page={PAGE}",
                self.slug
            ),
            None => format!("repos/{}/actions/runs?per_page={PAGE}", self.slug),
        };
        if let Some(branch) = &self.filter_branch {
            path.push_str(&format!("&branch={}", urlq(branch)));
        }
        if let Some(status) = RUN_STATUS[self.run_status] {
            path.push_str(&format!("&status={status}"));
        }
        path
    }

    fn commits_path(&self) -> String {
        let mut path = format!("repos/{}/commits?per_page={PAGE}", self.slug);
        if let Some(branch) = &self.filter_branch {
            path.push_str(&format!("&sha={}", urlq(branch)));
        }
        path
    }

    // ── fetching ─────────────────────────────────────────────────────────────

    /// Re-read the active tab, cancelling whatever it had in flight.
    fn refresh(&mut self, jobs: &mut Jobs) {
        let req = self.req();
        let slug = self.slug.clone();
        match self.tab {
            Tab::Repo => {
                self.repo.begin(req);
                let path = format!("repos/{slug}");
                spawn(
                    jobs,
                    req,
                    move || github::api(&path).map(|v| Repo::from_json(&v)),
                    |b: &mut GithubBrowser, r, out| b.repo.deliver(r, out),
                );
            }
            Tab::Runs => {
                self.runs.begin(req);
                let path = self.runs_path();
                spawn(
                    jobs,
                    req,
                    move || github::api(&path).map(|v| Run::parse_list(&v)),
                    |b: &mut GithubBrowser, r, out| b.runs.deliver(r, out),
                );
            }
            Tab::Workflows => {
                self.workflows.begin(req);
                let path = format!("repos/{slug}/actions/workflows?per_page=100");
                spawn(
                    jobs,
                    req,
                    move || github::api(&path).map(|v| Workflow::parse_list(&v)),
                    |b: &mut GithubBrowser, r, out| b.workflows.deliver(r, out),
                );
            }
            Tab::Pulls => {
                self.pulls.begin(req);
                let state = TOPIC_STATE[self.topic_state];
                let path = format!(
                    "repos/{slug}/pulls?state={state}&per_page={PAGE}&sort=updated&direction=desc"
                );
                spawn(
                    jobs,
                    req,
                    move || github::api(&path).map(|v| Topic::parse_list(&v, true)),
                    |b: &mut GithubBrowser, r, out| b.pulls.deliver(r, out),
                );
            }
            Tab::Issues => {
                self.issues.begin(req);
                let state = TOPIC_STATE[self.topic_state];
                let path = format!(
                    "repos/{slug}/issues?state={state}&per_page={PAGE}&sort=updated&direction=desc"
                );
                spawn(
                    jobs,
                    req,
                    move || {
                        github::api(&path).map(|v| {
                            // `/issues` includes pull requests; the Issues tab
                            // shows only real issues.
                            Topic::parse_list(&v, false)
                                .into_iter()
                                .filter(|t| !t.is_pr)
                                .collect()
                        })
                    },
                    |b: &mut GithubBrowser, r, out| b.issues.deliver(r, out),
                );
            }
            Tab::Releases => {
                self.releases.begin(req);
                let path = format!("repos/{slug}/releases?per_page={PAGE}");
                spawn(
                    jobs,
                    req,
                    move || github::api(&path).map(|v| Release::parse_list(&v)),
                    |b: &mut GithubBrowser, r, out| b.releases.deliver(r, out),
                );
            }
            Tab::Branches => {
                self.branches.begin(req);
                let path = format!("repos/{slug}/branches?per_page=100");
                spawn(
                    jobs,
                    req,
                    move || github::api(&path).map(|v| Branch::parse_list(&v)),
                    |b: &mut GithubBrowser, r, out| b.branches.deliver(r, out),
                );
            }
            Tab::Commits => {
                self.commits.begin(req);
                let path = self.commits_path();
                spawn(
                    jobs,
                    req,
                    move || github::api(&path).map(|v| CommitRow::parse_list(&v)),
                    |b: &mut GithubBrowser, r, out| b.commits.deliver(r, out),
                );
            }
            Tab::Inbox => {
                self.inbox.begin(req);
                let all = self.inbox_all;
                let path = format!("notifications?all={all}&per_page={PAGE}");
                spawn(
                    jobs,
                    req,
                    move || github::api(&path).map(|v| Notification::parse_list(&v)),
                    |b: &mut GithubBrowser, r, out| b.inbox.deliver(r, out),
                );
            }
        }
    }

    /// Drop every cached tab and re-read the active one (`A`).
    fn refresh_all(&mut self, jobs: &mut Jobs) {
        self.repo = Slot::default();
        self.runs = Slot::default();
        self.workflows = Slot::default();
        self.pulls = Slot::default();
        self.issues = Slot::default();
        self.releases = Slot::default();
        self.branches = Slot::default();
        self.commits = Slot::default();
        self.inbox = Slot::default();
        self.rate = Slot::default();
        self.refresh(jobs);
    }

    /// Re-read whichever slots the active tab draws from, if they were never
    /// asked for. Called from `render`, which is the only place with `jobs`
    /// after a tab switch.
    fn ensure_loaded(&mut self, jobs: &mut Jobs) {
        let idle = match self.tab {
            Tab::Repo => self.repo.idle(),
            Tab::Runs => self.runs.idle(),
            Tab::Workflows => self.workflows.idle(),
            Tab::Pulls => self.pulls.idle(),
            Tab::Issues => self.issues.idle(),
            Tab::Releases => self.releases.idle(),
            Tab::Branches => self.branches.idle(),
            Tab::Commits => self.commits.idle(),
            Tab::Inbox => self.inbox.idle(),
        };
        if idle {
            self.refresh(jobs);
        }
        if self.rate.idle() {
            let req = self.req();
            self.rate.begin(req);
            spawn(
                jobs,
                req,
                || github::api("rate_limit").map(|v| RateLimit::from_json(&v)),
                |b: &mut GithubBrowser, r, out| b.rate.deliver(r, out),
            );
        }
    }

    /// A mutation came back: report it and re-read the tab it changed.
    fn action_done(&mut self, out: Result<String, String>) {
        match out {
            Ok(msg) => {
                self.pending_status = Some(msg);
                self.refresh_due = true;
            }
            Err(e) => self.pending_status = Some(format!("error: {e}")),
        }
        zmax_event::request_redraw();
    }

    /// Fire a mutating request; the reply sets the status line and refreshes.
    fn act(
        &mut self,
        jobs: &mut Jobs,
        method: &'static str,
        path: String,
        body: Option<Value>,
        ok: String,
    ) {
        let req = self.req();
        spawn(
            jobs,
            req,
            move || github::request(method, &path, body.as_ref()).map(|_| ok),
            |b: &mut GithubBrowser, _r, out| b.action_done(out),
        );
    }
}

// ── rows ─────────────────────────────────────────────────────────────────────

impl GithubBrowser {
    /// Every row the active tab would draw at `width` columns, before the
    /// search filter.
    fn all_rows(&self, width: usize) -> Vec<Row> {
        // Two columns are spent on the glyph and its trailing space.
        let w = width.saturating_sub(2);
        match self.tab {
            Tab::Repo => self.repo_rows(),
            Tab::Runs => self.run_rows(w),
            Tab::Workflows => self.workflow_rows(w),
            Tab::Pulls => self.topic_rows(self.pulls.data.as_deref().unwrap_or(&[]), w),
            Tab::Issues => self.topic_rows(self.issues.data.as_deref().unwrap_or(&[]), w),
            Tab::Releases => self.release_rows(w),
            Tab::Branches => self.branch_rows(w),
            Tab::Commits => self.commit_rows(w),
            Tab::Inbox => self.inbox_rows(w),
        }
    }

    /// The rows actually shown: `all_rows` minus anything the `/` search
    /// excludes.
    fn rows(&self, width: usize) -> Vec<Row> {
        let rows = self.all_rows(width);
        if self.search.is_empty() {
            return rows;
        }
        let needle = self.search.to_lowercase();
        rows.into_iter()
            .filter(|r| r.text.to_lowercase().contains(&needle))
            .collect()
    }

    fn repo_rows(&self) -> Vec<Row> {
        let Some(r) = self.repo.data.as_ref() else {
            return vec![Row::info(self.repo.placeholder("no repository data"))];
        };
        let mut rows = vec![
            Row::info(format!(
                "{}{}{}",
                r.full_name,
                if r.visibility.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", r.visibility)
                },
                if r.archived { "  [archived]" } else { "" }
            )),
            Row::info(r.description.clone()),
            Row::info(String::new()),
            Row::info(format!(
                "default branch  {}      language  {}      license  {}",
                if r.default_branch.is_empty() {
                    "-"
                } else {
                    &r.default_branch
                },
                if r.language.is_empty() {
                    "-"
                } else {
                    &r.language
                },
                if r.license.is_empty() {
                    "-"
                } else {
                    &r.license
                }
            )),
            Row::info(format!(
                "stars {}   forks {}   watchers {}   open issues {}   size {}",
                r.stars,
                r.forks,
                r.watchers,
                r.open_issues,
                github::fmt_bytes(r.size_kb * 1024)
            )),
            Row::info(format!(
                "created {}   last push {}",
                github::age_of(&r.created_at),
                github::age_of(&r.pushed_at)
            )),
        ];
        if !r.topics.is_empty() {
            rows.push(Row::info(format!("topics  {}", r.topics.join(", "))));
        }
        if !r.homepage.is_empty() {
            rows.push(Row::info(format!("homepage  {}", r.homepage)));
        }
        rows.push(Row::info(format!("url  {}", r.html_url)));
        rows
    }

    fn run_rows(&self, w: usize) -> Vec<Row> {
        let Some(runs) = self.runs.data.as_ref() else {
            return vec![Row::info(self.runs.placeholder("no workflow runs"))];
        };
        if runs.is_empty() {
            return vec![Row::info("no workflow runs match the filter")];
        }
        // #num | workflow | title | branch | duration | age
        let title_w = flex(w, 7 + 17 + 19 + 10 + 9, 72);
        runs.iter()
            .enumerate()
            .map(|(i, r)| {
                let (glyph, scope) = r.icon();
                let text = format!(
                    "{} {} {} {} {} {}",
                    pad(&format!("#{}", r.number), 6),
                    pad(&r.workflow, 16),
                    pad(&r.title, title_w),
                    pad(&r.branch, 18),
                    pad(&r.duration(), 9),
                    pad(&github::age_of(&r.created_at), 8),
                );
                let row = Row::new(glyph, scope, text, RowKey::Run(i));
                if r.conclusion.as_deref() == Some("skipped") {
                    row.dimmed()
                } else {
                    row
                }
            })
            .collect()
    }

    fn workflow_rows(&self, w: usize) -> Vec<Row> {
        let Some(workflows) = self.workflows.data.as_ref() else {
            return vec![Row::info(self.workflows.placeholder("no workflows"))];
        };
        let name_w = flex(w, 46, 40);
        workflows
            .iter()
            .enumerate()
            .map(|(i, wf)| {
                let active = wf.state == "active";
                let glyph = if active { "▸" } else { "⊘" };
                let scope = if active { "diff.plus" } else { "comment" };
                let text = format!(
                    "{} {} {}",
                    pad(&wf.name, name_w),
                    pad(&wf.path, 36),
                    pad(&wf.state, 8)
                );
                let row = Row::new(glyph, scope, text, RowKey::Workflow(i));
                if active {
                    row
                } else {
                    row.dimmed()
                }
            })
            .collect()
    }

    fn topic_rows(&self, topics: &[Topic], w: usize) -> Vec<Row> {
        let slot_msg = if self.tab == Tab::Pulls {
            self.pulls.placeholder("no pull requests")
        } else {
            self.issues.placeholder("no issues")
        };
        let loaded = if self.tab == Tab::Pulls {
            self.pulls.data.is_some()
        } else {
            self.issues.data.is_some()
        };
        if !loaded {
            return vec![Row::info(slot_msg)];
        }
        if topics.is_empty() {
            return vec![Row::info("nothing matches the state filter")];
        }
        let title_w = flex(w, 8 + 15 + 6 + 9, 84);
        topics
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let (glyph, scope) = t.icon();
                let labels = if t.labels.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", t.labels.join(","))
                };
                let title = format!(
                    "{}{}{}",
                    t.title,
                    labels,
                    if t.draft { " (draft)" } else { "" }
                );
                let text = format!(
                    "{} {} {} {} {}",
                    pad(&format!("#{}", t.number), 7),
                    pad(&title, title_w),
                    pad(&t.author, 14),
                    pad(&format!("{}c", t.comments), 5),
                    pad(&github::age_of(&t.updated_at), 8),
                );
                let row = Row::new(glyph, scope, text, RowKey::Topic(i));
                if t.state == "closed" && !t.merged {
                    row.dimmed()
                } else {
                    row
                }
            })
            .collect()
    }

    fn release_rows(&self, w: usize) -> Vec<Row> {
        let Some(releases) = self.releases.data.as_ref() else {
            return vec![Row::info(self.releases.placeholder("no releases"))];
        };
        let name_w = flex(w, 17 + 22 + 9, 48);
        releases
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let glyph = if r.draft {
                    "◌"
                } else if r.prerelease {
                    "◐"
                } else {
                    "◆"
                };
                let scope = if r.draft || r.prerelease {
                    "warning"
                } else {
                    "diff.plus"
                };
                let text = format!(
                    "{} {} {} {}",
                    pad(&r.tag, 16),
                    pad(&r.name, name_w),
                    pad(
                        &format!("{} assets, {} downloads", r.assets.len(), r.downloads()),
                        21
                    ),
                    pad(&github::age_of(&r.published_at), 8),
                );
                let row = Row::new(glyph, scope, text, RowKey::Release(i));
                if r.draft {
                    row.dimmed()
                } else {
                    row
                }
            })
            .collect()
    }

    fn branch_rows(&self, w: usize) -> Vec<Row> {
        let Some(branches) = self.branches.data.as_ref() else {
            return vec![Row::info(self.branches.placeholder("no branches"))];
        };
        let name_w = flex(w, 22, 60);
        branches
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let glyph = if b.protected { "◆" } else { "◇" };
                let text = format!(
                    "{} {} {}",
                    pad(&b.name, name_w),
                    pad(&b.sha.chars().take(7).collect::<String>(), 8),
                    if b.protected { "protected" } else { "" }
                );
                Row::new(glyph, "ui.text", text, RowKey::Branch(i))
            })
            .collect()
    }

    fn commit_rows(&self, w: usize) -> Vec<Row> {
        let Some(commits) = self.commits.data.as_ref() else {
            return vec![Row::info(self.commits.placeholder("no commits"))];
        };
        let summary_w = flex(w, 9 + 19 + 9, 90);
        commits
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let text = format!(
                    "{} {} {} {}",
                    pad(&c.short_sha(), 8),
                    pad(&c.summary, summary_w),
                    pad(&c.author, 18),
                    pad(&github::age_of(&c.date), 8),
                );
                Row::new("•", "constant", text, RowKey::Commit(i))
            })
            .collect()
    }

    fn inbox_rows(&self, w: usize) -> Vec<Row> {
        let Some(items) = self.inbox.data.as_ref() else {
            return vec![Row::info(self.inbox.placeholder("inbox is empty"))];
        };
        if items.is_empty() {
            return vec![Row::info("inbox is empty")];
        }
        let title_w = flex(w, 26 + 15 + 9, 84);
        items
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let glyph = if n.unread { "●" } else { "○" };
                let scope = if n.unread { "warning" } else { "comment" };
                let text = format!(
                    "{} {} {} {}",
                    pad(&n.repo, 25),
                    pad(&n.title, title_w),
                    pad(&n.reason, 14),
                    pad(&github::age_of(&n.updated_at), 8),
                );
                let row = Row::new(glyph, scope, text, RowKey::Notification(i));
                if n.unread {
                    row
                } else {
                    row.dimmed()
                }
            })
            .collect()
    }

    // ── selection ────────────────────────────────────────────────────────────

    fn sel(&self) -> usize {
        self.sel[self.tab.index()]
    }

    fn set_sel(&mut self, value: usize) {
        let idx = self.tab.index();
        self.sel[idx] = value;
    }

    fn move_sel(&mut self, delta: isize, len: usize) {
        if len == 0 {
            self.set_sel(0);
            return;
        }
        let max = len as isize - 1;
        let next = (self.sel() as isize + delta).clamp(0, max) as usize;
        self.set_sel(next);
    }

    /// What the cursor is on, as the key its row carries.
    fn current_key(&self, width: usize) -> RowKey {
        self.rows(width)
            .get(self.sel())
            .map(|r| r.key)
            .unwrap_or(RowKey::None)
    }

    fn current_run(&self, width: usize) -> Option<&Run> {
        match self.current_key(width) {
            RowKey::Run(i) => self.runs.data.as_ref()?.get(i),
            _ => None,
        }
    }

    fn current_workflow(&self, width: usize) -> Option<&Workflow> {
        match self.current_key(width) {
            RowKey::Workflow(i) => self.workflows.data.as_ref()?.get(i),
            _ => None,
        }
    }

    fn current_topic(&self, width: usize) -> Option<&Topic> {
        match self.current_key(width) {
            RowKey::Topic(i) => match self.tab {
                Tab::Pulls => self.pulls.data.as_ref()?.get(i),
                _ => self.issues.data.as_ref()?.get(i),
            },
            _ => None,
        }
    }

    /// The web URL of whatever the cursor is on — `o` and `y` both use it.
    fn current_url(&self, width: usize) -> String {
        match self.current_key(width) {
            RowKey::Run(i) => self
                .runs
                .data
                .as_ref()
                .and_then(|v| v.get(i))
                .map(|r| r.html_url.clone()),
            RowKey::Workflow(i) => self
                .workflows
                .data
                .as_ref()
                .and_then(|v| v.get(i))
                .map(|w| w.html_url.clone()),
            RowKey::Topic(i) => match self.tab {
                Tab::Pulls => self.pulls.data.as_ref(),
                _ => self.issues.data.as_ref(),
            }
            .and_then(|v| v.get(i))
            .map(|t| t.html_url.clone()),
            RowKey::Release(i) => self
                .releases
                .data
                .as_ref()
                .and_then(|v| v.get(i))
                .map(|r| r.html_url.clone()),
            RowKey::Branch(i) => self
                .branches
                .data
                .as_ref()
                .and_then(|v| v.get(i))
                .map(|b| format!("https://github.com/{}/tree/{}", self.slug, b.name)),
            RowKey::Commit(i) => self
                .commits
                .data
                .as_ref()
                .and_then(|v| v.get(i))
                .map(|c| c.html_url.clone()),
            RowKey::Notification(i) => self
                .inbox
                .data
                .as_ref()
                .and_then(|v| v.get(i))
                .map(|n| n.web_url()),
            RowKey::None => self.repo.data.as_ref().map(|r| r.html_url.clone()),
        }
        .unwrap_or_default()
    }
}

// ── actions ──────────────────────────────────────────────────────────────────

impl GithubBrowser {
    /// Move to `tab`, keeping that tab's own cursor.
    fn goto_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.armed = None;
    }

    /// `Enter`: open whatever the cursor is on. Some rows push a detail page,
    /// some re-filter another tab.
    fn open(&mut self, cx: &mut Context) -> Option<Callback> {
        let w = self.width;
        match self.current_key(w) {
            RowKey::Run(_) => {
                let run = self.current_run(w)?.clone();
                Some(push(GithubRun::new(self.slug.clone(), run)))
            }
            RowKey::Workflow(_) => {
                let wf = self.current_workflow(w)?.clone();
                self.filter_workflow = Some((wf.id, wf.name.clone()));
                self.runs = Slot::default();
                self.goto_tab(Tab::Runs);
                self.set_sel(0);
                None
            }
            RowKey::Topic(_) => {
                let topic = self.current_topic(w)?.clone();
                Some(push(GithubTopic::new(self.slug.clone(), topic)))
            }
            RowKey::Release(i) => {
                let release = self.releases.data.as_ref()?.get(i)?.clone();
                Some(push(GithubText::ready(
                    format!(" Release {}", release.tag),
                    release.html_url.clone(),
                    release_lines(&release),
                )))
            }
            RowKey::Branch(i) => {
                let branch = self.branches.data.as_ref()?.get(i)?.name.clone();
                self.filter_branch = Some(branch);
                self.commits = Slot::default();
                self.goto_tab(Tab::Commits);
                self.set_sel(0);
                None
            }
            RowKey::Commit(i) => {
                let commit = self.commits.data.as_ref()?.get(i)?.clone();
                let slug = self.slug.clone();
                let sha = commit.sha.clone();
                Some(push(GithubText::fetch(
                    format!(" Commit {}", commit.short_sha()),
                    commit.html_url.clone(),
                    move || {
                        github::api(&format!("repos/{slug}/commits/{sha}"))
                            .map(|v| commit_lines(&CommitDetail::from_json(&v)))
                    },
                )))
            }
            RowKey::Notification(i) => {
                let url = self.inbox.data.as_ref()?.get(i)?.web_url();
                browse(&url, cx);
                None
            }
            RowKey::None => None,
        }
    }

    /// `R` / `F` / `X` / `D` on the Runs tab.
    fn run_action(&mut self, key: char, cx: &mut Context) {
        let w = self.width;
        let Some(run) = self.current_run(w).cloned() else {
            cx.editor.set_status("no run selected");
            return;
        };
        let slug = self.slug.clone();
        let id = run.id;
        match key {
            'R' => self.act(
                cx.jobs,
                "POST",
                format!("repos/{slug}/actions/runs/{id}/rerun"),
                None,
                format!("re-running #{}", run.number),
            ),
            'F' => self.act(
                cx.jobs,
                "POST",
                format!("repos/{slug}/actions/runs/{id}/rerun-failed-jobs"),
                None,
                format!("re-running failed jobs of #{}", run.number),
            ),
            'X' => self.act(
                cx.jobs,
                "POST",
                format!("repos/{slug}/actions/runs/{id}/cancel"),
                None,
                format!("cancelling #{}", run.number),
            ),
            'D' => {
                if self.armed == Some(Armed::DeleteRun(id)) {
                    self.armed = None;
                    self.act(
                        cx.jobs,
                        "DELETE",
                        format!("repos/{slug}/actions/runs/{id}"),
                        None,
                        format!("deleted run #{}", run.number),
                    );
                } else {
                    self.armed = Some(Armed::DeleteRun(id));
                    cx.editor
                        .set_status(format!("press D again to delete run #{}", run.number));
                }
            }
            _ => {}
        }
    }

    /// `e` on the Workflows tab: flip the selected workflow between active and
    /// disabled.
    fn toggle_workflow(&mut self, cx: &mut Context) {
        let w = self.width;
        let Some(wf) = self.current_workflow(w).cloned() else {
            cx.editor.set_status("no workflow selected");
            return;
        };
        if !wf.is_file() {
            cx.editor
                .set_status("that workflow is managed by GitHub and can't be toggled");
            return;
        }
        let verb = if wf.state == "active" {
            "disable"
        } else {
            "enable"
        };
        let slug = self.slug.clone();
        self.act(
            cx.jobs,
            "PUT",
            format!("repos/{slug}/actions/workflows/{}/{verb}", wf.id),
            None,
            format!("{verb}d {}", wf.name),
        );
    }

    /// `c` on a topic tab: post a comment.
    fn comment(&mut self, number: u64, body: String, jobs: &mut Jobs) {
        let slug = self.slug.clone();
        self.act(
            jobs,
            "POST",
            format!("repos/{slug}/issues/{number}/comments"),
            Some(json!({ "body": body })),
            format!("commented on #{number}"),
        );
    }

    /// `s` on a topic tab: close an open topic, reopen a closed one.
    fn toggle_topic_state(&mut self, cx: &mut Context) {
        let w = self.width;
        let Some(topic) = self.current_topic(w).cloned() else {
            cx.editor.set_status("no topic selected");
            return;
        };
        let next = if topic.state == "open" {
            "closed"
        } else {
            "open"
        };
        let slug = self.slug.clone();
        self.act(
            cx.jobs,
            "PATCH",
            format!("repos/{slug}/issues/{}", topic.number),
            Some(json!({ "state": next })),
            format!("#{} is now {next}", topic.number),
        );
    }

    /// `M` on the PR tab: merge, on the second press.
    fn merge_pull(&mut self, cx: &mut Context) {
        let w = self.width;
        let Some(topic) = self.current_topic(w).cloned() else {
            cx.editor.set_status("no pull request selected");
            return;
        };
        if self.armed != Some(Armed::MergePull(topic.number)) {
            self.armed = Some(Armed::MergePull(topic.number));
            cx.editor
                .set_status(format!("press M again to merge #{}", topic.number));
            return;
        }
        self.armed = None;
        let slug = self.slug.clone();
        self.act(
            cx.jobs,
            "PUT",
            format!("repos/{slug}/pulls/{}/merge", topic.number),
            None,
            format!("merged #{}", topic.number),
        );
    }

    /// `C` on the PR tab: `gh pr checkout` the selected pull request locally.
    fn checkout_pull(&mut self, cx: &mut Context) {
        let w = self.width;
        let Some(topic) = self.current_topic(w).cloned() else {
            cx.editor.set_status("no pull request selected");
            return;
        };
        let dir = self.dir.clone();
        let number = topic.number;
        let req = self.req();
        spawn(
            cx.jobs,
            req,
            move || {
                let out = std::process::Command::new("gh")
                    .current_dir(&dir)
                    .args(["pr", "checkout", &number.to_string()])
                    .output()
                    .map_err(|e| format!("gh: {e}"))?;
                if out.status.success() {
                    Ok(format!("checked out #{number}"))
                } else {
                    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
                }
            },
            |b: &mut GithubBrowser, _r, out| b.action_done(out),
        );
    }

    /// `m` / `M` on the Inbox tab.
    fn inbox_action(&mut self, all: bool, cx: &mut Context) {
        if all {
            self.act(
                cx.jobs,
                "PUT",
                "notifications".to_string(),
                None,
                "marked every notification read".to_string(),
            );
            return;
        }
        let w = self.width;
        let id = match self.current_key(w) {
            RowKey::Notification(i) => self
                .inbox
                .data
                .as_ref()
                .and_then(|v| v.get(i))
                .map(|n| n.id.clone()),
            _ => None,
        };
        let Some(id) = id else {
            cx.editor.set_status("no notification selected");
            return;
        };
        self.act(
            cx.jobs,
            "PATCH",
            format!("notifications/threads/{id}"),
            None,
            "marked read".to_string(),
        );
    }

    /// `S`: cycle the tab's filter — run status, topic state, or the inbox's
    /// unread/all toggle.
    fn cycle_filter(&mut self, cx: &mut Context) {
        match self.tab {
            Tab::Runs => {
                self.run_status = (self.run_status + 1) % RUN_STATUS.len();
                self.runs = Slot::default();
                self.set_sel(0);
                let label = RUN_STATUS[self.run_status].unwrap_or("all");
                cx.editor.set_status(format!("run status: {label}"));
            }
            Tab::Pulls | Tab::Issues => {
                self.topic_state = (self.topic_state + 1) % TOPIC_STATE.len();
                self.pulls = Slot::default();
                self.issues = Slot::default();
                self.set_sel(0);
                cx.editor
                    .set_status(format!("state: {}", TOPIC_STATE[self.topic_state]));
            }
            Tab::Inbox => {
                self.inbox_all = !self.inbox_all;
                self.inbox = Slot::default();
                self.set_sel(0);
                cx.editor.set_status(if self.inbox_all {
                    "inbox: all"
                } else {
                    "inbox: unread"
                });
            }
            _ => cx.editor.set_status("no filter on this tab"),
        }
    }

    /// Apply the branch filter typed into the minibuffer (empty clears it).
    fn apply_branch(&mut self, text: &str) {
        self.filter_branch = (!text.trim().is_empty()).then(|| text.trim().to_string());
        self.runs = Slot::default();
        self.commits = Slot::default();
        self.set_sel(0);
    }

    /// The filter summary drawn under the tab bar, or `None` when nothing is
    /// filtered.
    fn filter_line(&self) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if let Some((_, name)) = &self.filter_workflow {
            parts.push(format!("workflow={name}"));
        }
        if let Some(branch) = &self.filter_branch {
            parts.push(format!("branch={branch}"));
        }
        if self.tab == Tab::Runs {
            if let Some(status) = RUN_STATUS[self.run_status] {
                parts.push(format!("status={status}"));
            }
        }
        if matches!(self.tab, Tab::Pulls | Tab::Issues) && self.topic_state != 0 {
            parts.push(format!("state={}", TOPIC_STATE[self.topic_state]));
        }
        if self.tab == Tab::Inbox && self.inbox_all {
            parts.push("inbox=all".to_string());
        }
        if !self.search.is_empty() {
            parts.push(format!("search={}", self.search));
        }
        (!parts.is_empty()).then(|| parts.join("   "))
    }

    /// Keep the auto-refresh poll running while `a` is on.
    fn tick(&mut self, jobs: &mut Jobs) {
        if self.auto_due {
            self.auto_due = false;
            if self.auto {
                self.refresh(jobs);
            }
        }
        if self.auto && !self.auto_armed {
            self.auto_armed = true;
            arm_timer(jobs, |b: &mut GithubBrowser| {
                b.auto_armed = false;
                b.auto_due = true;
            });
        }
    }
}

/// Release notes rendered as a text page.
fn release_lines(r: &Release) -> Vec<TextLine> {
    let mut lines = vec![
        TextLine::head(format!(
            "{}  ({}){}",
            if r.name.is_empty() { &r.tag } else { &r.name },
            r.tag,
            if r.draft {
                "  [draft]"
            } else if r.prerelease {
                "  [prerelease]"
            } else {
                ""
            }
        )),
        TextLine::dim(format!(
            "published {} by {}",
            github::age_of(&r.published_at),
            r.author
        )),
        TextLine::plain(String::new()),
    ];
    for line in r.body.lines() {
        lines.push(TextLine::plain(line.to_string()));
    }
    if !r.assets.is_empty() {
        lines.push(TextLine::plain(String::new()));
        lines.push(TextLine::head(format!("Assets ({})", r.assets.len())));
        for a in &r.assets {
            lines.push(TextLine::plain(format!(
                "  {}  {}  {} downloads",
                pad(&a.name, 48),
                pad(&github::fmt_bytes(a.size), 10),
                a.downloads
            )));
        }
    }
    lines
}

/// A commit's message and full diff rendered as a text page.
fn commit_lines(c: &CommitDetail) -> Vec<TextLine> {
    let mut lines = vec![
        TextLine::head(format!("commit {}", c.sha)),
        TextLine::dim(format!("{}  {}", c.author, github::age_of(&c.date))),
        TextLine::dim(format!(
            "{} files changed, +{} -{}",
            c.files.len(),
            c.additions,
            c.deletions
        )),
        TextLine::plain(String::new()),
    ];
    for line in c.message.lines() {
        lines.push(TextLine::plain(format!("    {line}")));
    }
    for f in &c.files {
        lines.push(TextLine::plain(String::new()));
        lines.extend(file_lines(f));
    }
    lines
}

/// One changed file: a header, then its patch coloured as a diff.
fn file_lines(f: &FileChange) -> Vec<TextLine> {
    let mut lines = vec![TextLine::head(format!(
        "{} {}  +{} -{}",
        match f.status.as_str() {
            "added" => "A",
            "removed" => "D",
            "renamed" => "R",
            "modified" => "M",
            other => other,
        },
        f.filename,
        f.additions,
        f.deletions
    ))];
    match &f.patch {
        Some(patch) => {
            for line in patch.lines() {
                let scope = if line.starts_with("@@") {
                    "ui.linenr"
                } else if line.starts_with('+') {
                    "diff.plus"
                } else if line.starts_with('-') {
                    "diff.minus"
                } else {
                    "ui.text"
                };
                lines.push(TextLine::new(line.to_string(), scope));
            }
        }
        None => lines.push(TextLine::dim("  (no textual diff)")),
    }
    lines
}

// ── the browser: input handling and rendering ────────────────────────────────

impl GithubBrowser {
    /// Mirror a live `/` search buffer into the applied filter as it is typed.
    fn sync_search(&mut self) {
        let text = match &self.input {
            Some(Input::Search(s)) => Some(s.clone()),
            _ => None,
        };
        if let Some(text) = text {
            self.search = text;
            self.set_sel(0);
        }
    }

    /// Keys while the one-line minibuffer is open.
    fn handle_input_key(&mut self, key: KeyEvent, cx: &mut Context) {
        match key {
            key!(Esc) | ctrl!('g') => {
                // Abandoning a search restores the unfiltered list.
                if matches!(self.input, Some(Input::Search(_))) {
                    self.search.clear();
                }
                self.input = None;
            }
            key!(Enter) => match self.input.take() {
                Some(Input::Search(text)) => {
                    self.search = text;
                    self.set_sel(0);
                }
                Some(Input::Branch(text)) => self.apply_branch(&text),
                Some(Input::Dispatch { id, name, buf }) => {
                    let git_ref = buf.trim().to_string();
                    if git_ref.is_empty() {
                        cx.editor.set_status("dispatch cancelled: no ref given");
                        return;
                    }
                    let slug = self.slug.clone();
                    self.act(
                        cx.jobs,
                        "POST",
                        format!("repos/{slug}/actions/workflows/{id}/dispatches"),
                        Some(json!({ "ref": git_ref })),
                        format!("dispatched {name} on {git_ref}"),
                    );
                }
                Some(Input::Comment { number, buf }) => {
                    if buf.trim().is_empty() {
                        cx.editor.set_status("empty comment discarded");
                    } else {
                        self.comment(number, buf, cx.jobs);
                    }
                }
                None => {}
            },
            key!(Backspace) => {
                if let Some(input) = &mut self.input {
                    input.buffer().pop();
                }
                self.sync_search();
            }
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
            } if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT => {
                if let Some(input) = &mut self.input {
                    input.buffer().push(c);
                }
                self.sync_search();
            }
            _ => {}
        }
    }

    /// Start the comment minibuffer for the selected topic.
    fn begin_comment(&mut self, cx: &mut Context) {
        let w = self.width;
        match self.current_topic(w).map(|t| t.number) {
            Some(number) => {
                self.input = Some(Input::Comment {
                    number,
                    buf: String::new(),
                })
            }
            None => cx.editor.set_status("no topic selected"),
        }
    }

    /// Start the dispatch minibuffer, pre-filled with the checked-out branch
    /// (or the repository's default branch).
    fn begin_dispatch(&mut self, cx: &mut Context) {
        let w = self.width;
        let Some(wf) = self.current_workflow(w).cloned() else {
            cx.editor.set_status("no workflow selected");
            return;
        };
        if !wf.is_file() {
            cx.editor
                .set_status("that workflow is managed by GitHub and can't be dispatched");
            return;
        }
        let default = github::current_branch(&self.dir)
            .or_else(|| self.repo.data.as_ref().map(|r| r.default_branch.clone()))
            .unwrap_or_else(|| "main".to_string());
        self.input = Some(Input::Dispatch {
            id: wf.id,
            name: wf.name,
            buf: default,
        });
    }

    /// The right-hand side of the title bar: last action result, auto-refresh
    /// state and the API budget.
    fn header_right(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(msg) = &self.pending_status {
            parts.push(msg.clone());
        }
        if self.auto {
            parts.push(format!("auto {AUTO_SECS}s"));
        }
        if let Some(rate) = self.rate.data {
            parts.push(format!("api {}/{}", rate.remaining, rate.limit));
        }
        parts.join("   ")
    }

    /// Row counts for the tab bar, so the shape of the repo is visible without
    /// visiting every tab.
    fn tab_count(&self, tab: Tab) -> Option<usize> {
        match tab {
            Tab::Repo => None,
            Tab::Runs => self.runs.data.as_ref().map(Vec::len),
            Tab::Workflows => self.workflows.data.as_ref().map(Vec::len),
            Tab::Pulls => self.pulls.data.as_ref().map(Vec::len),
            Tab::Issues => self.issues.data.as_ref().map(Vec::len),
            Tab::Releases => self.releases.data.as_ref().map(Vec::len),
            Tab::Branches => self.branches.data.as_ref().map(Vec::len),
            Tab::Commits => self.commits.data.as_ref().map(Vec::len),
            Tab::Inbox => self.inbox.data.as_ref().map(Vec::len),
        }
    }

    fn render_tab_bar(&self, surface: &mut Surface, area: Rect, theme: &zmax_view::Theme, y: u16) {
        let text = theme.get("ui.text");
        let active = bold(theme.get("ui.text.focus"));
        let sel = theme.get("ui.selection");
        let dim = theme.get("ui.linenr");
        let mut x = area.x;
        for (i, tab) in TABS.iter().enumerate() {
            let count = self
                .tab_count(*tab)
                .map(|n| format!(" {n}"))
                .unwrap_or_default();
            let label = format!(" {}:{}{} ", i + 1, tab.title(), count);
            let w = label.chars().count() as u16;
            if x + w >= area.x + area.width {
                break;
            }
            if *tab == self.tab {
                surface.set_style(Rect::new(x, y, w, 1), sel);
                surface.set_stringn(x, y, &label, w as usize, active);
            } else {
                surface.set_stringn(x, y, &label, w as usize, text);
            }
            x += w;
            if i + 1 < TABS.len() {
                surface.set_stringn(x, y, "│", 1, dim);
                x += 1;
            }
        }
    }
}

impl Component for GithubBrowser {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => normalize(*key),
            _ => return EventResult::Ignored(None),
        };

        if self.input.is_some() {
            self.handle_input_key(key, cx);
            return EventResult::Consumed(None);
        }

        let width = self.width;
        let len = self.rows(width).len();
        let page = self.viewport.max(1);

        // Any key other than a second `D`/`M` disarms a pending destructive
        // action, so it can never fire from a stale arm.
        let arming = matches!(key, key!('D') | key!('M'));
        if !arming {
            self.armed = None;
        }

        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close())),

            // movement
            key!('j') | key!(Down) | ctrl!('n') => self.move_sel(1, len),
            key!('k') | key!(Up) | ctrl!('p') => self.move_sel(-1, len),
            ctrl!('d') => self.move_sel(page as isize / 2, len),
            ctrl!('u') => self.move_sel(-(page as isize) / 2, len),
            key!(PageDown) | ctrl!('v') => self.move_sel(page as isize, len),
            key!(PageUp) | alt!('v') => self.move_sel(-(page as isize), len),
            key!('g') | key!(Home) => self.set_sel(0),
            key!('G') | key!(End) => self.set_sel(len.saturating_sub(1)),

            // tabs
            key!(Tab) | key!('l') | key!(Right) => {
                let next = (self.tab.index() + 1) % TABS.len();
                self.goto_tab(TABS[next]);
            }
            key!('h') | key!(Left) => {
                let prev = (self.tab.index() + TABS.len() - 1) % TABS.len();
                self.goto_tab(TABS[prev]);
            }
            // Shift-Tab arrives as Tab with the shift modifier.
            KeyEvent {
                code: KeyCode::Tab,
                modifiers,
            } if modifiers == KeyModifiers::SHIFT => {
                let prev = (self.tab.index() + TABS.len() - 1) % TABS.len();
                self.goto_tab(TABS[prev]);
            }
            KeyEvent {
                code: KeyCode::Char(c @ '1'..='9'),
                modifiers,
            } if modifiers == KeyModifiers::NONE => {
                let idx = c as usize - '1' as usize;
                if idx < TABS.len() {
                    self.goto_tab(TABS[idx]);
                }
            }

            // opening
            key!(Enter) => {
                if let Some(callback) = self.open(cx) {
                    return EventResult::Consumed(Some(callback));
                }
            }
            key!('o') => {
                let url = self.current_url(width);
                browse(&url, cx);
            }
            key!('y') => {
                let url = self.current_url(width);
                yank(&url, cx);
            }

            // filtering
            key!('/') => self.input = Some(Input::Search(self.search.clone())),
            key!('b') => {
                self.input = Some(Input::Branch(
                    self.filter_branch.clone().unwrap_or_default(),
                ))
            }
            key!('S') => self.cycle_filter(cx),
            key!('w') => {
                if self.filter_workflow.take().is_some() {
                    self.runs = Slot::default();
                    self.set_sel(0);
                    cx.editor.set_status("workflow filter cleared");
                }
            }

            // refreshing
            key!('r') => self.refresh(cx.jobs),
            key!('A') => self.refresh_all(cx.jobs),
            key!('a') => {
                self.auto = !self.auto;
                cx.editor.set_status(if self.auto {
                    "auto-refresh on"
                } else {
                    "auto-refresh off"
                });
                if self.auto {
                    self.tick(cx.jobs);
                }
            }

            // run actions
            key!('R') | key!('F') | key!('X') if self.tab == Tab::Runs => {
                if let KeyCode::Char(c) = key.code {
                    self.run_action(c, cx);
                }
            }
            key!('D') if self.tab == Tab::Runs => self.run_action('D', cx),

            // workflow actions
            key!('d') if self.tab == Tab::Workflows => self.begin_dispatch(cx),
            key!('e') if self.tab == Tab::Workflows => self.toggle_workflow(cx),

            // topic actions
            key!('c') if matches!(self.tab, Tab::Pulls | Tab::Issues) => self.begin_comment(cx),
            key!('s') if matches!(self.tab, Tab::Pulls | Tab::Issues) => {
                self.toggle_topic_state(cx)
            }
            key!('C') if self.tab == Tab::Pulls => self.checkout_pull(cx),
            key!('M') if self.tab == Tab::Pulls => self.merge_pull(cx),

            // inbox actions
            key!('m') if self.tab == Tab::Inbox => self.inbox_action(false, cx),
            key!('M') if self.tab == Tab::Inbox => self.inbox_action(true, cx),

            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        self.width = area.width as usize;
        if self.refresh_due {
            self.refresh_due = false;
            self.refresh(cx.jobs);
        }
        self.tick(cx.jobs);
        self.ensure_loaded(cx.jobs);

        let theme = &cx.editor.theme;
        let mut bg = theme.get("ui.background");
        if cx.editor.config().transparent_background {
            bg.bg = None;
        }
        surface.clear_with(area, bg);
        if area.width < 20 || area.height < 6 {
            return;
        }
        let head = bold(theme.get("ui.text.focus"));
        let info = theme.get("ui.linenr");
        let text = theme.get("ui.text");
        let sel_style = theme.get("ui.selection");

        // Title bar.
        let title = format!(" GitHub  {}", self.slug);
        surface.set_stringn(area.x, area.y, &title, area.width as usize, head);
        let right = self.header_right();
        if !right.is_empty() {
            let w = right.chars().count();
            if title.chars().count() + w + 3 < area.width as usize {
                surface.set_stringn(area.x + area.width - w as u16 - 1, area.y, &right, w, info);
            }
        }

        self.render_tab_bar(surface, area, theme, area.y + 1);

        // Filter / minibuffer line.
        let filter_y = area.y + 2;
        match &self.input {
            Some(input) => {
                let line = format!(" {}: {}_", input.label(), input.text());
                surface.set_stringn(area.x, filter_y, &line, area.width as usize, head);
            }
            None => {
                if let Some(line) = self.filter_line() {
                    surface.set_stringn(
                        area.x,
                        filter_y,
                        &format!(" {line}"),
                        area.width as usize,
                        info,
                    );
                }
            }
        }

        // Key hints on the last row.
        let hint = {
            let common = "j/k move  1-9/Tab tab  Enter open  o browse  y yank  / search  r refresh  A all  q quit";
            let per_tab = self.tab.hint();
            if per_tab.is_empty() {
                common.to_string()
            } else {
                format!("{common}  ·  {per_tab}")
            }
        };
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            &trunc(&hint, area.width.saturating_sub(1) as usize),
            area.width as usize,
            info,
        );

        // Rows.
        let body_y = area.y + 3;
        let body_h = area.height.saturating_sub(4) as usize;
        self.viewport = body_h.max(1);
        let rows = self.rows(area.width as usize);
        if self.sel() >= rows.len() {
            self.set_sel(rows.len().saturating_sub(1));
        }
        let idx = self.tab.index();
        self.scroll[idx] = scroll_into_view(self.sel[idx], self.scroll[idx], body_h);
        let scroll = self.scroll[idx];

        for (offset, row) in rows.iter().enumerate().skip(scroll).take(body_h) {
            let y = body_y + (offset - scroll) as u16;
            let selected = offset == self.sel();
            if selected {
                surface.set_style(Rect::new(area.x, y, area.width, 1), sel_style);
            }
            let glyph_style = if selected {
                sel_style
            } else {
                theme.get(row.glyph_scope)
            };
            surface.set_stringn(area.x, y, row.glyph, 2, glyph_style);
            let body_style = if selected {
                sel_style
            } else if row.dim {
                info
            } else {
                text
            };
            surface.set_stringn(
                area.x + 2,
                y,
                &row.text,
                area.width.saturating_sub(2) as usize,
                body_style,
            );
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("github-browser")
    }
}

// ── run detail ───────────────────────────────────────────────────────────────

/// Everything the run page shows, fetched together.
struct RunDetail {
    run: Run,
    jobs: Vec<Job>,
    artifacts: Vec<Artifact>,
}

/// What a row on the run page refers to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RunRow {
    None,
    /// Index into `jobs`.
    Job(usize),
    /// Job index and step index — opening a step opens its job's log.
    Step(usize, usize),
    Artifact(usize),
}

/// One CI run: its jobs, each job's steps with timings, and its artifacts.
///
/// `Enter` on a job (or on one of its steps) downloads that job's log into
/// [`GithubLog`]. `a` polls every few seconds so a running pipeline updates in
/// place.
pub struct GithubRun {
    slug: String,
    run: Run,
    detail: Slot<RunDetail>,
    keys: Vec<RunRow>,
    sel: usize,
    scroll: usize,
    viewport: usize,
    width: usize,
    auto: bool,
    auto_armed: bool,
    auto_due: bool,
    /// The cursor starts on the run header; once the jobs land it drops onto the
    /// first of them, so `Enter` opens a log without any navigation.
    focus_first_job: bool,
    next_req: u64,
}

impl GithubRun {
    fn new(slug: String, run: Run) -> Self {
        // A run that is still going starts polling on its own; a finished one
        // is static and doesn't need to.
        let auto = run.active();
        GithubRun {
            slug,
            run,
            detail: Slot::default(),
            keys: Vec::new(),
            sel: 0,
            scroll: 0,
            viewport: 1,
            width: 80,
            auto,
            auto_armed: false,
            auto_due: false,
            focus_first_job: true,
            next_req: 0,
        }
    }

    fn req(&mut self) -> u64 {
        self.next_req += 1;
        self.next_req
    }

    fn refresh(&mut self, jobs: &mut Jobs) {
        let req = self.req();
        self.detail.begin(req);
        let slug = self.slug.clone();
        let id = self.run.id;
        spawn(
            jobs,
            req,
            move || {
                let run = github::api(&format!("repos/{slug}/actions/runs/{id}"))
                    .map(|v| Run::from_json(&v))?;
                let jobs = github::api(&format!(
                    "repos/{slug}/actions/runs/{id}/jobs?per_page=100&filter=latest"
                ))
                .map(|v| Job::parse_list(&v))?;
                // Artifacts are informational; a repo with none, or a token
                // without the scope, shouldn't fail the whole page.
                let artifacts = github::api(&format!(
                    "repos/{slug}/actions/runs/{id}/artifacts?per_page=100"
                ))
                .map(|v| Artifact::parse_list(&v))
                .unwrap_or_default();
                Ok(RunDetail {
                    run,
                    jobs,
                    artifacts,
                })
            },
            |page: &mut GithubRun, r, out| {
                if let Ok(detail) = &out {
                    page.run = detail.run.clone();
                    // Stop polling once the pipeline is finished.
                    if !detail.run.active() {
                        page.auto = false;
                    }
                }
                page.detail.deliver(r, out);
            },
        );
    }

    fn tick(&mut self, jobs: &mut Jobs) {
        if self.auto_due {
            self.auto_due = false;
            if self.auto {
                self.refresh(jobs);
            }
        }
        if self.auto && !self.auto_armed {
            self.auto_armed = true;
            arm_timer(jobs, |page: &mut GithubRun| {
                page.auto_armed = false;
                page.auto_due = true;
            });
        }
    }

    /// Build the page's rows, recording what each one points at so the key
    /// handler can act on the cursor.
    fn rows(&mut self, width: usize) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut keys = Vec::new();
        let run = &self.run;

        let (glyph, scope) = run.icon();
        let push_row = |row: Row, key: RunRow, rows: &mut Vec<Row>, keys: &mut Vec<RunRow>| {
            rows.push(row);
            keys.push(key);
        };

        push_row(
            Row::new(
                glyph,
                scope,
                format!(
                    "{} #{}  {}  ({})",
                    run.workflow,
                    run.number,
                    github::status_word(&run.status, run.conclusion.as_deref()),
                    run.duration()
                ),
                RowKey::None,
            ),
            RunRow::None,
            &mut rows,
            &mut keys,
        );
        push_row(
            Row::info(format!(
                "{}  ·  {} on {}  ·  by {}  ·  attempt {}  ·  {}",
                run.title,
                run.event,
                run.branch,
                run.actor,
                run.attempt,
                github::age_of(&run.created_at)
            )),
            RunRow::None,
            &mut rows,
            &mut keys,
        );
        push_row(
            Row::info(format!("{}  {}", run.short_sha(), run.path)),
            RunRow::None,
            &mut rows,
            &mut keys,
        );
        push_row(Row::info(String::new()), RunRow::None, &mut rows, &mut keys);

        let Some(detail) = self.detail.data.as_ref() else {
            push_row(
                Row::info(self.detail.placeholder("no jobs")),
                RunRow::None,
                &mut rows,
                &mut keys,
            );
            self.keys = keys;
            return rows;
        };

        let name_w = flex(width, 34, 56);
        for (ji, job) in detail.jobs.iter().enumerate() {
            let (glyph, scope) = job.icon();
            let text = format!(
                "{} {} {}",
                pad(&job.name, name_w),
                pad(
                    &github::status_word(&job.status, job.conclusion.as_deref()),
                    14
                ),
                pad(&job.duration(), 10)
            );
            push_row(
                Row::new(glyph, scope, text, RowKey::None),
                RunRow::Job(ji),
                &mut rows,
                &mut keys,
            );
            for (si, step) in job.steps.iter().enumerate() {
                let (glyph, scope) = step.icon();
                let text = format!(
                    "   {} {} {}",
                    pad(&format!("{}.", step.number), 4),
                    pad(&step.name, name_w.saturating_sub(5)),
                    pad(&step.duration(), 10)
                );
                let row = Row::new(glyph, scope, text, RowKey::None);
                let row = if step.status == "completed"
                    && step.conclusion.as_deref() == Some("skipped")
                {
                    row.dimmed()
                } else {
                    row
                };
                push_row(row, RunRow::Step(ji, si), &mut rows, &mut keys);
            }
            if !job.runner.is_empty() {
                push_row(
                    Row::info(format!("   runner: {}", job.runner)).dimmed(),
                    RunRow::Job(ji),
                    &mut rows,
                    &mut keys,
                );
            }
        }

        if !detail.artifacts.is_empty() {
            push_row(Row::info(String::new()), RunRow::None, &mut rows, &mut keys);
            push_row(
                Row::new(
                    "◇",
                    "ui.text.focus",
                    format!("Artifacts ({})", detail.artifacts.len()),
                    RowKey::None,
                ),
                RunRow::None,
                &mut rows,
                &mut keys,
            );
            for (ai, artifact) in detail.artifacts.iter().enumerate() {
                let text = format!(
                    "   {} {} {}",
                    pad(&artifact.name, name_w),
                    pad(&github::fmt_bytes(artifact.size), 10),
                    if artifact.expired { "expired" } else { "" }
                );
                push_row(
                    Row::new("·", "comment", text, RowKey::None),
                    RunRow::Artifact(ai),
                    &mut rows,
                    &mut keys,
                );
            }
        }

        self.keys = keys;
        rows
    }

    /// The job the cursor is on, whether it sits on the job row or one of its
    /// steps.
    fn selected_job(&self) -> Option<&Job> {
        let detail = self.detail.data.as_ref()?;
        match self.keys.get(self.sel)? {
            RunRow::Job(j) | RunRow::Step(j, _) => detail.jobs.get(*j),
            _ => None,
        }
    }

    fn act(&mut self, jobs: &mut Jobs, path: String) {
        let req = self.req();
        spawn(
            jobs,
            req,
            move || github::request("POST", &path, None).map(|_| ()),
            |page: &mut GithubRun, _r, out| {
                if out.is_ok() {
                    // Poll again so the new state shows up without a keypress.
                    page.auto_due = true;
                    page.auto = true;
                }
                zmax_event::request_redraw();
            },
        );
    }
}

impl Component for GithubRun {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => normalize(*key),
            _ => return EventResult::Ignored(None),
        };
        let len = self.keys.len();
        let page = self.viewport.max(1);
        let slug = self.slug.clone();
        let id = self.run.id;

        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close())),
            key!('j') | key!(Down) | ctrl!('n') => {
                self.sel = (self.sel + 1).min(len.saturating_sub(1))
            }
            key!('k') | key!(Up) | ctrl!('p') => self.sel = self.sel.saturating_sub(1),
            ctrl!('d') | key!(PageDown) => {
                self.sel = (self.sel + page / 2).min(len.saturating_sub(1))
            }
            ctrl!('u') | key!(PageUp) => self.sel = self.sel.saturating_sub(page / 2),
            key!('g') | key!(Home) => self.sel = 0,
            key!('G') | key!(End) => self.sel = len.saturating_sub(1),
            key!(Enter) => {
                if let Some(job) = self.selected_job() {
                    let log = GithubLog::new(
                        self.slug.clone(),
                        job.id,
                        job.name.clone(),
                        job.html_url.clone(),
                    );
                    return EventResult::Consumed(Some(push(log)));
                }
                cx.editor.set_status("no job on this row");
            }
            key!('o') => {
                let url = self
                    .selected_job()
                    .map(|j| j.html_url.clone())
                    .unwrap_or_else(|| self.run.html_url.clone());
                browse(&url, cx);
            }
            key!('y') => {
                let url = self
                    .selected_job()
                    .map(|j| j.html_url.clone())
                    .unwrap_or_else(|| self.run.html_url.clone());
                yank(&url, cx);
            }
            key!('r') => self.refresh(cx.jobs),
            key!('a') => {
                self.auto = !self.auto;
                cx.editor.set_status(if self.auto {
                    "auto-refresh on"
                } else {
                    "auto-refresh off"
                });
                if self.auto {
                    self.tick(cx.jobs);
                }
            }
            key!('R') => {
                self.act(cx.jobs, format!("repos/{slug}/actions/runs/{id}/rerun"));
                cx.editor.set_status("re-running");
            }
            key!('F') => {
                self.act(
                    cx.jobs,
                    format!("repos/{slug}/actions/runs/{id}/rerun-failed-jobs"),
                );
                cx.editor.set_status("re-running failed jobs");
            }
            key!('X') => {
                self.act(cx.jobs, format!("repos/{slug}/actions/runs/{id}/cancel"));
                cx.editor.set_status("cancelling");
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        self.width = area.width as usize;
        self.tick(cx.jobs);
        if self.detail.idle() {
            self.refresh(cx.jobs);
        }

        let transparent = cx.editor.config().transparent_background;
        let theme = &cx.editor.theme;
        let right = if self.auto {
            format!("auto {AUTO_SECS}s")
        } else {
            String::new()
        };
        let body = frame(
            surface,
            area,
            theme,
            transparent,
            &format!(" Run #{}  {}", self.run.number, self.run.workflow),
            &right,
            "j/k move  Enter log  o browse  y yank  r refresh  a auto  R rerun  F failed  X cancel  q back",
        );
        if body.height == 0 {
            return;
        }
        self.viewport = body.height as usize;
        let rows = self.rows(area.width as usize);
        if self.focus_first_job {
            if let Some(first) = self
                .keys
                .iter()
                .position(|key| matches!(key, RunRow::Job(_)))
            {
                self.sel = first;
                self.focus_first_job = false;
            }
        }
        if self.sel >= rows.len() {
            self.sel = rows.len().saturating_sub(1);
        }
        self.scroll = scroll_into_view(self.sel, self.scroll, body.height as usize);

        let text = theme.get("ui.text");
        let info = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        for (offset, row) in rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body.height as usize)
        {
            let y = body.y + (offset - self.scroll) as u16;
            let selected = offset == self.sel;
            if selected {
                surface.set_style(Rect::new(body.x, y, body.width, 1), sel_style);
            }
            let glyph_style = if selected {
                sel_style
            } else {
                theme.get(row.glyph_scope)
            };
            surface.set_stringn(body.x, y, row.glyph, 2, glyph_style);
            let body_style = if selected {
                sel_style
            } else if row.dim {
                info
            } else {
                text
            };
            surface.set_stringn(
                body.x + 2,
                y,
                &row.text,
                body.width.saturating_sub(2) as usize,
                body_style,
            );
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("github-run")
    }
}

// ── job log ──────────────────────────────────────────────────────────────────

/// One job's Actions log.
///
/// The runner's `##[group]` sections fold (`Enter`/`Tab` on a header, `z` / `Z`
/// for all), `t` shows or hides the per-line timestamps, `E` narrows to the
/// error and warning lines, and `/` filters to lines containing a string.
pub struct GithubLog {
    slug: String,
    job_id: u64,
    title: String,
    url: String,
    slot: Slot<Vec<LogLine>>,
    /// Indices of the group headers currently collapsed.
    folded: HashSet<usize>,
    show_time: bool,
    errors_only: bool,
    filter: String,
    input: Option<String>,
    sel: usize,
    scroll: usize,
    hscroll: usize,
    viewport: usize,
    next_req: u64,
}

impl GithubLog {
    fn new(slug: String, job_id: u64, title: String, url: String) -> Self {
        GithubLog {
            slug,
            job_id,
            title,
            url,
            slot: Slot::default(),
            folded: HashSet::new(),
            show_time: false,
            errors_only: false,
            filter: String::new(),
            input: None,
            sel: 0,
            scroll: 0,
            hscroll: 0,
            viewport: 1,
            next_req: 0,
        }
    }

    fn refresh(&mut self, jobs: &mut Jobs) {
        self.next_req += 1;
        let req = self.next_req;
        self.slot.begin(req);
        let slug = self.slug.clone();
        let id = self.job_id;
        spawn(
            jobs,
            req,
            move || {
                github::api_text(&format!("repos/{slug}/actions/jobs/{id}/logs"))
                    .map(|raw| github::parse_log(&raw))
            },
            |page: &mut GithubLog, r, out| page.slot.deliver(r, out),
        );
    }

    /// Indices of the lines currently on screen, after folding and filtering.
    fn visible(&self) -> Vec<usize> {
        let Some(lines) = self.slot.data.as_ref() else {
            return Vec::new();
        };
        let needle = self.filter.to_lowercase();
        lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                if self.errors_only
                    && !matches!(line.kind, LogKind::Error | LogKind::Warning)
                    && line.kind != LogKind::Group
                {
                    return false;
                }
                if !needle.is_empty() {
                    // While filtering, group headers are only kept if they match
                    // too — the point is to see the hits, not the scaffolding.
                    return line.text.to_lowercase().contains(&needle);
                }
                match line.group {
                    Some(header) => !self.folded.contains(&header),
                    None => true,
                }
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Toggle the fold on the group the cursor is on (its header, or the group
    /// it belongs to).
    fn toggle_fold(&mut self) {
        let visible = self.visible();
        let Some(&index) = visible.get(self.sel) else {
            return;
        };
        let Some(lines) = self.slot.data.as_ref() else {
            return;
        };
        let header = match lines[index].kind {
            LogKind::Group => index,
            _ => match lines[index].group {
                Some(header) => header,
                None => return,
            },
        };
        if !self.folded.remove(&header) {
            self.folded.insert(header);
        }
    }

    fn fold_all(&mut self, fold: bool) {
        self.folded.clear();
        if !fold {
            return;
        }
        if let Some(lines) = self.slot.data.as_ref() {
            for (i, line) in lines.iter().enumerate() {
                if line.kind == LogKind::Group {
                    self.folded.insert(i);
                }
            }
        }
    }

    /// How many error and warning lines the log holds — shown in the header so
    /// a failed job says why at a glance.
    fn problem_count(&self) -> (usize, usize) {
        let Some(lines) = self.slot.data.as_ref() else {
            return (0, 0);
        };
        lines
            .iter()
            .fold((0, 0), |(errors, warnings), line| match line.kind {
                LogKind::Error => (errors + 1, warnings),
                LogKind::Warning => (errors, warnings + 1),
                _ => (errors, warnings),
            })
    }
}

impl Component for GithubLog {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => normalize(*key),
            _ => return EventResult::Ignored(None),
        };

        // The `/` filter owns every key while it is open.
        if self.input.is_some() {
            match key {
                key!(Esc) | ctrl!('g') => {
                    self.input = None;
                    self.filter.clear();
                }
                key!(Enter) => {
                    self.filter = self.input.take().unwrap_or_default();
                    self.sel = 0;
                }
                key!(Backspace) => {
                    if let Some(buf) = &mut self.input {
                        buf.pop();
                    }
                }
                KeyEvent {
                    code: KeyCode::Char(c),
                    modifiers,
                } if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT => {
                    if let Some(buf) = &mut self.input {
                        buf.push(c);
                    }
                }
                _ => {}
            }
            return EventResult::Consumed(None);
        }

        let len = self.visible().len();
        let page = self.viewport.max(1);
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close())),
            key!('j') | key!(Down) | ctrl!('n') => {
                self.sel = (self.sel + 1).min(len.saturating_sub(1))
            }
            key!('k') | key!(Up) | ctrl!('p') => self.sel = self.sel.saturating_sub(1),
            ctrl!('d') | key!(PageDown) | ctrl!('v') => {
                self.sel = (self.sel + page / 2).min(len.saturating_sub(1))
            }
            ctrl!('u') | key!(PageUp) | alt!('v') => self.sel = self.sel.saturating_sub(page / 2),
            key!('g') | key!(Home) => self.sel = 0,
            key!('G') | key!(End) => self.sel = len.saturating_sub(1),
            key!('l') | key!(Right) => self.hscroll += 8,
            key!('h') | key!(Left) => self.hscroll = self.hscroll.saturating_sub(8),
            key!(Enter) | key!(Tab) => self.toggle_fold(),
            key!('z') => self.fold_all(true),
            key!('Z') => self.fold_all(false),
            key!('t') => self.show_time = !self.show_time,
            key!('E') => {
                self.errors_only = !self.errors_only;
                self.sel = 0;
            }
            key!('/') => self.input = Some(self.filter.clone()),
            key!('r') => self.refresh(cx.jobs),
            key!('o') => browse(&self.url.clone(), cx),
            key!('y') => yank(&self.url.clone(), cx),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        if self.slot.idle() {
            self.refresh(cx.jobs);
        }
        let transparent = cx.editor.config().transparent_background;
        let theme = &cx.editor.theme;

        let (errors, warnings) = self.problem_count();
        let mut right_parts: Vec<String> = Vec::new();
        if errors > 0 || warnings > 0 {
            right_parts.push(format!("{errors} errors  {warnings} warnings"));
        }
        if self.errors_only {
            right_parts.push("errors only".to_string());
        }
        if !self.filter.is_empty() {
            right_parts.push(format!("/{}", self.filter));
        }
        let body = frame(
            surface,
            area,
            theme,
            transparent,
            &format!(" Log  {}", self.title),
            &right_parts.join("   "),
            "j/k move  Enter fold  z/Z fold-all  t times  E errors  / filter  h/l scroll  r refresh  q back",
        );
        if body.height == 0 {
            return;
        }
        self.viewport = body.height as usize;

        // The filter minibuffer replaces the first body row while it is open.
        let (body, filter_row) = match self.input.is_some() {
            true => (
                Rect::new(body.x, body.y + 1, body.width, body.height - 1),
                Some(body.y),
            ),
            false => (body, None),
        };
        if let (Some(y), Some(buf)) = (filter_row, self.input.as_ref()) {
            surface.set_stringn(
                body.x,
                y,
                &format!(" filter: {buf}_"),
                body.width as usize,
                bold(theme.get("ui.text.focus")),
            );
        }

        let visible = self.visible();
        if visible.is_empty() {
            surface.set_stringn(
                body.x,
                body.y,
                &self.slot.placeholder("log is empty"),
                body.width as usize,
                theme.get("ui.linenr"),
            );
            return;
        }
        if self.sel >= visible.len() {
            self.sel = visible.len() - 1;
        }
        self.scroll = scroll_into_view(self.sel, self.scroll, body.height as usize);

        let sel_style = theme.get("ui.selection");
        let time_style = theme.get("ui.linenr");
        let Some(lines) = self.slot.data.as_ref() else {
            return;
        };
        for (offset, &index) in visible
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body.height as usize)
        {
            let y = body.y + (offset - self.scroll) as u16;
            let line = &lines[index];
            let selected = offset == self.sel;
            if selected {
                surface.set_style(Rect::new(body.x, y, body.width, 1), sel_style);
            }
            let mut x = body.x;
            if self.show_time && !line.time.is_empty() {
                let stamp = format!("{} ", line.time);
                let w = stamp.chars().count() as u16;
                surface.set_stringn(
                    x,
                    y,
                    &stamp,
                    w as usize,
                    if selected { sel_style } else { time_style },
                );
                x += w;
            }
            // A group header shows its fold state; body lines are indented
            // under it.
            let prefix = match (line.kind, line.group) {
                (LogKind::Group, _) => {
                    if self.folded.contains(&index) {
                        "▸ "
                    } else {
                        "▾ "
                    }
                }
                (_, Some(_)) => "  ",
                _ => "",
            };
            let text = format!("{prefix}{}", line.text);
            let text: String = text.chars().skip(self.hscroll).collect();
            let style = if selected {
                sel_style
            } else {
                theme.get(line.kind.scope())
            };
            surface.set_stringn(
                x,
                y,
                &text,
                (body.x + body.width).saturating_sub(x) as usize,
                style,
            );
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("github-log")
    }
}

// ── text page ────────────────────────────────────────────────────────────────

/// The work a text page runs on its first render: it either produces the styled
/// lines or the message to show in their place. Boxed because it is handed over
/// to the worker thread, and named because the spelled-out type is what
/// `clippy::type_complexity` refuses in the field below.
type PendingFetch = Box<dyn FnOnce() -> Result<Vec<TextLine>, String> + Send>;

/// A scrollable, pre-styled text page: release notes, a commit diff, or one
/// file's patch. Content is either supplied up front ([`GithubText::ready`]) or
/// fetched on first render ([`GithubText::fetch`]).
pub struct GithubText {
    title: String,
    url: String,
    lines: Vec<TextLine>,
    error: Option<String>,
    /// The fetch to run on first render, if the content wasn't ready.
    pending: Option<PendingFetch>,
    req: Option<u64>,
    scroll: usize,
    hscroll: usize,
    viewport: usize,
}

impl GithubText {
    /// A page whose content is already in hand.
    fn ready(title: String, url: String, lines: Vec<TextLine>) -> Self {
        GithubText {
            title,
            url,
            lines,
            error: None,
            pending: None,
            req: None,
            scroll: 0,
            hscroll: 0,
            viewport: 1,
        }
    }

    /// A page that loads its content on first render.
    fn fetch(
        title: String,
        url: String,
        work: impl FnOnce() -> Result<Vec<TextLine>, String> + Send + 'static,
    ) -> Self {
        GithubText {
            title,
            url,
            lines: Vec::new(),
            error: None,
            pending: Some(Box::new(work)),
            req: None,
            scroll: 0,
            hscroll: 0,
            viewport: 1,
        }
    }

    fn deliver(&mut self, out: Result<Vec<TextLine>, String>) {
        self.req = None;
        match out {
            Ok(lines) => {
                self.lines = lines;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
}

impl Component for GithubText {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => normalize(*key),
            _ => return EventResult::Ignored(None),
        };
        let max = self.lines.len().saturating_sub(1);
        let page = self.viewport.max(1);
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close())),
            key!('j') | key!(Down) | ctrl!('n') => self.scroll = (self.scroll + 1).min(max),
            key!('k') | key!(Up) | ctrl!('p') => self.scroll = self.scroll.saturating_sub(1),
            ctrl!('d') | key!(PageDown) | ctrl!('v') => {
                self.scroll = (self.scroll + page / 2).min(max)
            }
            ctrl!('u') | key!(PageUp) | alt!('v') => {
                self.scroll = self.scroll.saturating_sub(page / 2)
            }
            key!('g') | key!(Home) => self.scroll = 0,
            key!('G') | key!(End) => self.scroll = max,
            key!('l') | key!(Right) => self.hscroll += 8,
            key!('h') | key!(Left) => self.hscroll = self.hscroll.saturating_sub(8),
            key!('o') => browse(&self.url.clone(), cx),
            key!('y') => yank(&self.url.clone(), cx),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        if let Some(work) = self.pending.take() {
            self.req = Some(1);
            spawn(cx.jobs, 1, work, |page: &mut GithubText, _r, out| {
                page.deliver(out);
                zmax_event::request_redraw();
            });
        }
        let transparent = cx.editor.config().transparent_background;
        let theme = &cx.editor.theme;
        let body = frame(
            surface,
            area,
            theme,
            transparent,
            &self.title.clone(),
            "",
            "j/k scroll  C-d/C-u page  h/l pan  o browse  y yank  q back",
        );
        if body.height == 0 {
            return;
        }
        self.viewport = body.height as usize;

        if self.lines.is_empty() {
            let message = match &self.error {
                Some(e) => format!("error: {e}"),
                None if self.req.is_some() => "loading…".to_string(),
                None => "nothing to show".to_string(),
            };
            surface.set_stringn(
                body.x,
                body.y,
                &message,
                body.width as usize,
                theme.get("ui.linenr"),
            );
            return;
        }
        if self.scroll >= self.lines.len() {
            self.scroll = self.lines.len() - 1;
        }
        for (offset, line) in self
            .lines
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body.height as usize)
        {
            let y = body.y + (offset - self.scroll) as u16;
            let text: String = line.text.chars().skip(self.hscroll).collect();
            surface.set_stringn(body.x, y, &text, body.width as usize, theme.get(line.scope));
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("github-text")
    }
}

// ── issue / pull-request detail ──────────────────────────────────────────────

/// Everything the topic page shows. Issues fill in only `body` and `comments`;
/// pull requests add their checks, files and reviews.
struct TopicDetail {
    topic: Topic,
    body: String,
    comments: Vec<Comment>,
    reviews: Vec<Review>,
    files: Vec<FileChange>,
    checks: Vec<Check>,
    additions: u64,
    deletions: u64,
}

/// What a row on the topic page refers to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TopicRow {
    None,
    File(usize),
    Check(usize),
}

/// One issue or pull request: body, CI checks, changed files, reviews and the
/// comment thread. `Enter` opens a file's patch or a check's details page.
pub struct GithubTopic {
    slug: String,
    topic: Topic,
    detail: Slot<TopicDetail>,
    keys: Vec<TopicRow>,
    sel: usize,
    scroll: usize,
    viewport: usize,
    width: usize,
    next_req: u64,
}

impl GithubTopic {
    fn new(slug: String, topic: Topic) -> Self {
        GithubTopic {
            slug,
            topic,
            detail: Slot::default(),
            keys: Vec::new(),
            sel: 0,
            scroll: 0,
            viewport: 1,
            width: 80,
            next_req: 0,
        }
    }

    fn refresh(&mut self, jobs: &mut Jobs) {
        self.next_req += 1;
        let req = self.next_req;
        self.detail.begin(req);
        let slug = self.slug.clone();
        let number = self.topic.number;
        let is_pr = self.topic.is_pr;
        spawn(
            jobs,
            req,
            move || {
                let issue = github::api(&format!("repos/{slug}/issues/{number}"))?;
                let topic = Topic::from_json(&issue, is_pr);
                let body = issue
                    .get("body")
                    .and_then(|b| b.as_str())
                    .unwrap_or("")
                    .to_string();
                let comments = github::api(&format!(
                    "repos/{slug}/issues/{number}/comments?per_page=100"
                ))
                .map(|v| Comment::parse_list(&v))
                .unwrap_or_default();
                let (mut reviews, mut files, mut checks) = (Vec::new(), Vec::new(), Vec::new());
                let (mut additions, mut deletions) = (0, 0);
                if is_pr {
                    if let Ok(pull) = github::api(&format!("repos/{slug}/pulls/{number}")) {
                        additions = pull.get("additions").and_then(|v| v.as_u64()).unwrap_or(0);
                        deletions = pull.get("deletions").and_then(|v| v.as_u64()).unwrap_or(0);
                        // Checks hang off the head commit, not the PR.
                        if let Some(sha) = pull
                            .get("head")
                            .and_then(|h| h.get("sha"))
                            .and_then(|s| s.as_str())
                        {
                            checks = github::api(&format!(
                                "repos/{slug}/commits/{sha}/check-runs?per_page=100"
                            ))
                            .map(|v| Check::parse_list(&v))
                            .unwrap_or_default();
                        }
                    }
                    files = github::api(&format!("repos/{slug}/pulls/{number}/files?per_page=100"))
                        .map(|v| FileChange::parse_list(&v))
                        .unwrap_or_default();
                    reviews =
                        github::api(&format!("repos/{slug}/pulls/{number}/reviews?per_page=100"))
                            .map(|v| Review::parse_list(&v))
                            .unwrap_or_default();
                }
                Ok(TopicDetail {
                    topic,
                    body,
                    comments,
                    reviews,
                    files,
                    checks,
                    additions,
                    deletions,
                })
            },
            |page: &mut GithubTopic, r, out| {
                if let Ok(detail) = &out {
                    page.topic = detail.topic.clone();
                }
                page.detail.deliver(r, out);
            },
        );
    }

    fn rows(&mut self, width: usize) -> Vec<Row> {
        let mut rows: Vec<Row> = Vec::new();
        let mut keys: Vec<TopicRow> = Vec::new();
        let add = |row: Row, key: TopicRow, rows: &mut Vec<Row>, keys: &mut Vec<TopicRow>| {
            rows.push(row);
            keys.push(key);
        };

        let t = &self.topic;
        let (glyph, scope) = t.icon();
        add(
            Row::new(
                glyph,
                scope,
                format!("#{}  {}", t.number, t.title),
                RowKey::None,
            ),
            TopicRow::None,
            &mut rows,
            &mut keys,
        );
        let mut meta = format!(
            "{} · by {} · updated {}",
            if t.merged {
                "merged".to_string()
            } else {
                t.state.clone()
            },
            t.author,
            github::age_of(&t.updated_at)
        );
        if t.is_pr && !t.base.is_empty() {
            meta.push_str(&format!(" · {} ← {}", t.base, t.head));
        }
        if !t.labels.is_empty() {
            meta.push_str(&format!(" · [{}]", t.labels.join(", ")));
        }
        add(Row::info(meta), TopicRow::None, &mut rows, &mut keys);

        let Some(detail) = self.detail.data.as_ref() else {
            add(
                Row::info(self.detail.placeholder("no detail")),
                TopicRow::None,
                &mut rows,
                &mut keys,
            );
            self.keys = keys;
            return rows;
        };

        if detail.additions > 0 || detail.deletions > 0 {
            add(
                Row::info(format!(
                    "{} files changed, +{} -{}",
                    detail.files.len(),
                    detail.additions,
                    detail.deletions
                )),
                TopicRow::None,
                &mut rows,
                &mut keys,
            );
        }
        add(
            Row::info(String::new()),
            TopicRow::None,
            &mut rows,
            &mut keys,
        );
        for line in detail.body.lines() {
            add(
                Row::info(line.to_string()),
                TopicRow::None,
                &mut rows,
                &mut keys,
            );
        }

        let name_w = flex(width, 30, 64);

        if !detail.checks.is_empty() {
            add(
                Row::info(String::new()),
                TopicRow::None,
                &mut rows,
                &mut keys,
            );
            add(
                Row::new(
                    "◆",
                    "ui.text.focus",
                    format!("Checks ({})", detail.checks.len()),
                    RowKey::None,
                ),
                TopicRow::None,
                &mut rows,
                &mut keys,
            );
            for (i, check) in detail.checks.iter().enumerate() {
                let (glyph, scope) = check.icon();
                let text = format!(
                    "   {} {}",
                    pad(&check.name, name_w),
                    github::status_word(&check.status, check.conclusion.as_deref())
                );
                add(
                    Row::new(glyph, scope, text, RowKey::None),
                    TopicRow::Check(i),
                    &mut rows,
                    &mut keys,
                );
            }
        }

        if !detail.files.is_empty() {
            add(
                Row::info(String::new()),
                TopicRow::None,
                &mut rows,
                &mut keys,
            );
            add(
                Row::new(
                    "◇",
                    "ui.text.focus",
                    format!("Files ({})", detail.files.len()),
                    RowKey::None,
                ),
                TopicRow::None,
                &mut rows,
                &mut keys,
            );
            for (i, f) in detail.files.iter().enumerate() {
                let text = format!(
                    "   {} {}",
                    pad(&f.filename, name_w),
                    pad(&format!("+{} -{}", f.additions, f.deletions), 14)
                );
                add(
                    Row::new("·", "constant", text, RowKey::None),
                    TopicRow::File(i),
                    &mut rows,
                    &mut keys,
                );
            }
        }

        if !detail.reviews.is_empty() {
            add(
                Row::info(String::new()),
                TopicRow::None,
                &mut rows,
                &mut keys,
            );
            add(
                Row::new(
                    "◈",
                    "ui.text.focus",
                    format!("Reviews ({})", detail.reviews.len()),
                    RowKey::None,
                ),
                TopicRow::None,
                &mut rows,
                &mut keys,
            );
            for review in &detail.reviews {
                add(
                    Row::info(format!(
                        "   {} {} {}",
                        pad(&review.author, 20),
                        pad(&review.state, 18),
                        github::age_of(&review.submitted_at)
                    )),
                    TopicRow::None,
                    &mut rows,
                    &mut keys,
                );
            }
        }

        if !detail.comments.is_empty() {
            add(
                Row::info(String::new()),
                TopicRow::None,
                &mut rows,
                &mut keys,
            );
            add(
                Row::new(
                    "»",
                    "ui.text.focus",
                    format!("Comments ({})", detail.comments.len()),
                    RowKey::None,
                ),
                TopicRow::None,
                &mut rows,
                &mut keys,
            );
            for comment in &detail.comments {
                add(
                    Row::info(format!(
                        "   {} · {}",
                        comment.author,
                        github::age_of(&comment.created_at)
                    ))
                    .dimmed(),
                    TopicRow::None,
                    &mut rows,
                    &mut keys,
                );
                for line in comment.body.lines() {
                    add(
                        Row::info(format!("     {line}")),
                        TopicRow::None,
                        &mut rows,
                        &mut keys,
                    );
                }
            }
        }

        self.keys = keys;
        rows
    }
}

impl Component for GithubTopic {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => normalize(*key),
            _ => return EventResult::Ignored(None),
        };
        let len = self.keys.len();
        let page = self.viewport.max(1);
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close())),
            key!('j') | key!(Down) | ctrl!('n') => {
                self.sel = (self.sel + 1).min(len.saturating_sub(1))
            }
            key!('k') | key!(Up) | ctrl!('p') => self.sel = self.sel.saturating_sub(1),
            ctrl!('d') | key!(PageDown) | ctrl!('v') => {
                self.sel = (self.sel + page / 2).min(len.saturating_sub(1))
            }
            ctrl!('u') | key!(PageUp) | alt!('v') => self.sel = self.sel.saturating_sub(page / 2),
            key!('g') | key!(Home) => self.sel = 0,
            key!('G') | key!(End) => self.sel = len.saturating_sub(1),
            key!('r') => self.refresh(cx.jobs),
            key!('o') => browse(&self.topic.html_url.clone(), cx),
            key!('y') => yank(&self.topic.html_url.clone(), cx),
            key!(Enter) => {
                let Some(detail) = self.detail.data.as_ref() else {
                    return EventResult::Consumed(None);
                };
                match self.keys.get(self.sel) {
                    Some(TopicRow::File(i)) => {
                        if let Some(f) = detail.files.get(*i) {
                            let page = GithubText::ready(
                                format!(" {}", f.filename),
                                self.topic.html_url.clone(),
                                file_lines(f),
                            );
                            return EventResult::Consumed(Some(push(page)));
                        }
                    }
                    Some(TopicRow::Check(i)) => {
                        if let Some(check) = detail.checks.get(*i) {
                            let url = check.details_url.clone();
                            browse(&url, cx);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        self.width = area.width as usize;
        if self.detail.idle() {
            self.refresh(cx.jobs);
        }
        let transparent = cx.editor.config().transparent_background;
        let theme = &cx.editor.theme;
        let kind = if self.topic.is_pr { "PR" } else { "Issue" };
        let body = frame(
            surface,
            area,
            theme,
            transparent,
            &format!(" {kind} #{}", self.topic.number),
            "",
            "j/k move  Enter open file/check  o browse  y yank  r refresh  q back",
        );
        if body.height == 0 {
            return;
        }
        self.viewport = body.height as usize;
        let rows = self.rows(area.width as usize);
        if self.sel >= rows.len() {
            self.sel = rows.len().saturating_sub(1);
        }
        self.scroll = scroll_into_view(self.sel, self.scroll, body.height as usize);

        let text = theme.get("ui.text");
        let info = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        for (offset, row) in rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(body.height as usize)
        {
            let y = body.y + (offset - self.scroll) as u16;
            let selected = offset == self.sel;
            if selected {
                surface.set_style(Rect::new(body.x, y, body.width, 1), sel_style);
            }
            let glyph_style = if selected {
                sel_style
            } else {
                theme.get(row.glyph_scope)
            };
            surface.set_stringn(body.x, y, row.glyph, 2, glyph_style);
            let body_style = if selected {
                sel_style
            } else if row.dim {
                info
            } else {
                text
            };
            surface.set_stringn(
                body.x + 2,
                y,
                &row.text,
                body.width.saturating_sub(2) as usize,
                body_style,
            );
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("github-topic")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifted_letters_reach_the_uppercase_bindings() {
        // Both shapes an enhanced-protocol terminal can report shift+e in.
        for code in [KeyCode::Char('E'), KeyCode::Char('e')] {
            let shifted = KeyEvent {
                code,
                modifiers: KeyModifiers::SHIFT,
            };
            assert_eq!(normalize(shifted), key!('E'), "{code:?}");
        }
        // Ctrl-shift keeps its control modifier.
        let ctrl_shift = normalize(KeyEvent {
            code: KeyCode::Char('r'),
            modifiers: KeyModifiers::SHIFT | KeyModifiers::CONTROL,
        });
        assert_eq!(ctrl_shift, ctrl!('R'));
        // Unshifted keys are untouched.
        assert_eq!(normalize(key!('q')), key!('q'));
        assert_eq!(normalize(key!(Enter)), key!(Enter));
    }

    #[test]
    fn columns_truncate_and_pad_to_width() {
        assert_eq!(pad("abc", 6), "abc   ");
        assert_eq!(pad("abcdefgh", 4), "abc…");
        assert_eq!(pad("", 3), "   ");
        assert_eq!(trunc("abc", 0), "");
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(urlq("main"), "main");
        assert_eq!(urlq("feature/x y"), "feature%2Fx%20y");
        assert_eq!(urlq("release-1.0_beta~2"), "release-1.0_beta~2");
    }

    #[test]
    fn scrolling_keeps_the_cursor_in_the_window() {
        // Cursor above the window pulls it up; below pushes it down.
        assert_eq!(scroll_into_view(0, 5, 10), 0);
        assert_eq!(scroll_into_view(12, 0, 10), 3);
        assert_eq!(scroll_into_view(4, 0, 10), 0, "already visible: unmoved");
        assert_eq!(scroll_into_view(4, 0, 0), 0, "no rows: no scroll");
    }

    #[test]
    fn tab_names_cover_the_aliases() {
        assert_eq!(Tab::from_name("ci"), Some(Tab::Runs));
        assert_eq!(Tab::from_name("PRs"), Some(Tab::Pulls));
        assert_eq!(Tab::from_name("notifications"), Some(Tab::Inbox));
        assert_eq!(Tab::from_name("nonsense"), None);
        // Every tab's own title round-trips, so `:github <title>` always works.
        for tab in TABS {
            assert_eq!(Tab::from_name(tab.title()), Some(tab), "{}", tab.title());
        }
    }

    #[test]
    fn slots_drop_stale_replies() {
        let mut slot: Slot<u32> = Slot::default();
        assert!(slot.idle(), "never asked");
        slot.begin(1);
        assert!(slot.loading());
        // A newer request supersedes the first; the first reply is discarded.
        slot.begin(2);
        slot.deliver(1, Ok(10));
        assert_eq!(slot.data, None, "stale reply ignored");
        slot.deliver(2, Ok(20));
        assert_eq!(slot.data, Some(20));
        assert!(!slot.loading() && !slot.idle());
    }

    #[test]
    fn a_file_patch_is_coloured_as_a_diff() {
        let file = FileChange {
            filename: "src/main.rs".into(),
            status: "modified".into(),
            additions: 1,
            deletions: 1,
            patch: Some("@@ -1 +1 @@\n-old\n+new\n context".into()),
        };
        let lines = file_lines(&file);
        assert_eq!(lines[0].scope, "ui.text.focus");
        assert!(lines[0].text.starts_with("M src/main.rs"));
        assert_eq!(lines[1].scope, "ui.linenr", "hunk header");
        assert_eq!(lines[2].scope, "diff.minus");
        assert_eq!(lines[3].scope, "diff.plus");
        assert_eq!(lines[4].scope, "ui.text");
    }
}
