#!/usr/bin/env bash
# Build and install the `zmax` binary into ~/.cargo/bin, and link the bundled
# runtime (themes, queries, grammars) into the config dir so it resolves.
# Works from anywhere — paths are resolved relative to this script.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cargo install --path "$repo/zmax-term" --locked --force
mkdir -p "$HOME/.zmax"
ln -sfn "$repo/runtime" "$HOME/.zmax/runtime"

echo "installed: $(command -v zmax 2>/dev/null || echo "$HOME/.cargo/bin/zmax")  (runtime -> ~/.zmax/runtime)"
