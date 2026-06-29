#!/usr/bin/env bash
# Build + install zemacs locally into ~/.cargo/bin, then cut a GitHub release.
#
# The release is tag-driven: pushing a `v<version>` tag triggers
# .github/workflows/release.yml, which builds + uploads the per-target binaries
# (full + minimal) and bumps the Homebrew tap. This script installs the local
# binary first (so you're running what you ship), then tags the current commit
# `v<workspace version>` and pushes the tag.
#
# Works from anywhere — paths are resolved relative to this script.
#
# Usage:
#   scripts/release.sh                # install into ~/.cargo/bin, then tag+push v<Cargo version>
#   scripts/release.sh --install-only # only build + install into ~/.cargo/bin
#   scripts/release.sh --release-only # only tag + push (skip the local install)
#   scripts/release.sh v1.2.3         # override the tag (default: workspace version)
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

install_only=0
release_only=0
tag=""
for arg in "$@"; do
  case "$arg" in
    --install-only) install_only=1 ;;
    --release-only) release_only=1 ;;
    v*)             tag="$arg" ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

# Default the tag to the workspace version declared in Cargo.toml.
if [[ -z "$tag" ]]; then
  version="$(grep -m1 '^version = ' "$repo/Cargo.toml" | sed -E 's/.*"([^"]+)".*/\1/')"
  [[ -n "${version:-}" ]] || { echo "could not read version from Cargo.toml" >&2; exit 1; }
  tag="v${version}"
fi

do_install() {
  "$repo/scripts/install.sh"
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
  # Make sure the commit being tagged is on the remote, then push the tag to
  # trigger the Release workflow.
  git push origin HEAD
  git tag -a "$tag" -m "$tag"
  git push origin "$tag"
  echo "pushed ${tag} — Release workflow: https://github.com/MenkeTechnologies/zemacs/actions/workflows/release.yml"
}

if [[ "$release_only" -eq 1 ]]; then
  do_release
elif [[ "$install_only" -eq 1 ]]; then
  do_install
else
  do_install
  do_release
fi
