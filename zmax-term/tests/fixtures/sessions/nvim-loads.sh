#!/usr/bin/env bash
# Check that nvim restores the sessions zmax writes exactly as it restores its
# own. Run the parity test with ZMAX_SESSION_OUT=<dir> first; it keeps zmax's
# `:mksession` output there. Each is loaded in nvim the way generate.sh loads
# nvim's sessions and compared with the committed dump (`*.nvim`).
#
#   ZMAX_SESSION_OUT=/tmp/zout cargo test -p zmax-term --features integration --test session_parity
#   zmax-term/tests/fixtures/sessions/nvim-loads.sh /tmp/zout
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="${1:?usage: nvim-loads.sh <dir of zmax sessions>}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
w="$tmp/w"
mkdir -p "$w/sub"
for f in a b c d; do perl -e 'print "'"$f"' line $_\n" for 1..200' >"$w/$f.txt"; done
perl -e 'for $s (1..6) { print "section $s {{{\n"; print "  body $s.$_\n" for 1..8; print "}}}\n" }' >"$w/m.txt"
perl -e 'print "sub line $_\n" for 1..50' >"$w/sub/s.txt"
real="$(cd "$w" && pwd -P)"
status=0
for s in "$out"/*.vim; do
  name="$(basename "$s" .vim)"
  W="$real" perl -pe 's/\@DIR\@/$ENV{W}/g' "$s" | grep -v '^set stal=' >"$tmp/$name.vim"
  (cd "$w" && nvim --clean --headless --cmd 'set columns=120 lines=150 showtabline=0' \
    --cmd "let g:dumpfile='$tmp/$name.out'" -S "$tmp/$name.vim" -S "$here/dump.vim" -c 'qa!' 2>/dev/null)
  if diff -u "$here/$name.nvim" "$tmp/$name.out"; then
    echo "$name: nvim restores zmax's session as its own"
  else
    status=1
  fi
done
exit "$status"
