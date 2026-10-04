#!/usr/bin/env bash
# Rebuild the session parity fixtures from nvim: write one `:mksession` per
# layout in layouts.vim, then load each in a fresh nvim at zmax's integration
# screen (120x150) and dump what it restored (dump.vim). zmax's tests never
# draw its tab bar, so nvim loads a copy without the session's `set stal=…`
# lines and has no tab line either: both lay windows out in the same rows. The workspace matches write_workspace() in
# session_parity.rs; its path is replaced by @DIR@ in the committed sessions.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
w="$tmp/w"
mkdir -p "$w/sub" "$tmp/out"
for f in a b c d; do perl -e 'print "'"$f"' line $_\n" for 1..200' >"$w/$f.txt"; done
perl -e 'for $s (1..6) { print "section $s {{{\n"; print "  body $s.$_\n" for 1..8; print "}}}\n" }' >"$w/m.txt"
perl -e 'print "sub line $_\n" for 1..50' >"$w/sub/s.txt"
cd "$w"
nvim --clean --headless --cmd 'set columns=200 lines=60' \
  --cmd "let g:outdir='$tmp/out'" --cmd "let g:wdir='$w'" -S "$here/layouts.vim"
for s in "$tmp"/out/*.vim; do
  name="$(basename "$s" .vim)"
  grep -v '^set stal=' "$s" >"$tmp/$name.notabline.vim"
  nvim --clean --headless --cmd 'set columns=120 lines=150 showtabline=0' \
    --cmd "let g:dumpfile='$here/$name.nvim'" -S "$tmp/$name.notabline.vim" -S "$here/dump.vim" -c 'qa!' 2>/dev/null
  W="$w" perl -pe 's/\Q$ENV{W}\E/\@DIR\@/g' "$s" >"$here/$name.vim"
done
echo "regenerated: $(ls "$here"/*.nvim | wc -l | tr -d ' ') sessions"
