## GitHub browser

`:github` (aliases `:gh`, `:hub`) opens a full-screen browser over the GitHub
repository the focused buffer lives in — the `origin` remote decides which one.
An optional argument picks the starting tab, e.g. `:github prs`; without one the
browser opens on the CI runs.

Requests go through the `gh` CLI when it is installed, so the browser inherits
whatever `gh auth` is logged into. Without `gh` it talks to the REST API
directly, authenticated with `$GITHUB_TOKEN` or `$GH_TOKEN`. Both transports
read `$GH_HOST`, so a non-default instance needs only the variable `gh` itself
uses — and unauthenticated, at GitHub's 60-requests-per-hour public
rate, if neither is set. Every request runs off the UI thread; the API budget
left is shown in the top-right corner.

### Tabs

| # | Tab | Shows |
| --- | --- | --- |
| 1 | Repo | Description, default branch, language, licence, stars, forks, topics |
| 2 | Runs | Workflow runs — the CI pipelines — filtered by branch, status and workflow |
| 3 | Workflows | The repository's workflows, with dispatch and enable/disable |
| 4 | PRs | Pull requests: checks, changed files, reviews, comments |
| 5 | Issues | Issues and their comment threads |
| 6 | Releases | Releases, their notes and their assets |
| 7 | Branches | Branches, protection state and tip commit |
| 8 | Commits | Recent commits, each opening its full diff |
| 9 | Inbox | Your notification inbox |

`1`–`9` jump straight to a tab; `Tab` / `Shift-Tab` (or `h` / `l`) cycle.

### Keys

Everywhere: `j`/`k` move, `C-d`/`C-u` page, `g`/`G` jump to top/bottom, `Enter`
opens, `o` opens the row in a browser, `y` copies its URL to the kill ring, `/`
filters the list, `r` refreshes the tab, `A` refreshes everything, `q` closes.

| Tab | Key | Action |
| --- | --- | --- |
| Runs | `R` / `F` | Re-run the whole run / only its failed jobs |
| Runs | `X` / `D` | Cancel the run / delete it (press `D` twice) |
| Runs | `b` / `S` / `w` | Filter by branch / cycle the status filter / clear the workflow filter |
| Runs | `a` | Auto-refresh every 8 seconds |
| Workflows | `d` / `e` | Dispatch the workflow against a ref / enable or disable it |
| PRs, Issues | `c` / `s` | Comment / close or reopen |
| PRs | `C` / `M` | `gh pr checkout` it / merge it (press `M` twice) |
| Inbox | `m` / `M` | Mark the notification read / mark them all read |

### Runs, jobs and logs

`Enter` on a run opens it: every job with its steps, each with its own status
glyph and duration, plus the artifacts the run uploaded. A run that is still
going polls on its own until it finishes; `a` toggles that for a run that has
already completed. `R`, `F` and `X` work here too.

`Enter` on a job (or on any of its steps) downloads that job's log:

| Key | Action |
| --- | --- |
| `Enter` / `Tab` | Fold or unfold the `##[group]` section under the cursor |
| `z` / `Z` | Fold every group / unfold them all |
| `t` | Show or hide the runner's per-line timestamps |
| `E` | Narrow to the error and warning lines |
| `/` | Filter to lines containing a string |
| `h` / `l` | Pan left and right |

The header counts the log's errors and warnings, so a failed job says why
without scrolling.
