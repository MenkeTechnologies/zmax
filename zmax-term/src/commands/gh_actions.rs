//! JetBrains GitHub plugin actions (`Github.*`) over the GitHub API client in
//! [`crate::github`]: gists, pull requests for the current branch, reviews,
//! sharing a project, and syncing a fork.
//!
//! Every request runs off the UI thread; its result comes back to the editor
//! as a status line, a clipboard entry or a scratch buffer.

use serde_json::{json, Value};

use super::{prompt_then, show_text_in_scratch, Context, Editor};
use crate::job::Callback;
use crate::ui::{overlay::overlaid, Picker, PickerColumn};

/// The repository root the current buffer (or the working directory) is in.
fn repo_dir(editor: &Editor) -> std::path::PathBuf {
    let dir = doc!(editor)
        .path()
        .and_then(|p| p.parent().map(ToOwned::to_owned))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));
    super::git_in(&dir, &["rev-parse", "--show-toplevel"])
        .map(std::path::PathBuf::from)
        .unwrap_or(dir)
}

/// Run `work` off the UI thread and hand what it returns to `done` on the
/// editor, reporting an error on the status line instead.
fn github_job(
    jobs: &mut crate::job::Jobs,
    editor: &mut Editor,
    busy: &str,
    work: impl FnOnce() -> Result<String, String> + Send + 'static,
    done: impl FnOnce(&mut Editor, String) + Send + 'static,
) {
    editor.set_status(format!("github: {busy}"));
    jobs.callback(async move {
        let result = tokio::task::spawn_blocking(work)
            .await
            .map_err(|e| anyhow::anyhow!("github task: {e}"))?;
        Ok(Callback::Editor(Box::new(move |editor: &mut Editor| match result {
            Ok(value) => done(editor, value),
            Err(e) => editor.set_error(format!("github: {e}")),
        })))
    });
}

/// The open pull request whose head is `branch` in `slug`, as its JSON.
fn branch_pull_request(slug: &str, branch: &str) -> Result<Value, String> {
    let owner = slug.split('/').next().unwrap_or_default();
    let list = crate::github::api(&format!("repos/{slug}/pulls?state=open&head={owner}:{branch}"))?;
    list.as_array()
        .and_then(|prs| prs.first().cloned())
        .ok_or_else(|| format!("no open pull request for {branch}"))
}

/// The slug and branch of the current buffer's repository.
fn slug_and_branch(editor: &mut Editor) -> Option<(String, String)> {
    let dir = repo_dir(editor);
    let slug = match crate::github::repo_slug(&dir) {
        Ok(slug) => slug,
        Err(e) => {
            editor.set_error(format!("github: {e}"));
            return None;
        }
    };
    match crate::github::current_branch(&dir) {
        Some(branch) => Some((slug, branch)),
        None => {
            editor.set_error("github: HEAD is detached");
            None
        }
    }
}

/// JetBrains "Create Gist" (`Github.Create.Gist`): the selection, or the whole
/// buffer when nothing is selected, as a secret gist. Its URL goes to the
/// clipboard.
pub fn github_create_gist(cx: &mut Context) {
    let (name, content) = {
        let (view, doc) = current_ref!(cx.editor);
        let text = doc.text().slice(..);
        let range = doc.selection(view.id).primary();
        let content = if range.len() > 1 {
            range.fragment(text).into_owned()
        } else {
            text.to_string()
        };
        let name = doc
            .path()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "snippet.txt".into());
        (name, content)
    };
    github_job(
        cx.jobs,
        cx.editor,
        "creating gist…",
        move || {
            let body = json!({ "public": false, "files": { name: { "content": content } } });
            let gist = crate::github::request("POST", "gists", Some(&body))?;
            gist["html_url"].as_str().map(str::to_string).ok_or_else(|| "no gist URL in the reply".into())
        },
        |editor, url| {
            let _ = editor.registers.write('+', vec![url.clone()]);
            editor.set_status(format!("gist created, URL yanked: {url}"));
        },
    );
}

/// JetBrains "Create Pull Request" (`Github.Create.Pull.Request`): a pull
/// request from the current branch into the repository's default branch,
/// titled from the prompt. The branch has to be pushed already.
pub fn github_create_pull_request(cx: &mut Context) {
    let Some((slug, branch)) = slug_and_branch(cx.editor) else {
        return;
    };
    prompt_then(cx, "pull request title: ", move |cx, title| {
        let (slug, branch, title) = (slug.clone(), branch.clone(), title.to_string());
        github_job(
            cx.jobs,
            cx.editor,
            "opening pull request…",
            move || {
                let repo = crate::github::api(&format!("repos/{slug}"))?;
                let base = repo["default_branch"].as_str().unwrap_or("main").to_string();
                let body = json!({ "title": title, "head": branch, "base": base });
                let pr = crate::github::request("POST", &format!("repos/{slug}/pulls"), Some(&body))?;
                pr["html_url"].as_str().map(str::to_string).ok_or_else(|| "no pull request URL in the reply".into())
            },
            |editor, url| {
                let _ = editor.registers.write('+', vec![url.clone()]);
                editor.set_status(format!("pull request opened, URL yanked: {url}"));
            },
        );
    });
}

/// JetBrains "View Pull Request in Browser" for the current branch's pull
/// request.
pub fn github_open_branch_pr(cx: &mut Context) {
    let Some((slug, branch)) = slug_and_branch(cx.editor) else {
        return;
    };
    github_job(
        cx.jobs,
        cx.editor,
        "finding the pull request…",
        move || {
            let pr = branch_pull_request(&slug, &branch)?;
            pr["html_url"].as_str().map(str::to_string).ok_or_else(|| "no URL".into())
        },
        |editor, url| match super::open_in_browser(&url) {
            Ok(()) => editor.set_status(format!("opening {url}")),
            Err(e) => editor.set_error(format!("failed to open browser: {e}")),
        },
    );
}

/// JetBrains "Copy Pull Request URL" for the current branch's pull request.
pub fn github_copy_branch_pr_url(cx: &mut Context) {
    let Some((slug, branch)) = slug_and_branch(cx.editor) else {
        return;
    };
    github_job(
        cx.jobs,
        cx.editor,
        "finding the pull request…",
        move || {
            let pr = branch_pull_request(&slug, &branch)?;
            pr["html_url"].as_str().map(str::to_string).ok_or_else(|| "no URL".into())
        },
        |editor, url| {
            let _ = editor.registers.write('+', vec![url.clone()]);
            editor.set_status(format!("yanked {url}"));
        },
    );
}

/// The pull request's number, title, state, author, branches and description
/// as text. Pure — unit tested.
fn render_pull_request(pr: &Value) -> String {
    let field = |key: &str| pr[key].as_str().unwrap_or("").to_string();
    format!(
        "#{} {}\n{} by {} — {} into {}\n{}\n\n{}\n",
        pr["number"],
        field("title"),
        if pr["merged_at"].is_string() { "merged".to_string() } else { field("state") },
        pr["user"]["login"].as_str().unwrap_or("?"),
        pr["head"]["ref"].as_str().unwrap_or("?"),
        pr["base"]["ref"].as_str().unwrap_or("?"),
        field("html_url"),
        field("body"),
    )
}

/// JetBrains `Github.PullRequest.Show`: a pull request's details by number.
pub fn github_show_pull_request(cx: &mut Context) {
    let dir = repo_dir(cx.editor);
    let slug = match crate::github::repo_slug(&dir) {
        Ok(slug) => slug,
        Err(e) => {
            cx.editor.set_error(format!("github: {e}"));
            return;
        }
    };
    prompt_then(cx, "pull request number: ", move |cx, number| {
        let (slug, number) = (slug.clone(), number.trim_start_matches('#').to_string());
        github_job(
            cx.jobs,
            cx.editor,
            "loading pull request…",
            move || crate::github::api(&format!("repos/{slug}/pulls/{number}")).map(|pr| render_pull_request(&pr)),
            |editor, text| show_text_in_scratch(editor, &text),
        );
    });
}

/// JetBrains `Github.PullRequest.Review.Submit`: approve, comment on, or
/// request changes to the current branch's pull request.
pub fn github_submit_review(cx: &mut Context) {
    let Some((slug, branch)) = slug_and_branch(cx.editor) else {
        return;
    };
    let events = ["COMMENT", "APPROVE", "REQUEST_CHANGES"].map(String::from).to_vec();
    let columns = [PickerColumn::new("review", |e: &String, _: &()| e.as_str().into())];
    let picker = Picker::new(columns, 0, events, (), move |cx, event: &String, _| {
        let (slug, branch, event) = (slug.clone(), branch.clone(), event.clone());
        super::prompt_then_cx_allow_empty(cx, "review comment: ", move |cx, body| {
            let (slug, branch, event, body) = (slug.clone(), branch.clone(), event.clone(), body.to_string());
            github_job(
                cx.jobs,
                cx.editor,
                "submitting review…",
                move || {
                    let pr = branch_pull_request(&slug, &branch)?;
                    let number = pr["number"].as_u64().ok_or("no pull request number")?;
                    let review = json!({ "event": event, "body": body });
                    crate::github::request("POST", &format!("repos/{slug}/pulls/{number}/reviews"), Some(&review))?;
                    Ok(format!("#{number}"))
                },
                |editor, pr| editor.set_status(format!("review submitted on {pr}")),
            );
        });
    });
    cx.push_layer(Box::new(overlaid(picker)));
}

/// JetBrains "Share Project on GitHub" (`Github.Share`): create a private
/// repository under your account, add it as `origin`, and push the branch.
pub fn github_share(cx: &mut Context) {
    let dir = repo_dir(cx.editor);
    let suggested = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    prompt_then(cx, "repository name (empty: the directory's): ", move |cx, name| {
        let name = if name.is_empty() { suggested.clone() } else { name.to_string() };
        let dir = dir.clone();
        github_job(
            cx.jobs,
            cx.editor,
            "creating the repository…",
            move || {
                let body = json!({ "name": name, "private": true });
                let repo = crate::github::request("POST", "user/repos", Some(&body))?;
                let url = repo["clone_url"].as_str().ok_or("no clone URL in the reply")?;
                super::git_in(&dir, &["remote", "add", "origin", url])?;
                super::git_in(&dir, &["push", "-u", "origin", "HEAD"])?;
                Ok(repo["html_url"].as_str().unwrap_or(url).to_string())
            },
            |editor, url| editor.set_status(format!("shared on GitHub: {url}")),
        );
    });
}

/// JetBrains "Sync Fork" (`Github.Sync.Fork`): "rebase your GitHub forked
/// repository relative to the origin". The fork's parent is asked of the API
/// and added as the `upstream` remote when there is none; the current branch
/// is then rebased onto the parent's default branch.
pub fn github_sync_fork(cx: &mut Context) {
    let dir = repo_dir(cx.editor);
    let slug = match crate::github::repo_slug(&dir) {
        Ok(slug) => slug,
        Err(e) => {
            cx.editor.set_error(format!("github: {e}"));
            return;
        }
    };
    github_job(
        cx.jobs,
        cx.editor,
        "syncing fork…",
        move || {
            let repo = crate::github::api(&format!("repos/{slug}"))?;
            let parent = &repo["parent"];
            let url = parent["clone_url"].as_str().ok_or("this repository is not a fork")?;
            let branch = parent["default_branch"].as_str().unwrap_or("main");
            if super::git_in(&dir, &["remote", "get-url", "upstream"]).is_err() {
                super::git_in(&dir, &["remote", "add", "upstream", url])?;
            }
            super::git_in(&dir, &["fetch", "upstream"])?;
            super::git_in(&dir, &["rebase", &format!("upstream/{branch}")])?;
            Ok(format!("upstream/{branch}"))
        },
        |editor, onto| {
            super::reload_all_open_docs(editor);
            editor.set_status(format!("rebased onto {onto}"));
        },
    );
}

#[cfg(test)]
mod tests {
    use super::render_pull_request;
    use serde_json::json;

    #[test]
    fn a_merged_pull_request_says_merged_rather_than_closed() {
        let pr = json!({
            "number": 7,
            "title": "Fix it",
            "state": "closed",
            "merged_at": "2026-01-01T00:00:00Z",
            "user": { "login": "user" },
            "head": { "ref": "fix" },
            "base": { "ref": "main" },
            "html_url": "https://example.com/pr/7",
            "body": "Details."
        });
        let text = render_pull_request(&pr);
        assert!(text.starts_with("#7 Fix it\nmerged by user — fix into main\n"), "{text}");
        assert!(text.ends_with("Details.\n"));
    }
}
