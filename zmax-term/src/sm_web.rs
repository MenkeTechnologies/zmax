//! Spacemacs `+web-services` / `+readers` layer ports: Hacker News, lobste.rs,
//! reddit, Twitch, streamlink, engine-mode search, WakaTime, Confluence,
//! Evernote (geeknote), Twitter, whisper, xkcd, elfeed and the geolocation
//! weather commands.
//!
//! Each emacs layer here is the same shape — a command talks to a JSON endpoint
//! or an external CLI and shows the result in a buffer — so every entry point
//! keeps the layer contract `pub fn name(args: &[&str]) -> Result<Outcome,
//! String>` and hands back either a status line or a page for the caller to put
//! in a scratch buffer. The shared process/HTTP plumbing lives in [`crate::sm`];
//! what is here is each layer's own request shape and rendering.
//!
//! Two pure helpers are exported outside that contract because the caller needs
//! them with buffer text rather than command words: [`to_confluence_wiki`]
//! (ox-confluence's exporter) and [`html_text`] / [`strip_tags`] (used by the
//! HN, Confluence and elfeed renderers).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

use crate::sm::{self, Outcome};

// ───────────────────────────── shared helpers ─────────────────────────────

/// Read an environment variable that a layer cannot work without, turning an
/// unset or blank value into an error that names the variable and where to get
/// its value.
fn env_required(name: &str, hint: &str) -> Result<String, String> {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(format!("${name} is unset — {hint}")),
    }
}

/// Read an environment variable, treating blank as unset.
fn env_opt(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// `$HOME`, or an error when the process has none.
fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "$HOME is unset".to_string())
}

/// Path of a per-user layer config file under `~/.config/zmax/`.
fn config_file(name: &str) -> Result<PathBuf, String> {
    Ok(home()?.join(".config").join("zmax").join(name))
}

/// Read a newline-separated config list (blank lines and `#` comments dropped).
/// When the file does not exist it is created holding `example` and the call
/// fails with a message saying so, which is how the reddit and elfeed layers
/// bootstrap their subscription lists on first use.
fn read_list_config(name: &str, example: &str) -> Result<(PathBuf, Vec<String>), String> {
    let path = config_file(name)?;
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&path, example).map_err(|e| format!("{}: {e}", path.display()))?;
        return Err(format!(
            "created {} — it holds a commented example; add your entries and re-run",
            path.display()
        ));
    }
    let body = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let lines = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    Ok((path, lines))
}

/// `v[key]` as a string, or `""`.
fn jstr<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// `v[key]` as an integer, accepting a JSON float (reddit sends `ups` either
/// way depending on the endpoint).
fn jnum(v: &Value, key: &str) -> i64 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        _ => 0,
    }
}

/// Standard base64 (RFC 4648, padded). WakaTime and Confluence both authenticate
/// with HTTP Basic and nothing else in the tree needs an encoder, so this is a
/// local 20-line implementation instead of a new dependency.
fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b1 = chunk[0] as u32;
        let b2 = *chunk.get(1).unwrap_or(&0) as u32;
        let b3 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b1 << 16) | (b2 << 8) | b3;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// `Authorization: Basic …` value for `credentials` (`user:token`).
fn basic_auth(credentials: &str) -> String {
    format!("Basic {}", base64(credentials.as_bytes()))
}

/// Remove every HTML/XML tag, mapping `<p>` to a paragraph break and `<br>` to a
/// line break. Entities are left alone — [`unescape_entities`] decodes them
/// afterwards so that an escaped `&lt;b&gt;` in the source survives as literal
/// text instead of being re-read as a tag.
pub fn strip_tags(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let after = &rest[lt + 1..];
        match after.find('>') {
            Some(gt) => {
                let name: String = after[..gt]
                    .trim()
                    .trim_start_matches('/')
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect::<String>()
                    .to_ascii_lowercase();
                match name.as_str() {
                    "p" | "div" | "li" | "tr" | "blockquote" | "pre" => out.push_str("\n\n"),
                    "br" => out.push('\n'),
                    _ => {}
                }
                rest = &after[gt + 1..];
            }
            // An unterminated `<` is literal text, not a tag.
            None => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Decode the entity set the Hacker News, Confluence and feed payloads use.
/// `&amp;` is decoded last so `&amp;lt;` yields the literal `&lt;`.
fn unescape_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&#x2F;", "/")
        .replace("&#47;", "/")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// Collapse runs of three or more newlines to a single blank line and trim.
fn squeeze_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut newlines = 0;
    for c in s.chars() {
        if c == '\n' {
            newlines += 1;
            if newlines <= 2 {
                out.push('\n');
            }
        } else {
            newlines = 0;
            out.push(c);
        }
    }
    out.trim().to_string()
}

/// Render a fragment of HTML as plain text: tags dropped, `<p>` turned into a
/// blank line, entities decoded. Hacker News comment bodies, Confluence storage
/// bodies and feed summaries are all small HTML fragments of exactly this kind,
/// so they share one renderer.
pub fn html_text(src: &str) -> String {
    squeeze_blank_lines(&unescape_entities(&strip_tags(src)))
}

// ───────────────────────────── hackernews layer ─────────────────────────────
//
// The emacs `hackernews` package reads the Firebase v0 API: one request for the
// story-id array of a feed, then one request per item. The ports below do the
// same sequentially and cap the item count so a command stays interactive.

/// Base of the Hacker News Firebase API.
const HN_API: &str = "https://hacker-news.firebaseio.com/v0";

/// Feeds `hackernews-top-stories` and friends map onto.
const HN_FEEDS: &[&str] = &["top", "new", "best", "ask", "show", "job"];

/// Fetch one Hacker News item by id.
fn hn_item(id: i64) -> Result<Value, String> {
    sm::http_get_json(&format!("{HN_API}/item/{id}.json"), &[])
}

/// `hackernews` — a story feed. First arg picks the feed (`top` default, plus
/// `new`, `best`, `ask`, `show`, `job`), second the story count (default 20,
/// capped at 100 because each story costs one extra request).
pub fn hackernews(args: &[&str]) -> Result<Outcome, String> {
    let feed = args.first().copied().unwrap_or("top");
    if !HN_FEEDS.contains(&feed) {
        return Err(format!(
            "hackernews: unknown feed `{feed}` — one of {}",
            HN_FEEDS.join(", ")
        ));
    }
    let count = match args.get(1) {
        Some(n) => n
            .parse::<usize>()
            .map_err(|_| format!("hackernews: `{n}` is not a count"))?,
        None => 20,
    }
    .clamp(1, 100);

    let ids = sm::http_get_json(&format!("{HN_API}/{feed}stories.json"), &[])?;
    let ids = ids
        .as_array()
        .ok_or_else(|| "hackernews: expected an array of story ids".to_string())?;

    let mut page = sm::heading(&format!("Hacker News — {feed} stories"));
    let mut shown = 0;
    for (i, id) in ids.iter().take(count).enumerate() {
        let Some(id) = id.as_i64() else { continue };
        let item = match hn_item(id) {
            Ok(item) => item,
            Err(e) => {
                page.push_str(&format!("{:>3}. <item {id} failed: {e}>\n\n", i + 1));
                continue;
            }
        };
        shown += 1;
        page.push_str(&format!(
            "{:>3}. {}  ({} pts, {} comments, by {})\n     {}\n     https://news.ycombinator.com/item?id={id}\n\n",
            i + 1,
            jstr(&item, "title"),
            jnum(&item, "score"),
            jnum(&item, "descendants"),
            jstr(&item, "by"),
            jstr(&item, "url"),
        ));
    }
    Ok(Outcome::page(
        format!("hackernews: {shown} {feed} stories"),
        page,
    ))
}

/// `hackernews-item` — one story plus its top-level comments. The comment
/// bodies are HTML fragments in the API, so they go through [`html_text`]. Only
/// the first 50 `kids` are fetched; the thread is one request per comment.
pub fn hackernews_item(args: &[&str]) -> Result<Outcome, String> {
    let id: i64 = args
        .first()
        .ok_or_else(|| "usage: hackernews-item <id>".to_string())?
        .parse()
        .map_err(|_| "hackernews-item: id must be a number".to_string())?;

    let item = hn_item(id)?;
    let title = jstr(&item, "title");
    let mut page = sm::heading(if title.is_empty() { "Hacker News item" } else { title });
    page.push_str(&format!(
        "{} pts, {} comments, by {}\n{}\nhttps://news.ycombinator.com/item?id={id}\n\n",
        jnum(&item, "score"),
        jnum(&item, "descendants"),
        jstr(&item, "by"),
        jstr(&item, "url"),
    ));
    let body = html_text(jstr(&item, "text"));
    if !body.is_empty() {
        page.push_str(&body);
        page.push_str("\n\n");
    }

    let kids: Vec<i64> = item
        .get("kids")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_i64).take(50).collect())
        .unwrap_or_default();
    page.push_str(&format!("── {} top-level comments ──\n\n", kids.len()));
    for kid in &kids {
        match hn_item(*kid) {
            Ok(c) => {
                if c.get("deleted").and_then(Value::as_bool).unwrap_or(false) {
                    continue;
                }
                page.push_str(&format!("{}:\n{}\n\n", jstr(&c, "by"), html_text(jstr(&c, "text"))));
            }
            Err(e) => page.push_str(&format!("<comment {kid} failed: {e}>\n\n")),
        }
    }
    Ok(Outcome::page(
        format!("hackernews-item {id}: {} comments", kids.len()),
        page,
    ))
}

// ───────────────────────────── lobsters layer ─────────────────────────────

/// `lobsters` — the lobste.rs front page. The emacs `lobsters` package reads the
/// same `hottest.json` / `newest.json` endpoints. `submitter_user` is a bare
/// username string on the current API and was an object on the older one, so
/// both shapes are accepted.
pub fn lobsters(args: &[&str]) -> Result<Outcome, String> {
    let feed = args.first().copied().unwrap_or("hottest");
    if feed != "hottest" && feed != "newest" {
        return Err(format!(
            "lobsters: unknown feed `{feed}` — hottest or newest"
        ));
    }
    let stories = sm::http_get_json(&format!("https://lobste.rs/{feed}.json"), &[])?;
    let stories = stories
        .as_array()
        .ok_or_else(|| "lobsters: expected an array of stories".to_string())?;

    let mut page = sm::heading(&format!("lobste.rs — {feed}"));
    for (i, s) in stories.iter().enumerate() {
        let tags = s
            .get("tags")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let user = match s.get("submitter_user") {
            Some(Value::String(u)) => u.clone(),
            Some(obj) => jstr(obj, "username").to_string(),
            None => String::new(),
        };
        page.push_str(&format!(
            "{:>3}. {} [{tags}]  ({} pts, {} comments, by {user})\n     {}\n     {}\n\n",
            i + 1,
            jstr(s, "title"),
            jnum(s, "score"),
            jnum(s, "comment_count"),
            jstr(s, "url"),
            jstr(s, "comments_url"),
        ));
    }
    Ok(Outcome::page(
        format!("lobsters: {} {feed} stories", stories.len()),
        page,
    ))
}

// ───────────────────────────── reddit layer ─────────────────────────────
//
// The emacs layer is `reddigg`, which reads reddit's public `.json` views. The
// only non-obvious requirement is the User-Agent: reddit answers a generic or
// missing one with 429, so every request here sends the layer's own.

/// User-Agent reddit's public JSON requires. A default agent is rate-limited.
const REDDIT_UA: (&str, &str) = ("User-Agent", "zmax:spacemacs-reddit-layer:1.0");

/// Example written to `~/.config/zmax/reddit-subs` on first use.
const REDDIT_SUBS_EXAMPLE: &str = "\
# reddigg-subs: one subreddit per line, no r/ prefix.
# Lines starting with # are ignored.
#rust
#emacs
#commandline
";

/// Render one `t3` listing (`data.children[].data`) as numbered story lines.
fn reddit_render_listing(listing: &Value, page: &mut String) -> usize {
    let children = listing
        .get("data")
        .and_then(|d| d.get("children"))
        .and_then(Value::as_array);
    let Some(children) = children else {
        return 0;
    };
    for (i, child) in children.iter().enumerate() {
        let Some(d) = child.get("data") else { continue };
        page.push_str(&format!(
            "{:>3}. {}  ({} pts, {} comments, r/{}, by u/{})\n     {}\n     https://reddit.com{}\n\n",
            i + 1,
            jstr(d, "title"),
            jnum(d, "ups"),
            jnum(d, "num_comments"),
            jstr(d, "subreddit"),
            jstr(d, "author"),
            jstr(d, "url"),
            jstr(d, "permalink"),
        ));
    }
    children.len()
}

/// Fetch a subreddit's listing.
fn reddit_listing(sub: &str, sort: &str, limit: usize) -> Result<Value, String> {
    sm::http_get_json(
        &format!("https://www.reddit.com/r/{sub}/{sort}.json?limit={limit}"),
        &[REDDIT_UA],
    )
}

/// `reddigg-view-sub` — one subreddit's listing. `<subreddit> [sort] [limit]`
/// with sort in hot/new/top/rising (default hot) and limit defaulting to 25.
pub fn reddit_view_sub(args: &[&str]) -> Result<Outcome, String> {
    let sub = args
        .first()
        .copied()
        .ok_or_else(|| "usage: reddit-view-sub <subreddit> [hot|new|top|rising] [limit]".to_string())?
        .trim_start_matches("r/");
    let sort = args.get(1).copied().unwrap_or("hot");
    if !["hot", "new", "top", "rising"].contains(&sort) {
        return Err(format!(
            "reddit-view-sub: unknown sort `{sort}` — hot, new, top or rising"
        ));
    }
    let limit = match args.get(2) {
        Some(n) => n
            .parse::<usize>()
            .map_err(|_| format!("reddit-view-sub: `{n}` is not a limit"))?,
        None => 25,
    }
    .clamp(1, 100);

    let listing = reddit_listing(sub, sort, limit)?;
    let mut page = sm::heading(&format!("r/{sub} — {sort}"));
    let n = reddit_render_listing(&listing, &mut page);
    Ok(Outcome::page(format!("r/{sub}: {n} posts"), page))
}

/// `reddigg-view-main` — the hot listing of every subreddit in `reddigg-subs`,
/// which here is `~/.config/zmax/reddit-subs` (one subreddit per line). The file
/// is created with a commented example the first time this runs.
pub fn reddit_view_main(_args: &[&str]) -> Result<Outcome, String> {
    let (path, subs) = read_list_config("reddit-subs", REDDIT_SUBS_EXAMPLE)?;
    if subs.is_empty() {
        return Err(format!("{}: no subreddits listed", path.display()));
    }
    let mut page = sm::heading("reddit — subscribed");
    let mut total = 0;
    for sub in &subs {
        let sub = sub.trim_start_matches("r/");
        page.push_str(&format!("── r/{sub} ──\n\n"));
        match reddit_listing(sub, "hot", 10) {
            Ok(listing) => total += reddit_render_listing(&listing, &mut page),
            Err(e) => page.push_str(&format!("  <failed: {e}>\n\n")),
        }
    }
    Ok(Outcome::page(
        format!("reddit: {total} posts across {} subreddits", subs.len()),
        page,
    ))
}

/// Walk reddit's comment tree depth-first, two spaces of indent per level.
/// `budget` is decremented across the whole walk so a huge thread cannot blow up
/// the page.
fn reddit_walk_comments(listing: &Value, depth: usize, budget: &mut usize, page: &mut String) {
    let Some(children) = listing
        .get("data")
        .and_then(|d| d.get("children"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for child in children {
        if *budget == 0 {
            return;
        }
        if jstr(child, "kind") == "more" {
            continue;
        }
        let Some(d) = child.get("data") else { continue };
        let body = jstr(d, "body");
        if body.is_empty() {
            continue;
        }
        *budget -= 1;
        let indent = "  ".repeat(depth);
        page.push_str(&format!("{indent}u/{}: ", jstr(d, "author")));
        for (i, line) in html_text(body).lines().enumerate() {
            if i == 0 {
                page.push_str(line);
            } else {
                page.push_str(&format!("\n{indent}{line}"));
            }
        }
        page.push_str("\n\n");
        if let Some(replies) = d.get("replies") {
            if replies.is_object() {
                reddit_walk_comments(replies, depth + 1, budget, page);
            }
        }
    }
}

/// `reddigg-view-comments` — a post's comment tree as indented text.
/// `<subreddit> <post-id>`. Capped at 200 comments.
pub fn reddit_comments(args: &[&str]) -> Result<Outcome, String> {
    let (sub, id) = match (args.first(), args.get(1)) {
        (Some(s), Some(i)) => (s.trim_start_matches("r/"), *i),
        _ => return Err("usage: reddit-comments <subreddit> <post-id>".to_string()),
    };
    let doc = sm::http_get_json(
        &format!("https://www.reddit.com/r/{sub}/comments/{id}.json"),
        &[REDDIT_UA],
    )?;
    let listings = doc
        .as_array()
        .ok_or_else(|| "reddit-comments: expected two listings".to_string())?;

    let mut page = String::new();
    if let Some(post) = listings
        .first()
        .and_then(|l| l.get("data"))
        .and_then(|d| d.get("children"))
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("data"))
    {
        page.push_str(&sm::heading(jstr(post, "title")));
        page.push_str(&format!(
            "{} pts, r/{}, by u/{}\nhttps://reddit.com{}\n\n",
            jnum(post, "ups"),
            jstr(post, "subreddit"),
            jstr(post, "author"),
            jstr(post, "permalink"),
        ));
        let self_text = html_text(jstr(post, "selftext"));
        if !self_text.is_empty() {
            page.push_str(&self_text);
            page.push_str("\n\n");
        }
    }

    let mut budget = 200usize;
    if let Some(comments) = listings.get(1) {
        reddit_walk_comments(comments, 0, &mut budget, &mut page);
    }
    Ok(Outcome::page(
        format!("reddit-comments: {} comments", 200 - budget),
        page,
    ))
}

// ───────────────────────────── twitch layer ─────────────────────────────

/// Twitch Helix credentials: every request needs both the client id and a
/// bearer token. Emacs' `twitch.el` reads the same pair from customs.
fn twitch_credentials() -> Result<(String, String), String> {
    let hint = "register an application at https://dev.twitch.tv/console and export \
                $TWITCH_CLIENT_ID and $TWITCH_OAUTH_TOKEN";
    let id = env_required("TWITCH_CLIENT_ID", hint)?;
    let token = env_required("TWITCH_OAUTH_TOKEN", hint)?;
    // `oauth:` is the prefix the Twitch CLI and IRC clients hand out; Helix
    // wants the bare token after `Bearer `.
    Ok((
        id,
        format!("Bearer {}", token.trim().trim_start_matches("oauth:")),
    ))
}

/// `twitch-search` — live channels matching a query, via
/// `helix/search/channels?live_only=true`.
pub fn twitch_search(args: &[&str]) -> Result<Outcome, String> {
    if args.is_empty() {
        return Err("usage: twitch-search <query>".to_string());
    }
    let (client_id, auth) = twitch_credentials()?;
    let headers = [("Client-Id", client_id.as_str()), ("Authorization", auth.as_str())];
    let query = args.join(" ");
    let doc = sm::http_get_json(
        &format!(
            "https://api.twitch.tv/helix/search/channels?query={}&first=20&live_only=true",
            sm::urlencode(&query)
        ),
        &headers,
    )?;
    let data = doc
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "twitch-search: no data in reply".to_string())?;

    let mut page = sm::heading(&format!("Twitch — live channels for `{query}`"));
    for c in data {
        page.push_str(&format!(
            "{}  {}  [{}]  https://twitch.tv/{}\n",
            jstr(c, "display_name"),
            sm::ellipsize(jstr(c, "title"), 60),
            jstr(c, "game_name"),
            jstr(c, "broadcaster_login"),
        ));
    }
    Ok(Outcome::page(
        format!("twitch-search: {} live channels", data.len()),
        page,
    ))
}

/// `twitch-streams` — the top live streams, optionally for one game. Helix
/// filters `helix/streams` by `game_id`, not by name, so a game argument is
/// first resolved through `helix/games?name=`.
pub fn twitch_streams(args: &[&str]) -> Result<Outcome, String> {
    let (client_id, auth) = twitch_credentials()?;
    let headers = [("Client-Id", client_id.as_str()), ("Authorization", auth.as_str())];

    let (url, title) = if args.is_empty() {
        (
            "https://api.twitch.tv/helix/streams?first=20".to_string(),
            "Twitch — top live streams".to_string(),
        )
    } else {
        let name = args.join(" ");
        let games = sm::http_get_json(
            &format!(
                "https://api.twitch.tv/helix/games?name={}",
                sm::urlencode(&name)
            ),
            &headers,
        )?;
        let id = games
            .get("data")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .map(|g| jstr(g, "id").to_string())
            .filter(|id| !id.is_empty())
            .ok_or_else(|| format!("twitch-streams: no game named `{name}`"))?;
        (
            format!("https://api.twitch.tv/helix/streams?first=20&game_id={id}"),
            format!("Twitch — top live streams for {name}"),
        )
    };

    let doc = sm::http_get_json(&url, &headers)?;
    let data = doc
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "twitch-streams: no data in reply".to_string())?;

    let mut page = sm::heading(&title);
    for s in data {
        page.push_str(&format!(
            "{:>8} viewers  {}  {}  [{}]  https://twitch.tv/{}\n",
            jnum(s, "viewer_count"),
            jstr(s, "user_name"),
            sm::ellipsize(jstr(s, "title"), 50),
            jstr(s, "game_name"),
            jstr(s, "user_login"),
        ));
    }
    Ok(Outcome::page(
        format!("twitch-streams: {} live", data.len()),
        page,
    ))
}

/// `twitch-open` — the channel URL for the caller to open.
pub fn twitch_open(args: &[&str]) -> Result<Outcome, String> {
    let name = args
        .first()
        .copied()
        .ok_or_else(|| "usage: twitch-open <channel>".to_string())?;
    Ok(Outcome::status(format!(
        "https://twitch.tv/{}",
        name.trim().trim_start_matches('@')
    )))
}

// ───────────────────────────── streamlink layer ─────────────────────────────

/// `streamlink-open` — hand a stream URL to the `streamlink` binary, which opens
/// it in the configured player. The process is spawned and deliberately not
/// waited on: the player runs until the user closes it, exactly as the emacs
/// layer's `start-process` does. The child is therefore reaped by the shell/OS,
/// not by us.
pub fn streamlink_open(args: &[&str]) -> Result<Outcome, String> {
    let url = args
        .first()
        .copied()
        .ok_or_else(|| "usage: streamlink-open <url> [quality]".to_string())?;
    let quality = args.get(1).copied().unwrap_or("best");
    let child = Command::new("streamlink")
        .arg(url)
        .arg(quality)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "`streamlink` not found on PATH".to_string()
            } else {
                format!("streamlink: {e}")
            }
        })?;
    Ok(Outcome::status(format!(
        "streamlink {url} {quality} — pid {}",
        child.id()
    )))
}

/// `streamlink-qualities` — the stream names a URL offers. `--json` gives them
/// as the keys of `streams`; when that fails (older builds, plugin errors) the
/// plain invocation's "Available streams:" line is parsed instead.
pub fn streamlink_qualities(args: &[&str]) -> Result<Outcome, String> {
    let url = args
        .first()
        .copied()
        .ok_or_else(|| "usage: streamlink-qualities <url>".to_string())?;

    if let Ok(out) = sm::run("streamlink", &["--json", url]) {
        if let Ok(doc) = serde_json::from_str::<Value>(&out) {
            if let Some(streams) = doc.get("streams").and_then(Value::as_object) {
                let mut page = sm::heading(&format!("streamlink — {url}"));
                for (name, s) in streams {
                    let kind = jstr(s, "type");
                    page.push_str(&format!("{name}  {kind}\n"));
                }
                return Ok(Outcome::page(
                    format!("streamlink: {} qualities", streams.len()),
                    page,
                ));
            }
        }
    }

    // Fallback: streamlink prints `Available streams: 160p (worst), …` on the
    // plain invocation, on stdout when it succeeds and stderr when it does not.
    let text = match sm::run("streamlink", &[url]) {
        Ok(out) => out,
        Err(err) => err,
    };
    let line = text
        .lines()
        .find(|l| l.contains("Available streams:"))
        .ok_or_else(|| format!("streamlink: no stream list in output: {}", text.trim()))?;
    let list = line.split("Available streams:").nth(1).unwrap_or("").trim();
    let mut page = sm::heading(&format!("streamlink — {url}"));
    for name in list.split(',') {
        page.push_str(name.trim());
        page.push('\n');
    }
    Ok(Outcome::page("streamlink: qualities", page))
}

// ─────────────────────────── search-engine layer ───────────────────────────
//
// The emacs layer wraps `engine-mode`, whose `defengine` forms are a name plus a
// URL template with the hexified query substituted in. The table below is the
// same idea with `{}` as the placeholder; `{tld}` is Amazon's
// `search-engine-amazon-tld`, resolved at build time from the environment.

/// Search engines and their URL templates. `{}` is replaced by the urlencoded
/// query, `{tld}` by `$SEARCH_ENGINE_AMAZON_TLD` (default `com`).
pub const SEARCH_ENGINES: &[(&str, &str)] = &[
    ("google", "https://www.google.com/search?q={}"),
    ("duckduckgo", "https://duckduckgo.com/?q={}"),
    ("github", "https://github.com/search?q={}"),
    ("gitlab", "https://gitlab.com/search?search={}"),
    ("stackoverflow", "https://stackoverflow.com/search?q={}"),
    ("wikipedia", "https://en.wikipedia.org/w/index.php?search={}"),
    ("youtube", "https://www.youtube.com/results?search_query={}"),
    ("amazon", "https://www.amazon.{tld}/s?k={}"),
    ("rust-docs", "https://docs.rs/releases/search?query={}"),
    ("crates.io", "https://crates.io/search?q={}"),
    ("mdn", "https://developer.mozilla.org/en-US/search?q={}"),
    ("npm", "https://www.npmjs.com/search?q={}"),
    ("arch-wiki", "https://wiki.archlinux.org/index.php?search={}"),
    ("wolfram-alpha", "https://www.wolframalpha.com/input/?i={}"),
    ("google-images", "https://www.google.com/search?tbm=isch&q={}"),
    ("google-maps", "https://maps.google.com/maps?q={}"),
    ("twitter", "https://twitter.com/search?q={}"),
    ("reddit", "https://www.reddit.com/search?q={}"),
    ("hoogle", "https://hoogle.haskell.org/?hoogle={}"),
    ("ctan", "https://ctan.org/search?phrase={}"),
    ("cve", "https://cve.mitre.org/cgi-bin/cvekey.cgi?keyword={}"),
    ("dockerhub", "https://hub.docker.com/search?q={}"),
    (
        "project-gutenberg",
        "https://www.gutenberg.org/ebooks/search/?query={}",
    ),
    (
        "translate",
        "https://translate.google.com/?sl=auto&tl=en&op=translate&text={}",
    ),
    // man7.org and linux.die.net have no query endpoint; both address pages by
    // name, so these jump straight to the section-1 page for the given name.
    ("man", "https://man7.org/linux/man-pages/man1/{}.1.html"),
    ("man-die", "https://linux.die.net/man/1/{}"),
];

/// Fold an engine key to the table's spelling: case-insensitive, `_` and `-`
/// interchangeable, plus the short aliases that get typed in practice.
fn engine_key(key: &str) -> String {
    let folded: String = key
        .trim()
        .chars()
        .map(|c| if c == '_' { '-' } else { c.to_ascii_lowercase() })
        .collect();
    match folded.as_str() {
        "ddg" | "duck-duck-go" | "duckduck" => "duckduckgo".to_string(),
        "gh" => "github".to_string(),
        "so" | "stack-overflow" => "stackoverflow".to_string(),
        "wiki" => "wikipedia".to_string(),
        "yt" => "youtube".to_string(),
        "docs.rs" | "rustdoc" | "rust-doc" => "rust-docs".to_string(),
        "crates" | "cratesio" => "crates.io".to_string(),
        "docker" | "docker-hub" => "dockerhub".to_string(),
        "images" => "google-images".to_string(),
        "maps" => "google-maps".to_string(),
        "wolfram" => "wolfram-alpha".to_string(),
        _ => folded,
    }
}

/// Build the URL for `engine` searching `query`, or `None` when no such engine
/// is in [`SEARCH_ENGINES`].
pub fn search_engine_url(engine: &str, query: &str) -> Option<String> {
    let key = engine_key(engine);
    let (_, template) = SEARCH_ENGINES.iter().find(|(name, _)| *name == key)?;
    let tld = env_opt("SEARCH_ENGINE_AMAZON_TLD").unwrap_or_else(|| "com".to_string());
    Some(
        template
            .replace("{tld}", tld.trim())
            .replace("{}", &sm::urlencode(query.trim())),
    )
}

/// Engine names worth suggesting for a key that did not resolve: those sharing a
/// substring or a first letter with it, falling back to the whole table.
fn nearest_engines(key: &str) -> String {
    let k = engine_key(key);
    let first = k.chars().next();
    let mut hits: Vec<&str> = SEARCH_ENGINES
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| name.contains(&k) || k.contains(*name) || name.chars().next() == first)
        .collect();
    if hits.is_empty() {
        hits = SEARCH_ENGINES.iter().map(|(name, _)| *name).collect();
    }
    hits.join(", ")
}

/// `search-engine-list` — the configured engines and their templates.
pub fn search_engine_list(_args: &[&str]) -> Result<Outcome, String> {
    let width = SEARCH_ENGINES
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);
    let mut page = sm::heading("search engines");
    for (name, template) in SEARCH_ENGINES {
        page.push_str(&format!("{name:width$}  {template}\n"));
    }
    Ok(Outcome::page(
        format!("{} search engines", SEARCH_ENGINES.len()),
        page,
    ))
}

/// `engine-mode` search — `<engine> <query…>`. The built URL comes back on the
/// status line for the caller to open.
pub fn search_engine(args: &[&str]) -> Result<Outcome, String> {
    let engine = args
        .first()
        .copied()
        .ok_or_else(|| "usage: search-engine <engine> <query…>".to_string())?;
    if args.len() < 2 {
        return Err("usage: search-engine <engine> <query…>".to_string());
    }
    let query = args[1..].join(" ");
    search_engine_url(engine, &query).map(Outcome::status).ok_or_else(|| {
        format!(
            "search-engine: unknown engine `{engine}` — did you mean: {}",
            nearest_engines(engine)
        )
    })
}

// ───────────────────────────── wakatime layer ─────────────────────────────

/// The wakatime binary the layer drives. Newer releases ship `wakatime-cli`;
/// the older python package installed `wakatime`.
fn wakatime_binary() -> Result<&'static str, String> {
    if sm::have("wakatime-cli") {
        Ok("wakatime-cli")
    } else if sm::have("wakatime") {
        Ok("wakatime")
    } else {
        Err("neither `wakatime-cli` nor `wakatime` is on PATH".to_string())
    }
}

/// `wakatime-status` — today's total coding time, from the local CLI.
pub fn wakatime_status(_args: &[&str]) -> Result<Outcome, String> {
    let out = sm::run(wakatime_binary()?, &["--today"])?;
    let today = out.trim();
    Ok(Outcome::status(format!(
        "wakatime today: {}",
        if today.is_empty() { "no data" } else { today }
    )))
}

/// `wakatime-heartbeat` — send one heartbeat for a file, which is what the emacs
/// minor mode does on save/buffer change.
pub fn wakatime_heartbeat(args: &[&str]) -> Result<Outcome, String> {
    let file = args
        .first()
        .copied()
        .ok_or_else(|| "usage: wakatime-heartbeat <file>".to_string())?;
    sm::run(
        wakatime_binary()?,
        &["--entity", file, "--plugin", "zmax", "--write"],
    )?;
    Ok(Outcome::status(format!("wakatime: heartbeat sent for {file}")))
}

/// `wakatime-dashboard` — the dashboard URL for the caller to open.
pub fn wakatime_dashboard(_args: &[&str]) -> Result<Outcome, String> {
    Ok(Outcome::status("https://wakatime.com/dashboard"))
}

/// Format a second count as `Hh Mm`.
fn hms(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{}h {:02}m", total / 3600, (total % 3600) / 60)
}

/// `wakatime-summary` — per-language and per-project totals from the WakaTime
/// API. `range` defaults to `Today` and accepts the API's other spellings
/// (`Yesterday`, `Last 7 Days`, `Last 30 Days`, …). Authenticates with HTTP
/// Basic over the raw API key, which is what WakaTime documents.
pub fn wakatime_summary(args: &[&str]) -> Result<Outcome, String> {
    let key = env_required(
        "WAKATIME_API_KEY",
        "copy it from https://wakatime.com/settings/api-key",
    )?;
    let range = if args.is_empty() {
        "Today".to_string()
    } else {
        args.join(" ")
    };
    let auth = basic_auth(key.trim());
    let doc = sm::http_get_json(
        &format!(
            "https://wakatime.com/api/v1/users/current/summaries?range={}",
            sm::urlencode(&range)
        ),
        &[("Authorization", auth.as_str())],
    )?;
    let days = doc
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "wakatime-summary: no data in reply".to_string())?;

    // Sum each bucket's `total_seconds` across every day in the range.
    let mut languages: Vec<(String, f64)> = Vec::new();
    let mut projects: Vec<(String, f64)> = Vec::new();
    let mut grand = 0.0f64;
    for day in days {
        grand += day
            .get("grand_total")
            .and_then(|g| g.get("total_seconds"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        for (bucket, into) in [("languages", &mut languages), ("projects", &mut projects)] {
            let Some(items) = day.get(bucket).and_then(Value::as_array) else {
                continue;
            };
            for item in items {
                let name = jstr(item, "name").to_string();
                let secs = item
                    .get("total_seconds")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                match into.iter_mut().find(|(n, _)| *n == name) {
                    Some(slot) => slot.1 += secs,
                    None => into.push((name, secs)),
                }
            }
        }
    }
    languages.sort_by(|a, b| b.1.total_cmp(&a.1));
    projects.sort_by(|a, b| b.1.total_cmp(&a.1));

    let mut page = sm::heading(&format!("WakaTime — {range}"));
    page.push_str(&format!("total  {}\n\n", hms(grand)));
    page.push_str("languages\n");
    for (name, secs) in &languages {
        page.push_str(&format!("  {name:20}  {}\n", hms(*secs)));
    }
    page.push_str("\nprojects\n");
    for (name, secs) in &projects {
        page.push_str(&format!("  {name:20}  {}\n", hms(*secs)));
    }
    Ok(Outcome::page(
        format!("wakatime {range}: {}", hms(grand)),
        page,
    ))
}

// ───────────────────────────── confluence layer ─────────────────────────────
//
// Two halves, as in emacs: `ox-confluence` exports an org buffer to Confluence
// wiki markup, and `confluence.el` talks to the REST API.

/// Convert markdown/org source text to Confluence wiki markup, the way
/// `ox-confluence` exports an org buffer.
///
/// Block level: ATX `#`…`######` and column-0 org `*`…`******` headings become
/// `h1.`…`h6.`; fenced code blocks become `{code:lang}`…`{code}` and their
/// contents are copied verbatim; `- `/`+ ` items become `*` items and `1. `/`1) `
/// items become `#` items, one marker per two columns of indent; a `|a|b|` row
/// becomes `|a|b|` and the first row of each table block becomes `||a||b||`,
/// with markdown separator rows (`|---|---|`) dropped.
///
/// Inline: `` `x` `` → `{{x}}`, `[t](u)` and org `[[u][t]]` → `[t|u]`,
/// `**bold**` → `*bold*`, org `/italic/` → `_italic_`. Confluence already
/// spells bold `*x*` and italic `_x_`, so those two forms pass through.
///
/// A column-0 `* ` is read as an org heading, not as a markdown bullet — that
/// ambiguity is inherent to accepting both syntaxes, and org is the format
/// ox-confluence exports.
pub fn to_confluence_wiki(src: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_code = false;
    let mut table_row = 0usize;

    for raw in src.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();

        if let Some(rest) = trimmed.strip_prefix("```") {
            if in_code {
                out.push("{code}".to_string());
            } else {
                let lang = rest.trim();
                out.push(if lang.is_empty() {
                    "{code}".to_string()
                } else {
                    format!("{{code:{lang}}}")
                });
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push(line.to_string());
            continue;
        }

        // Tables: a run of `|…|` lines, the first of which is the header.
        if trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 2 {
            let cells: Vec<&str> = trimmed.trim_matches('|').split('|').map(str::trim).collect();
            let separator = cells
                .iter()
                .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':' || ch == '+'));
            if separator {
                continue;
            }
            table_row += 1;
            let bar = if table_row == 1 { "||" } else { "|" };
            let body: Vec<String> = cells.iter().map(|c| convert_inline(c)).collect();
            out.push(format!("{bar}{}{bar}", body.join(bar)));
            continue;
        }
        table_row = 0;

        // ATX headings.
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            out.push(format!("h{hashes}. {}", convert_inline(trimmed[hashes..].trim())));
            continue;
        }
        // Org headings, which must start at column 0.
        let stars = line.chars().take_while(|c| *c == '*').count();
        if (1..=6).contains(&stars) && line.starts_with('*') && line[stars..].starts_with(' ') {
            out.push(format!("h{stars}. {}", convert_inline(line[stars..].trim())));
            continue;
        }

        let depth = (line.len() - trimmed.len()) / 2 + 1;
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            out.push(format!("{} {}", "*".repeat(depth), convert_inline(rest.trim())));
            continue;
        }
        if let Some(rest) = strip_ordered_marker(trimmed) {
            out.push(format!("{} {}", "#".repeat(depth), convert_inline(rest.trim())));
            continue;
        }

        out.push(convert_inline(line));
    }
    out.join("\n")
}

/// Strip a `1. ` / `1) ` ordered-list marker, returning the item text.
fn strip_ordered_marker(s: &str) -> Option<&str> {
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let rest = &s[digits..];
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
    rest.strip_prefix(' ')
}

/// Index of the next `needle` char in `chars` at or after `from`.
fn find_char(chars: &[char], from: usize, needle: char) -> Option<usize> {
    (from..chars.len()).find(|&i| chars[i] == needle)
}

/// Index of the next occurrence of the two-char `needle` at or after `from`.
fn find_pair(chars: &[char], from: usize, needle: [char; 2]) -> Option<usize> {
    (from..chars.len().saturating_sub(1)).find(|&i| chars[i] == needle[0] && chars[i + 1] == needle[1])
}

/// Apply the inline markup conversions of [`to_confluence_wiki`] in one
/// left-to-right pass, so a construct already rewritten is never rewritten
/// again (a URL inside a produced `[text|url]` is not read as org italics).
fn convert_inline(src: &str) -> String {
    let ch: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < ch.len() {
        let c = ch[i];

        // `code` → {{code}}
        if c == '`' {
            if let Some(end) = find_char(&ch, i + 1, '`') {
                out.push_str("{{");
                out.extend(&ch[i + 1..end]);
                out.push_str("}}");
                i = end + 1;
                continue;
            }
        }
        // org [[url][text]] / [[url]] → [text|url]
        if c == '[' && ch.get(i + 1) == Some(&'[') {
            if let Some(end) = find_pair(&ch, i + 2, [']', ']']) {
                let inner: String = ch[i + 2..end].iter().collect();
                let (url, text) = match inner.split_once("][") {
                    Some((u, t)) => (u.to_string(), t.to_string()),
                    None => (inner.clone(), inner),
                };
                out.push_str(&format!("[{text}|{url}]"));
                i = end + 2;
                continue;
            }
        }
        // markdown [text](url) → [text|url]
        if c == '[' {
            if let Some(bracket) = find_char(&ch, i + 1, ']') {
                if ch.get(bracket + 1) == Some(&'(') {
                    if let Some(paren) = find_char(&ch, bracket + 2, ')') {
                        let text: String = ch[i + 1..bracket].iter().collect();
                        let url: String = ch[bracket + 2..paren].iter().collect();
                        out.push_str(&format!("[{text}|{url}]"));
                        i = paren + 1;
                        continue;
                    }
                }
            }
        }
        // **bold** → *bold*
        if c == '*' && ch.get(i + 1) == Some(&'*') {
            if let Some(end) = find_pair(&ch, i + 2, ['*', '*']) {
                out.push('*');
                out.extend(&ch[i + 2..end]);
                out.push('*');
                i = end + 2;
                continue;
            }
        }
        // org /italic/ → _italic_, only at a word boundary and never across a
        // `:` so that `https://…` is left intact.
        let word_start = i == 0 || ch[i - 1].is_whitespace() || ch[i - 1] == '(';
        if c == '/' && word_start && ch.get(i + 1).is_some_and(|n| !n.is_whitespace() && *n != '/') {
            if let Some(end) = find_char(&ch, i + 1, '/') {
                let closes = ch
                    .get(end + 1)
                    .is_none_or(|n| n.is_whitespace() || n.is_ascii_punctuation());
                let clean = !ch[i + 1..end].contains(&':') && !ch[end - 1].is_whitespace();
                if closes && clean {
                    out.push('_');
                    out.extend(&ch[i + 1..end]);
                    out.push('_');
                    i = end + 1;
                    continue;
                }
            }
        }

        out.push(c);
        i += 1;
    }
    out
}

/// Confluence base URL and Basic auth header, from `$CONFLUENCE_URL` and
/// `$CONFLUENCE_AUTH` (`user:api-token`).
fn confluence_endpoint() -> Result<(String, String), String> {
    let url = env_required(
        "CONFLUENCE_URL",
        "set it to your wiki root, e.g. https://example.atlassian.net/wiki",
    )?;
    let auth = env_required(
        "CONFLUENCE_AUTH",
        "set it to `user:api-token`; create the token at https://id.atlassian.com/manage-profile/security/api-tokens",
    )?;
    Ok((
        url.trim().trim_end_matches('/').to_string(),
        basic_auth(auth.trim()),
    ))
}

/// `confluence-get-page` — one page's storage body, rendered to text.
/// `<space> <title>`.
pub fn confluence_page(args: &[&str]) -> Result<Outcome, String> {
    let space = args
        .first()
        .copied()
        .ok_or_else(|| "usage: confluence-page <space> <title…>".to_string())?;
    if args.len() < 2 {
        return Err("usage: confluence-page <space> <title…>".to_string());
    }
    let title = args[1..].join(" ");
    let (base, auth) = confluence_endpoint()?;
    let doc = sm::http_get_json(
        &format!(
            "{base}/rest/api/content?spaceKey={}&title={}&expand=body.storage",
            sm::urlencode(space),
            sm::urlencode(&title)
        ),
        &[("Authorization", auth.as_str())],
    )?;
    let page_json = doc
        .get("results")
        .and_then(Value::as_array)
        .and_then(|r| r.first())
        .ok_or_else(|| format!("confluence: no page titled `{title}` in space {space}"))?;
    let body = page_json
        .get("body")
        .and_then(|b| b.get("storage"))
        .map(|s| jstr(s, "value"))
        .unwrap_or("");

    let mut page = sm::heading(&title);
    page.push_str(&format!(
        "{base}/pages/viewpage.action?pageId={}\n\n",
        jstr(page_json, "id")
    ));
    page.push_str(&html_text(body));
    Ok(Outcome::page(
        format!("confluence: {space}/{title}"),
        page,
    ))
}

/// `confluence-search` — CQL full-text search, 25 results.
pub fn confluence_search(args: &[&str]) -> Result<Outcome, String> {
    if args.is_empty() {
        return Err("usage: confluence-search <query…>".to_string());
    }
    let query = args.join(" ");
    let (base, auth) = confluence_endpoint()?;
    let cql = format!("text~\"{}\"", query.replace('"', "\\\""));
    let doc = sm::http_get_json(
        &format!(
            "{base}/rest/api/content/search?cql={}&limit=25",
            sm::urlencode(&cql)
        ),
        &[("Authorization", auth.as_str())],
    )?;
    let results = doc
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| "confluence-search: no results field in reply".to_string())?;

    let mut page = sm::heading(&format!("Confluence — `{query}`"));
    for r in results {
        let space = r
            .get("space")
            .map(|s| jstr(s, "key").to_string())
            .unwrap_or_default();
        page.push_str(&format!(
            "{:>10}  {space:10}  {}\n",
            jstr(r, "id"),
            jstr(r, "title")
        ));
    }
    Ok(Outcome::page(
        format!("confluence-search: {} results", results.len()),
        page,
    ))
}

// ─────────────────────── evernote layer (geeknote CLI) ───────────────────────

/// Run a `geeknote` subcommand and page its output.
fn geeknote(sub: &str, args: &[&str], title: &str) -> Result<Outcome, String> {
    if !sm::have("geeknote") {
        return Err("`geeknote` not found on PATH".to_string());
    }
    let mut argv = vec![sub];
    argv.extend_from_slice(args);
    let out = sm::run("geeknote", &argv)?;
    let mut page = sm::heading(title);
    page.push_str(out.trim_end());
    page.push('\n');
    Ok(Outcome::page(title.to_string(), page))
}

/// `geeknote-find` — full-text search across notes.
pub fn geeknote_find(args: &[&str]) -> Result<Outcome, String> {
    if args.is_empty() {
        return Err("usage: geeknote-find <query…>".to_string());
    }
    let query = args.join(" ");
    geeknote("find", &["--search", &query], &format!("geeknote find `{query}`"))
}

/// `geeknote-show` — print one note by title.
pub fn geeknote_show(args: &[&str]) -> Result<Outcome, String> {
    if args.is_empty() {
        return Err("usage: geeknote-show <note title…>".to_string());
    }
    let note = args.join(" ");
    geeknote("show", &[&note], &format!("geeknote {note}"))
}

/// `geeknote-create` — a new note: first word-run before the content is the
/// title. `<title> <content…>` with the title taken as the first argument.
pub fn geeknote_create(args: &[&str]) -> Result<Outcome, String> {
    let title = args
        .first()
        .copied()
        .ok_or_else(|| "usage: geeknote-create <title> <content…>".to_string())?;
    let content = args[1..].join(" ");
    geeknote(
        "create",
        &["--title", title, "--content", &content],
        &format!("geeknote create `{title}`"),
    )
}

/// `geeknote-remove` — delete a note. `--force` is passed because the CLI
/// otherwise prompts on a terminal we do not own.
pub fn geeknote_remove(args: &[&str]) -> Result<Outcome, String> {
    if args.is_empty() {
        return Err("usage: geeknote-remove <note title…>".to_string());
    }
    let note = args.join(" ");
    geeknote(
        "remove",
        &["--note", &note, "--force"],
        &format!("geeknote remove `{note}`"),
    )
}

/// `geeknote-move` — move a note to another notebook, which geeknote spells as
/// an edit that only sets `--notebook`.
pub fn geeknote_move(args: &[&str]) -> Result<Outcome, String> {
    if args.len() < 2 {
        return Err("usage: geeknote-move <note> <notebook…>".to_string());
    }
    let note = args[0];
    let notebook = args[1..].join(" ");
    geeknote(
        "edit",
        &["--note", note, "--notebook", &notebook],
        &format!("geeknote move `{note}` → `{notebook}`"),
    )
}

/// `geeknote-notebook-list` — every notebook.
pub fn geeknote_notebooks(_args: &[&str]) -> Result<Outcome, String> {
    geeknote("notebook-list", &[], "geeknote notebooks")
}

// ───────────────────────────── twitter layer ─────────────────────────────
//
// `twittering-mode` spoke the v1.1 API, which is closed to free clients. The
// current public path is the v2 API with a bearer token, which is what these
// commands use. Note that on the free access tier v2 permits posting and
// looking up the authenticated user only; the timeline, user-tweet and recent
// search endpoints require a paid tier and answer 403 otherwise. That is an
// API-side policy, not a limitation of these commands.

/// The v2 bearer token every request carries.
fn twitter_bearer() -> Result<String, String> {
    let token = env_required(
        "TWITTER_BEARER_TOKEN",
        "create a project app at https://developer.twitter.com and export its bearer token",
    )?;
    Ok(format!("Bearer {}", token.trim()))
}

/// Render a v2 tweet payload (`data` plus `includes.users`) as text.
fn twitter_render(doc: &Value, title: &str) -> (usize, String) {
    let users: Vec<&Value> = doc
        .get("includes")
        .and_then(|i| i.get("users"))
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let username_of = |id: &str| -> String {
        users
            .iter()
            .find(|u| jstr(u, "id") == id)
            .map(|u| jstr(u, "username").to_string())
            .unwrap_or_else(|| id.to_string())
    };

    let tweets: Vec<&Value> = match doc.get("data") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(v) => vec![v],
        None => vec![],
    };
    let mut page = sm::heading(title);
    for t in &tweets {
        let metrics = t.get("public_metrics");
        let likes = metrics.map(|m| jnum(m, "like_count")).unwrap_or(0);
        let retweets = metrics.map(|m| jnum(m, "retweet_count")).unwrap_or(0);
        page.push_str(&format!(
            "@{}  {}\n{}\n{likes}\u{2665} {retweets}\u{21ba}\n\n",
            username_of(jstr(t, "author_id")),
            jstr(t, "created_at"),
            jstr(t, "text"),
        ));
    }
    (tweets.len(), page)
}

/// Fields every tweet query asks for, so all three renderers see one shape.
const TWITTER_FIELDS: &str =
    "max_results=25&tweet.fields=created_at,public_metrics&expansions=author_id&user.fields=username";

/// `twitter-timeline` — the authenticated user's reverse-chronological home
/// timeline. Requires an OAuth 2.0 user-context token; an app-only bearer is
/// rejected by `users/me`.
pub fn twitter_timeline(_args: &[&str]) -> Result<Outcome, String> {
    let auth = twitter_bearer()?;
    let headers = [("Authorization", auth.as_str())];
    let me = sm::http_get_json("https://api.twitter.com/2/users/me", &headers)?;
    let id = me
        .get("data")
        .map(|d| jstr(d, "id").to_string())
        .filter(|id| !id.is_empty())
        .ok_or_else(|| "twitter: users/me returned no id".to_string())?;
    let doc = sm::http_get_json(
        &format!(
            "https://api.twitter.com/2/users/{id}/timelines/reverse_chronological?{TWITTER_FIELDS}"
        ),
        &headers,
    )?;
    let (n, page) = twitter_render(&doc, "Twitter — home timeline");
    Ok(Outcome::page(format!("twitter: {n} tweets"), page))
}

/// `twitter-user` — one handle's recent tweets.
pub fn twitter_user(args: &[&str]) -> Result<Outcome, String> {
    let handle = args
        .first()
        .copied()
        .ok_or_else(|| "usage: twitter-user <handle>".to_string())?
        .trim_start_matches('@');
    let auth = twitter_bearer()?;
    let headers = [("Authorization", auth.as_str())];
    let user = sm::http_get_json(
        &format!("https://api.twitter.com/2/users/by/username/{handle}"),
        &headers,
    )?;
    let id = user
        .get("data")
        .map(|d| jstr(d, "id").to_string())
        .filter(|id| !id.is_empty())
        .ok_or_else(|| format!("twitter: no user @{handle}"))?;
    let doc = sm::http_get_json(
        &format!("https://api.twitter.com/2/users/{id}/tweets?{TWITTER_FIELDS}"),
        &headers,
    )?;
    let (n, page) = twitter_render(&doc, &format!("Twitter — @{handle}"));
    Ok(Outcome::page(format!("twitter @{handle}: {n} tweets"), page))
}

/// `twitter-search` — the recent-search endpoint (last 7 days).
pub fn twitter_search(args: &[&str]) -> Result<Outcome, String> {
    if args.is_empty() {
        return Err("usage: twitter-search <query…>".to_string());
    }
    let query = args.join(" ");
    let auth = twitter_bearer()?;
    let doc = sm::http_get_json(
        &format!(
            "https://api.twitter.com/2/tweets/search/recent?query={}&{TWITTER_FIELDS}",
            sm::urlencode(&query)
        ),
        &[("Authorization", auth.as_str())],
    )?;
    let (n, page) = twitter_render(&doc, &format!("Twitter — `{query}`"));
    Ok(Outcome::page(format!("twitter-search: {n} tweets"), page))
}

/// `twitter-post` — publish a tweet.
pub fn twitter_post(args: &[&str]) -> Result<Outcome, String> {
    if args.is_empty() {
        return Err("usage: twitter-post <text…>".to_string());
    }
    let text = args.join(" ");
    let auth = twitter_bearer()?;
    let doc = sm::http_post_json(
        "https://api.twitter.com/2/tweets",
        &[("Authorization", auth.as_str())],
        &serde_json::json!({ "text": text }),
    )?;
    let id = doc.get("data").map(|d| jstr(d, "id").to_string()).unwrap_or_default();
    Ok(Outcome::status(format!("twitter: posted {id}")))
}

// ───────────────────────── whisper layer (whisper.cpp) ─────────────────────────
//
// The emacs `whisper.el` package records or takes an audio file, normalises it
// with ffmpeg and runs a local whisper.cpp binary; nothing leaves the machine.
// The flags below are whisper.cpp's (`-m`, `-f`, `-nt`, `-l`).

/// Locate the whisper.cpp binary: the two names current builds install, then
/// `main` inside `$WHISPER_CPP_DIR` (what an in-tree build produces), then a
/// plain `whisper` on PATH.
fn whisper_binary() -> Result<String, String> {
    for name in ["whisper-cli", "whisper-cpp"] {
        if sm::have(name) {
            return Ok(name.to_string());
        }
    }
    if let Some(dir) = env_opt("WHISPER_CPP_DIR") {
        for candidate in ["build/bin/whisper-cli", "whisper-cli", "main"] {
            let path = Path::new(&dir).join(candidate);
            if path.is_file() {
                return Ok(path.to_string_lossy().into_owned());
            }
        }
    }
    if sm::have("whisper") {
        return Ok("whisper".to_string());
    }
    Err("no whisper binary found — install whisper.cpp (`whisper-cli`) or set $WHISPER_CPP_DIR"
        .to_string())
}

/// The model file: `$WHISPER_MODEL`, else the base English model under
/// `$WHISPER_CPP_DIR/models`.
fn whisper_model_path() -> Result<PathBuf, String> {
    if let Some(model) = env_opt("WHISPER_MODEL") {
        return Ok(PathBuf::from(model));
    }
    let dir = env_opt("WHISPER_CPP_DIR")
        .ok_or_else(|| "$WHISPER_MODEL is unset and $WHISPER_CPP_DIR gives no default".to_string())?;
    let path = Path::new(&dir).join("models").join("ggml-base.en.bin");
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!(
            "no model at {} — set $WHISPER_MODEL",
            path.display()
        ))
    }
}

/// Normalise `input` to the 16 kHz mono 16-bit wav whisper.cpp requires, unless
/// it already is a wav. Returns the path to feed the binary.
fn whisper_prepare_wav(input: &Path) -> Result<PathBuf, String> {
    let is_wav = input
        .extension()
        .map(|e| e.eq_ignore_ascii_case("wav"))
        .unwrap_or(false);
    if is_wav {
        return Ok(input.to_path_buf());
    }
    if !sm::have("ffmpeg") {
        return Err("`ffmpeg` not found on PATH — it converts the input to 16 kHz mono wav".to_string());
    }
    let out = std::env::temp_dir().join(format!("zmax-whisper-{}.wav", std::process::id()));
    let out_str = out.to_string_lossy().into_owned();
    sm::run(
        "ffmpeg",
        &[
            "-y",
            "-i",
            &input.to_string_lossy(),
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            &out_str,
        ],
    )?;
    Ok(out)
}

/// Run whisper.cpp over `wav` and return the transcript.
fn whisper_transcribe(wav: &Path, language: Option<&str>) -> Result<String, String> {
    let binary = whisper_binary()?;
    let model = whisper_model_path()?;
    let lang = language
        .map(str::to_string)
        .or_else(|| env_opt("WHISPER_LANGUAGE"))
        .unwrap_or_else(|| "en".to_string());
    let out = sm::run(
        &binary,
        &[
            "-m",
            &model.to_string_lossy(),
            "-f",
            &wav.to_string_lossy(),
            "-nt",
            "-l",
            &lang,
        ],
    )?;
    Ok(out.trim().to_string())
}

/// `whisper-file` — transcribe an audio file locally. `<audio-file> [language]`.
pub fn whisper_file(args: &[&str]) -> Result<Outcome, String> {
    let file = args
        .first()
        .copied()
        .ok_or_else(|| "usage: whisper-file <audio-file> [language]".to_string())?;
    let input = PathBuf::from(file);
    if !input.is_file() {
        return Err(format!("whisper-file: {} does not exist", input.display()));
    }
    let wav = whisper_prepare_wav(&input)?;
    let text = whisper_transcribe(&wav, args.get(1).copied())?;
    let mut page = sm::heading(&format!("whisper — {}", input.display()));
    page.push_str(&text);
    page.push('\n');
    Ok(Outcome::page(
        format!("whisper: {} chars transcribed", text.len()),
        page,
    ))
}

/// `whisper-model` — with an argument, set the model for this process; without
/// one, list the `.bin` models under `$WHISPER_CPP_DIR/models`. The setting is
/// process-local, matching the emacs variable's session scope.
pub fn whisper_model(args: &[&str]) -> Result<Outcome, String> {
    if let Some(model) = args.first() {
        std::env::set_var("WHISPER_MODEL", model);
        return Ok(Outcome::status(format!("whisper model: {model}")));
    }
    let dir = env_opt("WHISPER_CPP_DIR")
        .ok_or_else(|| "$WHISPER_CPP_DIR is unset — cannot list models".to_string())?;
    let models = Path::new(&dir).join("models");
    let entries = std::fs::read_dir(&models).map_err(|e| format!("{}: {e}", models.display()))?;
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".bin"))
        .collect();
    names.sort();
    let mut page = sm::heading(&format!("whisper models — {}", models.display()));
    for name in &names {
        page.push_str(name);
        page.push('\n');
    }
    Ok(Outcome::page(
        format!(
            "whisper: {} models, current {}",
            names.len(),
            env_opt("WHISPER_MODEL").unwrap_or_else(|| "<default>".to_string())
        ),
        page,
    ))
}

/// `whisper-language` — set or report the transcription language for this
/// process.
pub fn whisper_language(args: &[&str]) -> Result<Outcome, String> {
    match args.first() {
        Some(lang) => {
            std::env::set_var("WHISPER_LANGUAGE", lang);
            Ok(Outcome::status(format!("whisper language: {lang}")))
        }
        None => Ok(Outcome::status(format!(
            "whisper language: {}",
            env_opt("WHISPER_LANGUAGE").unwrap_or_else(|| "en (default)".to_string())
        ))),
    }
}

/// `whisper-run` — record from the default input for `[seconds]` (default 10)
/// and transcribe the recording. The capture device is ffmpeg's
/// `avfoundation :0` on macOS and `alsa default` elsewhere, the same pair
/// whisper.el picks between.
pub fn whisper_record(args: &[&str]) -> Result<Outcome, String> {
    let seconds = match args.first() {
        Some(s) => s
            .parse::<u32>()
            .map_err(|_| format!("whisper-record: `{s}` is not a number of seconds"))?,
        None => 10,
    };
    if !sm::have("ffmpeg") {
        return Err("`ffmpeg` not found on PATH — it drives the recording".to_string());
    }
    let out = std::env::temp_dir().join(format!("zmax-whisper-rec-{}.wav", std::process::id()));
    let out_str = out.to_string_lossy().into_owned();
    let secs = seconds.to_string();
    let (format, device) = if cfg!(target_os = "macos") {
        ("avfoundation", ":0")
    } else {
        ("alsa", "default")
    };
    sm::run(
        "ffmpeg",
        &[
            "-y", "-f", format, "-i", device, "-t", &secs, "-ar", "16000", "-ac", "1", "-c:a",
            "pcm_s16le", &out_str,
        ],
    )?;
    let text = whisper_transcribe(&out, args.get(1).copied())?;
    let mut page = sm::heading(&format!("whisper — {seconds}s recording"));
    page.push_str(&text);
    page.push('\n');
    Ok(Outcome::page(
        format!("whisper: recorded {seconds}s, {} chars", text.len()),
        page,
    ))
}

// ───────────────────────────── xkcd layer ─────────────────────────────
//
// `xkcd.el` fetches the JSON metadata, downloads the image and shows it in a
// buffer with the alt text. Here the image is cached on disk and its path comes
// back on the status line for the caller to render; the page carries the title,
// date, alt text and transcript.

/// Comic number most recently shown, so `xkcd-next` / `xkcd-prev` can step from
/// it. Zero means nothing has been shown yet.
static LAST_XKCD: AtomicU32 = AtomicU32::new(0);

/// Metadata URL for a comic number, or for the latest comic when `None`.
fn xkcd_info_url(num: Option<u32>) -> String {
    match num {
        Some(n) => format!("https://xkcd.com/{n}/info.0.json"),
        None => "https://xkcd.com/info.0.json".to_string(),
    }
}

/// Download `url` to `dest` with whichever of curl/wget exists — the comic image
/// is binary, so it does not go through the text HTTP helpers.
fn download(url: &str, dest: &Path) -> Result<(), String> {
    let dest = dest.to_string_lossy().into_owned();
    if sm::have("curl") {
        sm::run("curl", &["-fsSL", "-o", &dest, url]).map(|_| ())
    } else if sm::have("wget") {
        sm::run("wget", &["-q", "-O", &dest, url]).map(|_| ())
    } else {
        Err("neither `curl` nor `wget` is on PATH — one of them is needed to download the comic image".to_string())
    }
}

/// Fetch a comic's metadata, cache its image and build the outcome.
fn xkcd_show(num: Option<u32>) -> Result<Outcome, String> {
    let doc = sm::http_get_json(&xkcd_info_url(num), &[])?;
    let n = doc.get("num").and_then(Value::as_u64).unwrap_or(0) as u32;
    let img = jstr(&doc, "img");
    if img.is_empty() {
        return Err("xkcd: reply had no image url".to_string());
    }
    let dir = home()?.join(".cache").join("zmax").join("xkcd");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{n}.png"));
    if !path.is_file() {
        download(img, &path)?;
    }
    LAST_XKCD.store(n, Ordering::Relaxed);

    let page = format!(
        "xkcd #{n}: {}\n{}-{:0>2}-{:0>2}\n\n{}\n\n{}\n",
        jstr(&doc, "title"),
        jstr(&doc, "year"),
        jstr(&doc, "month"),
        jstr(&doc, "day"),
        jstr(&doc, "alt"),
        jstr(&doc, "transcript"),
    );
    Ok(Outcome::page(path.to_string_lossy().into_owned(), page))
}

/// The latest comic's number.
fn xkcd_latest() -> Result<u32, String> {
    let doc = sm::http_get_json(&xkcd_info_url(None), &[])?;
    doc.get("num")
        .and_then(Value::as_u64)
        .map(|n| n as u32)
        .ok_or_else(|| "xkcd: latest reply had no num".to_string())
}

/// `xkcd` — a comic by number, or the latest one.
pub fn xkcd(args: &[&str]) -> Result<Outcome, String> {
    let num = match args.first() {
        Some(n) => Some(
            n.parse::<u32>()
                .map_err(|_| format!("xkcd: `{n}` is not a comic number"))?,
        ),
        None => None,
    };
    xkcd_show(num)
}

/// `xkcd-rand` — a pseudo-random comic. The seed is the wall-clock nanosecond
/// count, which avoids a random-number dependency for a command whose only
/// requirement is that consecutive calls differ.
pub fn xkcd_random(_args: &[&str]) -> Result<Outcome, String> {
    let latest = xkcd_latest()?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(1);
    // Mix the low bits so consecutive calls within the same second differ.
    let mixed = nanos.wrapping_mul(6364136223846793005).rotate_left(17);
    let pick = (mixed % latest.max(1) as u64) as u32 + 1;
    xkcd_show(Some(pick.min(latest)))
}

/// `xkcd-next` — the comic after the one last shown, clamped to the latest.
pub fn xkcd_next(_args: &[&str]) -> Result<Outcome, String> {
    let latest = xkcd_latest()?;
    let last = LAST_XKCD.load(Ordering::Relaxed);
    let next = if last == 0 { latest } else { (last + 1).min(latest) };
    xkcd_show(Some(next))
}

/// `xkcd-prev` — the comic before the one last shown, clamped to #1.
pub fn xkcd_prev(_args: &[&str]) -> Result<Outcome, String> {
    let last = LAST_XKCD.load(Ordering::Relaxed);
    let prev = if last <= 1 { xkcd_latest()? } else { last - 1 };
    xkcd_show(Some(prev.max(1)))
}

/// The comic number a browse command should act on: the argument, else the one
/// last shown, else the latest.
fn xkcd_target(args: &[&str]) -> Result<u32, String> {
    match args.first() {
        Some(n) => n
            .parse::<u32>()
            .map_err(|_| format!("xkcd: `{n}` is not a comic number")),
        None => match LAST_XKCD.load(Ordering::Relaxed) {
            0 => xkcd_latest(),
            n => Ok(n),
        },
    }
}

/// `xkcd-open-browser` — the comic's page URL for the caller to open.
pub fn xkcd_open(args: &[&str]) -> Result<Outcome, String> {
    Ok(Outcome::status(format!(
        "https://xkcd.com/{}/",
        xkcd_target(args)?
    )))
}

/// `xkcd-open-explanation-browser` — the explainxkcd URL for the caller to open.
pub fn xkcd_explain(args: &[&str]) -> Result<Outcome, String> {
    Ok(Outcome::status(format!(
        "https://www.explainxkcd.com/wiki/index.php/{}",
        xkcd_target(args)?
    )))
}

// ───────────────────────────── elfeed layer ─────────────────────────────
//
// `elfeed` reads a list of feed URLs with optional tags and shows their entries
// newest first. Both RSS 2.0 and Atom appear in any real feed list, so the
// scanner below handles both; it is a tag scanner rather than a parser, which is
// the same tradeoff the eww renderer makes and keeps the dependency set at zero.

/// Example written to `~/.config/zmax/elfeed-feeds` on first use.
const ELFEED_EXAMPLE: &str = "\
# elfeed-feeds: one feed URL per line, optional space-separated tags after it.
# Lines starting with # are ignored.
#https://blog.rust-lang.org/feed.xml rust official
#https://lobste.rs/rss news
";

/// One parsed feed entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedEntry {
    pub feed: String,
    pub title: String,
    pub link: String,
    pub date: String,
}

/// Inner text of every `<name>…</name>` element in `xml`. Matching is on the
/// exact element name, so `<item>` does not also match `<items>`.
fn elements<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        let boundary = after.starts_with('>') || after.starts_with(|c: char| c.is_whitespace());
        if !boundary {
            rest = after;
            continue;
        }
        let Some(gt) = after.find('>') else { break };
        let body = &after[gt + 1..];
        let Some(end) = body.find(&close) else { break };
        out.push(&body[..end]);
        rest = &body[end + close.len()..];
    }
    out
}

/// Text of the first `<name>` element in `block`, with CDATA sections unwrapped,
/// tags removed and entities decoded.
fn element_text(block: &str, name: &str) -> String {
    let Some(raw) = elements(block, name).into_iter().next() else {
        return String::new();
    };
    let raw = raw.trim();
    let inner = raw
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
        .unwrap_or(raw);
    squeeze_blank_lines(&unescape_entities(&strip_tags(inner)))
}

/// Value of `attr` on the first `<name …>` tag in `block` that carries it.
fn element_attr(block: &str, name: &str, attr: &str) -> Option<String> {
    let open = format!("<{name}");
    let mut rest = block;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        let gt = after.find('>')?;
        let tag = &after[..gt];
        // Skip Atom's `rel="self"` link, which points at the feed itself.
        if !tag.contains("rel=\"self\"") {
            if let Some(pos) = tag.find(&format!("{attr}=")) {
                let value = &tag[pos + attr.len() + 1..];
                let quote = value.chars().next()?;
                if quote == '"' || quote == '\'' {
                    let end = value[1..].find(quote)? + 1;
                    return Some(unescape_entities(&value[1..end]));
                }
            }
        }
        rest = &after[gt + 1..];
    }
    None
}

/// The feed's own title: the first `<title>` appearing before the first entry.
fn feed_title(xml: &str) -> String {
    let cut = xml
        .find("<item")
        .into_iter()
        .chain(xml.find("<entry"))
        .min()
        .unwrap_or(xml.len());
    let head = &xml[..cut];
    let title = element_text(head, "title");
    if title.is_empty() {
        "feed".to_string()
    } else {
        title
    }
}

/// Parse an RSS 2.0 or Atom document into `(feed title, entries)`.
pub fn parse_feed(xml: &str) -> (String, Vec<FeedEntry>) {
    let feed = feed_title(xml);
    let mut entries = Vec::new();

    for item in elements(xml, "item") {
        entries.push(FeedEntry {
            feed: feed.clone(),
            title: element_text(item, "title"),
            link: element_text(item, "link"),
            date: element_text(item, "pubDate"),
        });
    }
    for entry in elements(xml, "entry") {
        let link = element_attr(entry, "link", "href").unwrap_or_else(|| element_text(entry, "link"));
        let date = {
            let updated = element_text(entry, "updated");
            if updated.is_empty() {
                element_text(entry, "published")
            } else {
                updated
            }
        };
        entries.push(FeedEntry {
            feed: feed.clone(),
            title: element_text(entry, "title"),
            link,
            date,
        });
    }
    (feed, entries)
}

/// Turn a feed date into a sortable `YYYYMMDDhhmmss` number, accepting both the
/// ISO 8601 stamps Atom uses and the RFC 822 stamps RSS uses. An unparseable
/// date sorts last (zero).
fn date_key(s: &str) -> u64 {
    fn num(s: &str) -> u64 {
        s.trim().parse().unwrap_or(0)
    }
    fn stamp(y: u64, mo: u64, d: u64, h: u64, mi: u64, sec: u64) -> u64 {
        ((((y * 100 + mo) * 100 + d) * 100 + h) * 100 + mi) * 100 + sec
    }

    let t = s.trim();
    let b = t.as_bytes();
    // ISO 8601: 2025-06-10T12:00:00Z
    if b.len() >= 10 && b[4] == b'-' && b[7] == b'-' {
        let (h, mi, sec) = if b.len() >= 19 && (b[10] == b'T' || b[10] == b' ') {
            (num(&t[11..13]), num(&t[14..16]), num(&t[17..19]))
        } else {
            (0, 0, 0)
        };
        return stamp(num(&t[0..4]), num(&t[5..7]), num(&t[8..10]), h, mi, sec);
    }
    // RFC 822: Tue, 10 Jun 2025 12:00:00 GMT (weekday optional)
    const MONTHS: &[&str] = &[
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let parts: Vec<&str> = t.split_whitespace().collect();
    let parts = if parts.first().is_some_and(|p| p.ends_with(',')) {
        &parts[1..]
    } else {
        &parts[..]
    };
    if parts.len() >= 3 {
        let day = num(parts[0]);
        let month = MONTHS
            .iter()
            .position(|m| parts[1].to_ascii_lowercase().starts_with(m))
            .map(|i| i as u64 + 1)
            .unwrap_or(0);
        let year = num(parts[2]);
        let (h, mi, sec) = match parts.get(3) {
            Some(time) => {
                let hms: Vec<&str> = time.split(':').collect();
                (
                    hms.first().map(|v| num(v)).unwrap_or(0),
                    hms.get(1).map(|v| num(v)).unwrap_or(0),
                    hms.get(2).map(|v| num(v)).unwrap_or(0),
                )
            }
            None => (0, 0, 0),
        };
        if month > 0 && year > 0 {
            return stamp(year, month, day, h, mi, sec);
        }
    }
    0
}

/// Read the feed list as `(url, tags)` pairs.
fn elfeed_config() -> Result<Vec<(String, Vec<String>)>, String> {
    let (_, lines) = read_list_config("elfeed-feeds", ELFEED_EXAMPLE)?;
    Ok(lines
        .into_iter()
        .map(|line| {
            let mut words = line.split_whitespace();
            let url = words.next().unwrap_or("").to_string();
            (url, words.map(str::to_string).collect())
        })
        .filter(|(url, _)| !url.is_empty())
        .collect())
}

/// `elfeed` — every configured feed's entries, newest first. An optional
/// argument filters by an exact tag or by a case-insensitive substring of the
/// entry title. A feed that fails to fetch or parse is reported in the page
/// rather than aborting the whole run.
pub fn elfeed(args: &[&str]) -> Result<Outcome, String> {
    let feeds = elfeed_config()?;
    if feeds.is_empty() {
        return Err("elfeed: no feeds configured".to_string());
    }
    let filter = args.first().map(|f| f.to_ascii_lowercase());
    // The argument is read as a tag when some configured feed carries it, and
    // as a title substring otherwise — the two ways elfeed's filter box is used.
    let by_tag = filter.as_ref().is_some_and(|f| {
        feeds
            .iter()
            .any(|(_, tags)| tags.iter().any(|t| t.eq_ignore_ascii_case(f)))
    });

    let mut failures = Vec::new();
    let mut all: Vec<(u64, FeedEntry)> = Vec::new();
    for (url, tags) in &feeds {
        if by_tag {
            let want = filter.as_deref().unwrap_or_default();
            if !tags.iter().any(|t| t.eq_ignore_ascii_case(want)) {
                continue;
            }
        }
        match sm::http_get(url, &[]) {
            Ok(body) => {
                let (_, entries) = parse_feed(&body);
                for e in entries {
                    all.push((date_key(&e.date), e));
                }
            }
            Err(e) => failures.push(format!("{url}: {e}")),
        }
    }
    if let Some(f) = filter.as_deref().filter(|_| !by_tag) {
        all.retain(|(_, e)| e.title.to_ascii_lowercase().contains(f));
    }
    all.sort_by_key(|(key, _)| std::cmp::Reverse(*key));
    all.truncate(200);

    let mut page = sm::heading(&match &filter {
        Some(f) => format!("elfeed — {f}"),
        None => "elfeed".to_string(),
    });
    for (_, e) in &all {
        page.push_str(&format!(
            "[{}] {}\n    {}  {}\n",
            e.feed, e.title, e.date, e.link
        ));
    }
    for failure in &failures {
        page.push_str(&format!("\n<failed: {failure}>\n"));
    }
    Ok(Outcome::page(
        format!("elfeed: {} entries from {} feeds", all.len(), feeds.len()),
        page,
    ))
}

/// `elfeed-add-feed` — append `<url> [tags…]` to the feed list.
pub fn elfeed_add(args: &[&str]) -> Result<Outcome, String> {
    use std::io::Write;

    let url = args
        .first()
        .copied()
        .ok_or_else(|| "usage: elfeed-add <url> [tags…]".to_string())?;
    let path = config_file("elfeed-feeds")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let line = if args.len() > 1 {
        format!("{url} {}\n", args[1..].join(" "))
    } else {
        format!("{url}\n")
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(line.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Outcome::status(format!(
        "elfeed: added {url} to {}",
        path.display()
    )))
}

/// `elfeed-feeds` — the configured feeds and their tags.
pub fn elfeed_feeds(_args: &[&str]) -> Result<Outcome, String> {
    let feeds = elfeed_config()?;
    let mut page = sm::heading("elfeed feeds");
    for (url, tags) in &feeds {
        page.push_str(&format!("{url}  [{}]\n", tags.join(" ")));
    }
    Ok(Outcome::page(format!("elfeed: {} feeds", feeds.len()), page))
}

/// `elfeed-show-entry` — fetch one entry's page and show it as text.
pub fn elfeed_show(args: &[&str]) -> Result<Outcome, String> {
    let url = args
        .first()
        .copied()
        .ok_or_else(|| "usage: elfeed-show <url>".to_string())?;
    let body = sm::http_get(url, &[])?;
    let mut page = sm::heading(url);
    page.push_str(&html_text(&body));
    page.push('\n');
    Ok(Outcome::page(format!("elfeed-show: {url}"), page))
}

// ─────────────────────────── geolocation layer ───────────────────────────

/// The location a weather command should use: the arguments, else
/// `$WEATHER_LOCATION`.
fn weather_location(args: &[&str]) -> Result<String, String> {
    if !args.is_empty() {
        return Ok(args.join(" "));
    }
    env_required("WEATHER_LOCATION", "or pass the location as an argument")
}

/// OpenWeatherMap unit system and its temperature suffix.
fn weather_units() -> (String, &'static str) {
    match env_opt("WEATHER_UNITS").as_deref() {
        Some("imperial") => ("imperial".to_string(), "°F"),
        Some("standard") => ("standard".to_string(), "K"),
        _ => ("metric".to_string(), "°C"),
    }
}

/// Fetch the OpenWeatherMap 5-day/3-hour forecast.
fn owm_forecast(location: &str, key: &str, units: &str) -> Result<Value, String> {
    sm::http_get_json(
        &format!(
            "https://api.openweathermap.org/data/2.5/forecast?q={}&appid={}&units={units}",
            sm::urlencode(location),
            sm::urlencode(key)
        ),
        &[],
    )
}

/// Fetch the wttr.in JSON view, which needs no API key.
fn wttr(location: &str) -> Result<Value, String> {
    sm::http_get_json(
        &format!("https://wttr.in/{}?format=j1", sm::urlencode(location)),
        &[],
    )
}

/// `weather` — a 5-day forecast. Uses OpenWeatherMap when
/// `$OPENWEATHER_API_KEY` is set, otherwise the keyless wttr.in JSON view; the
/// status line says which source answered.
pub fn weather(args: &[&str]) -> Result<Outcome, String> {
    let location = weather_location(args)?;
    let (units, degree) = weather_units();

    if let Some(key) = env_opt("OPENWEATHER_API_KEY") {
        let doc = owm_forecast(&location, key.trim(), &units)?;
        let city = doc
            .get("city")
            .map(|c| jstr(c, "name").to_string())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| location.clone());
        let list = doc
            .get("list")
            .and_then(Value::as_array)
            .ok_or_else(|| "weather: no forecast list in reply".to_string())?;

        let mut page = sm::heading(&format!("Weather — {city} (openweathermap)"));
        let mut day = String::new();
        for slot in list {
            let stamp = jstr(slot, "dt_txt");
            let (date, time) = stamp.split_once(' ').unwrap_or((stamp, ""));
            if date != day {
                page.push_str(&format!("\n{date}\n"));
                day = date.to_string();
            }
            let temp = slot
                .get("main")
                .and_then(|m| m.get("temp"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let desc = slot
                .get("weather")
                .and_then(Value::as_array)
                .and_then(|w| w.first())
                .map(|w| jstr(w, "description").to_string())
                .unwrap_or_default();
            page.push_str(&format!("  {:5}  {temp:6.1}{degree}  {desc}\n", &time[..5.min(time.len())]));
        }
        return Ok(Outcome::page(
            format!("weather {city}: {} slots (openweathermap)", list.len()),
            page,
        ));
    }

    let doc = wttr(&location)?;
    let area = doc
        .get("nearest_area")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|a| a.get("areaName"))
        .and_then(Value::as_array)
        .and_then(|n| n.first())
        .map(|n| jstr(n, "value").to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| location.clone());
    let days = doc
        .get("weather")
        .and_then(Value::as_array)
        .ok_or_else(|| "weather: no daily forecast in wttr.in reply".to_string())?;

    let imperial = units == "imperial";
    let mut page = sm::heading(&format!("Weather — {area} (wttr.in)"));
    for d in days {
        page.push_str(&format!(
            "\n{}  {}–{}{degree}\n",
            jstr(d, "date"),
            jstr(d, if imperial { "mintempF" } else { "mintempC" }),
            jstr(d, if imperial { "maxtempF" } else { "maxtempC" }),
        ));
        let hourly = d.get("hourly").and_then(Value::as_array);
        for hour in hourly.into_iter().flatten() {
            // wttr.in writes the hour as `0`, `300`, `600`, … local time.
            let raw = jstr(hour, "time");
            let minutes: u32 = raw.parse().unwrap_or(0);
            let desc = hour
                .get("weatherDesc")
                .and_then(Value::as_array)
                .and_then(|w| w.first())
                .map(|w| jstr(w, "value").trim().to_string())
                .unwrap_or_default();
            page.push_str(&format!(
                "  {:02}:00  {:>4}{degree}  {desc}\n",
                minutes / 100,
                jstr(hour, if imperial { "tempF" } else { "tempC" }),
            ));
        }
    }
    Ok(Outcome::page(
        format!("weather {area}: {} days (wttr.in)", days.len()),
        page,
    ))
}

/// `weather-quick` — the current conditions on the status line only, from the
/// same two sources as [`weather`].
pub fn weather_quick(args: &[&str]) -> Result<Outcome, String> {
    let location = weather_location(args)?;
    let (units, degree) = weather_units();

    if let Some(key) = env_opt("OPENWEATHER_API_KEY") {
        let doc = owm_forecast(&location, key.trim(), &units)?;
        let city = doc
            .get("city")
            .map(|c| jstr(c, "name").to_string())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| location.clone());
        let now = doc
            .get("list")
            .and_then(Value::as_array)
            .and_then(|l| l.first())
            .ok_or_else(|| "weather-quick: empty forecast".to_string())?;
        let temp = now
            .get("main")
            .and_then(|m| m.get("temp"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let desc = now
            .get("weather")
            .and_then(Value::as_array)
            .and_then(|w| w.first())
            .map(|w| jstr(w, "description").to_string())
            .unwrap_or_default();
        return Ok(Outcome::status(format!("{city}: {temp:.1}{degree} {desc}")));
    }

    let doc = wttr(&location)?;
    let current = doc
        .get("current_condition")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .ok_or_else(|| "weather-quick: no current condition in wttr.in reply".to_string())?;
    let temp = jstr(
        current,
        if units == "imperial" { "temp_F" } else { "temp_C" },
    );
    let desc = current
        .get("weatherDesc")
        .and_then(Value::as_array)
        .and_then(|w| w.first())
        .map(|w| jstr(w, "value").trim().to_string())
        .unwrap_or_default();
    Ok(Outcome::status(format!("{location}: {temp}{degree} {desc}")))
}

/// `sun-times` — sunrise, sunset, solar noon and day length for a coordinate.
/// Latitude and longitude come from the arguments or from
/// `$CALENDAR_LATITUDE` / `$CALENDAR_LONGITUDE`, the two variables the emacs
/// geolocation layer sets. The API returns UTC timestamps with
/// `formatted=0`; they are shown as returned, not converted to local time.
pub fn sun_times(args: &[&str]) -> Result<Outcome, String> {
    let lat = match args.first() {
        Some(v) => v.to_string(),
        None => env_required("CALENDAR_LATITUDE", "or pass <lat> <lng> as arguments")?,
    };
    let lng = match args.get(1) {
        Some(v) => v.to_string(),
        None => env_required("CALENDAR_LONGITUDE", "or pass <lat> <lng> as arguments")?,
    };
    let doc = sm::http_get_json(
        &format!(
            "https://api.sunrise-sunset.org/json?lat={}&lng={}&formatted=0",
            sm::urlencode(lat.trim()),
            sm::urlencode(lng.trim())
        ),
        &[],
    )?;
    let r = doc
        .get("results")
        .ok_or_else(|| "sun-times: no results in reply".to_string())?;
    let day_length = r
        .get("day_length")
        .and_then(Value::as_f64)
        .map(hms)
        .unwrap_or_else(|| jstr(r, "day_length").to_string());

    let mut page = sm::heading(&format!("Sun times — {lat}, {lng} (UTC)"));
    for (label, key) in [
        ("sunrise", "sunrise"),
        ("solar noon", "solar_noon"),
        ("sunset", "sunset"),
        ("civil twilight begin", "civil_twilight_begin"),
        ("civil twilight end", "civil_twilight_end"),
    ] {
        page.push_str(&format!("{label:22}  {}\n", jstr(r, key)));
    }
    page.push_str(&format!("{:22}  {day_length}\n", "day length"));
    Ok(Outcome::page(
        format!("sun-times {lat},{lng}: day length {day_length}"),
        page,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_text_breaks_paragraphs_and_decodes_entities() {
        let src = "<p>a &amp; b<i>!</i></p><p>x&#x27;y &lt;tag&gt;</p>";
        assert_eq!(html_text(src), "a & b!\n\nx'y <tag>");
        // A stray `<` that opens no tag survives as text.
        assert_eq!(html_text("2 < 3"), "2 < 3");
        // `&amp;lt;` decodes once, to the literal `&lt;`.
        assert_eq!(html_text("&amp;lt;"), "&lt;");
        // `<br>` is a single line break, `<p>` a blank line.
        assert_eq!(html_text("a<br>b"), "a\nb");
    }

    #[test]
    fn strip_tags_removes_markup_but_not_entities() {
        assert_eq!(strip_tags("<b>bold</b> &amp; <a href=\"x\">link</a>"), "bold &amp; link");
        assert_eq!(strip_tags("<li>one</li><li>two</li>").trim(), "one\n\n\n\ntwo");
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"user:api-token"), "dXNlcjphcGktdG9rZW4=");
        assert_eq!(basic_auth("user:tok"), "Basic dXNlcjp0b2s=");
    }

    #[test]
    fn parses_rss_items() {
        let xml = r#"<rss><channel><title>Example Blog</title>
            <item><title>First &amp; Best</title><link>https://e.com/1</link>
              <pubDate>Tue, 10 Jun 2025 12:00:00 GMT</pubDate>
              <description><![CDATA[<p>hello</p>]]></description></item>
            <item><title>Second</title><link>https://e.com/2</link>
              <pubDate>Wed, 11 Jun 2025 09:30:00 GMT</pubDate></item>
            </channel></rss>"#;
        let (feed, entries) = parse_feed(xml);
        assert_eq!(feed, "Example Blog");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "First & Best");
        assert_eq!(entries[0].link, "https://e.com/1");
        assert_eq!(entries[1].title, "Second");
        // Newest first once sorted by the date key.
        assert!(date_key(&entries[1].date) > date_key(&entries[0].date));
    }

    #[test]
    fn parses_atom_entries() {
        let xml = r#"<feed><title>Atom Feed</title>
            <link rel="self" href="https://a.io/feed.xml"/>
            <entry><title type="text">Post One</title>
              <link rel="alternate" href="https://a.io/one"/>
              <updated>2025-06-10T12:00:00Z</updated>
              <summary>text</summary></entry>
            </feed>"#;
        let (feed, entries) = parse_feed(xml);
        assert_eq!(feed, "Atom Feed");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Post One");
        // The `rel="self"` feed link is skipped in favour of the entry's own.
        assert_eq!(entries[0].link, "https://a.io/one");
        assert_eq!(date_key(&entries[0].date), 20_250_610_120_000);
    }

    #[test]
    fn element_scanner_does_not_match_longer_names() {
        assert_eq!(elements("<items><item>x</item></items>", "item"), vec!["x"]);
        assert_eq!(elements("<entry >a</entry>", "entry"), vec!["a"]);
    }

    #[test]
    fn date_key_orders_both_stamp_formats() {
        assert_eq!(date_key("2025-06-10T12:00:00Z"), 20_250_610_120_000);
        assert_eq!(date_key("Tue, 10 Jun 2025 12:00:00 GMT"), 20_250_610_120_000);
        assert_eq!(date_key("10 Jun 2025 12:00:00 +0000"), 20_250_610_120_000);
        assert_eq!(date_key("not a date"), 0);
    }

    #[test]
    fn confluence_headings_lists_and_tables() {
        let src = "\
# Title
*** Org Third
- one
  - nested
1. first
| a | b |
|---|---|
| 1 | 2 |";
        assert_eq!(
            to_confluence_wiki(src),
            "\
h1. Title
h3. Org Third
* one
** nested
# first
||a||b||
|1|2|"
        );
    }

    #[test]
    fn confluence_inline_markup() {
        assert_eq!(to_confluence_wiki("**bold** and *bold*"), "*bold* and *bold*");
        assert_eq!(to_confluence_wiki("/italic/ and _italic_"), "_italic_ and _italic_");
        assert_eq!(to_confluence_wiki("call `foo()` now"), "call {{foo()}} now");
        assert_eq!(
            to_confluence_wiki("see [docs](https://e.com/d)"),
            "see [docs|https://e.com/d]"
        );
        assert_eq!(
            to_confluence_wiki("see [[https://e.com/d][docs]]"),
            "see [docs|https://e.com/d]"
        );
        // A bare URL keeps its slashes: the org italics rule stops at `:`.
        assert_eq!(
            to_confluence_wiki("go to https://e.com/a/b"),
            "go to https://e.com/a/b"
        );
    }

    #[test]
    fn confluence_code_blocks_are_verbatim() {
        let src = "```rust\nlet x = *y;\n```\ntail";
        assert_eq!(
            to_confluence_wiki(src),
            "{code:rust}\nlet x = *y;\n{code}\ntail"
        );
        assert_eq!(to_confluence_wiki("```\nplain\n```"), "{code}\nplain\n{code}");
    }

    #[test]
    fn search_engine_urls_are_built_from_the_table() {
        assert_eq!(
            search_engine_url("github", "ripgrep tool"),
            Some("https://github.com/search?q=ripgrep%20tool".to_string())
        );
        // Aliases and separator/case folding resolve to the same entry.
        assert_eq!(search_engine_url("GH", "x"), search_engine_url("github", "x"));
        assert_eq!(search_engine_url("ddg", "x"), search_engine_url("duckduckgo", "x"));
        assert!(search_engine_url("nope", "x").is_none());
        // Amazon's TLD is substituted, defaulting to `com`.
        std::env::remove_var("SEARCH_ENGINE_AMAZON_TLD");
        assert_eq!(
            search_engine_url("amazon", "usb c"),
            Some("https://www.amazon.com/s?k=usb%20c".to_string())
        );
        // Every template must consume the query placeholder.
        for (name, template) in SEARCH_ENGINES {
            assert!(template.contains("{}"), "{name} has no query placeholder");
        }
    }

    #[test]
    fn xkcd_urls_address_the_latest_and_a_number() {
        assert_eq!(xkcd_info_url(None), "https://xkcd.com/info.0.json");
        assert_eq!(xkcd_info_url(Some(614)), "https://xkcd.com/614/info.0.json");
    }
}
