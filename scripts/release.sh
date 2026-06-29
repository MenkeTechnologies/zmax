#!/usr/bin/env bash
# Build + install zemacs locally into ~/.cargo/bin, then cut a GitHub release.
#
# The release is tag-driven: pushing a `v<version>` tag triggers
# .github/workflows/release.yml, which builds + uploads the per-target binaries
# (full + minimal) and bumps the Homebrew tap. This script first pulls the latest
# upstream code for the vendored scripting submodules (so the release ships the
# newest interpreters), installs the local binary (so you're running what you
# ship), then tags the current commit `v<workspace version>` and pushes the tag.
#
# Works from anywhere — paths are resolved relative to this script.
#
# Usage:
#   scripts/release.sh                     # update submodules, install, then tag+push v<Cargo version>
#   scripts/release.sh --install-only      # only update submodules + build/install into ~/.cargo/bin
#   scripts/release.sh --release-only      # only update submodules + tag/push (skip the local install)
#   scripts/release.sh --no-submodule-update  # release exactly the currently-pinned submodule commits
#   scripts/release.sh 1.2.3               # bump Cargo.toml to 1.2.3, then tag v1.2.3 + push
#   scripts/release.sh v1.2.3              # same (the leading v is optional)
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

install_only=0
release_only=0
update_submodules=1
version=""
for arg in "$@"; do
  case "$arg" in
    --install-only)        install_only=1 ;;
    --release-only)        release_only=1 ;;
    --no-submodule-update) update_submodules=0 ;;
    v[0-9]*)               version="${arg#v}" ;;
    [0-9]*)                version="$arg" ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

# Resolve the release version: an explicit `X.Y.Z` / `vX.Y.Z` argument, otherwise
# the workspace version already in Cargo.toml. An explicit version that differs is
# written into the manifest (and committed) before tagging, so the built binary's
# version matches the tag.
current_version="$(grep -m1 '^version = ' "$repo/Cargo.toml" | sed -E 's/.*"([^"]+)".*/\1/')"
[[ -n "${current_version:-}" ]] || { echo "could not read version from Cargo.toml" >&2; exit 1; }
version="${version:-$current_version}"
tag="v${version}"

# Pull the latest upstream commit for each vendored scripting submodule and
# commit the pointer bumps. The CI release builds from the committed submodule
# pointers, so this must land before the tag for the release to ship new code.
do_update_submodules() {
  cd "$repo"
  echo "updating vendored submodules (vendor/*) to latest upstream..."
  git submodule update --init --remote --recursive -- vendor
  if [[ -n "$(git status --porcelain -- vendor)" ]]; then
    git add vendor
    git commit -m "chore: bump vendored scripting submodules to latest upstream"
    echo "committed submodule pointer bumps"
  else
    echo "submodules already at latest upstream"
  fi
}

do_install() {
  "$repo/scripts/install.sh"
}

# Bump the workspace version to $version (and the report's version refs), sync the
# lockfile, and commit — only when an explicit version was given that differs from
# Cargo.toml. No-op otherwise.
do_bump_version() {
  cd "$repo"
  [[ "$version" == "$current_version" ]] && return 0
  echo "bumping version ${current_version} -> ${version}"
  # Rewrite the first `version = "..."` line (the [workspace.package] one).
  awk -v v="$version" 'BEGIN{done=0} /^version = "/ && !done {sub(/"[^"]+"/, "\"" v "\""); done=1} {print}' \
    Cargo.toml >Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml
  # Keep the engineering report's version stamps in sync (best-effort).
  perl -pi -e 's/v\d+\.\d+\.\d+/v'"$version"'/g' docs/report.html 2>/dev/null || true
  # Sync the lockfile's workspace entries so CI's `cargo build --locked` matches.
  cargo update --workspace --quiet 2>/dev/null || true
  git add Cargo.toml Cargo.lock docs/report.html
  git commit -m "release: v${version}"
}

do_release() {
  cd "$repo"
  if [[ -n "$(git status --porcelain)" ]]; then
    echo "working tree is dirty — commit or stash before releasing" >&2
    exit 1
  fi
  if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
    echo "tag ${tag} already exists — bump the version in Cargo.toml or pass a new tag" >&2
    exit 1
  fi
  # Push the commit (incl. any submodule bump) so CI builds it, then push the tag.
  git push origin HEAD
  git tag -a "$tag" -m "$tag"
  git push origin "$tag"
  echo "pushed ${tag} — Release workflow: https://github.com/MenkeTechnologies/zemacs/actions/workflows/release.yml"
}

[[ "$update_submodules" -eq 1 ]] && do_update_submodules

if [[ "$release_only" -eq 1 ]]; then
  do_bump_version
  do_release
elif [[ "$install_only" -eq 1 ]]; then
  do_bump_version
  do_install
else
  do_bump_version
  do_install
  do_release
fi
