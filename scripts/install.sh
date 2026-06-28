#!/usr/bin/env bash
# Build and install the `zemacs` binary into ~/.cargo/bin, and link the bundled
# runtime (themes, queries, grammars) into the config dir so it resolves.
# Works from anywhere — paths are resolved relative to this script.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cargo install --path "$repo/zemacs-term" --locked
mkdir -p "$HOME/.zemacs"
ln -sfn "$repo/runtime" "$HOME/.zemacs/runtime"

echo "installed: $(command -v zemacs 2>/dev/null || echo "$HOME/.cargo/bin/zemacs")  (runtime -> ~/.zemacs/runtime)"
