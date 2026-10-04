" Dump the restored state in the format zmax-term/tests/session_parity.rs
" prints, for the session parity fixtures (see generate.sh).
function! s:layout(l)
  if a:l[0] ==# 'leaf'
    return fnamemodify(bufname(winbufnr(a:l[1])), ':t')
  endif
  return a:l[0] . '(' . join(map(copy(a:l[1]), 's:layout(v:val)'), ',') . ')'
endfunction
let s:args = []
for s:i in range(argc())
  let s:name = fnamemodify(argv(s:i), ':t')
  call add(s:args, s:i == argidx() ? '[' . s:name . ']' : s:name)
endfor
let s:out = ['tab ' . tabpagenr() . '/' . tabpagenr('$') . ' win ' . winnr() . ' args ' . join(s:args, ' ')]
" The listed buffers with a name, sorted (numbering is not part of a session).
let s:bufs = map(filter(getbufinfo({'buflisted': 1}), 'v:val.name !=# ""'), 'fnamemodify(v:val.name, ":t")')
call add(s:out, 'bufs ' . join(sort(s:bufs), ','))
for t in range(1, tabpagenr('$'))
  call add(s:out, 'TAB ' . t . ' cwd ' . fnamemodify(getcwd(-1, t), ':t') . ' layout ' . s:layout(winlayout(t)))
  for w in range(1, tabpagewinnr(t, '$'))
    let id = win_getid(w, t)
    let info = getwininfo(id)[0]
    let closed = []
    call win_execute(id, 'let s:l = 1 | while s:l <= line("$") | if foldclosed(s:l) == s:l | call add(closed, s:l . "-" . foldclosedend(s:l)) | let s:l = foldclosedend(s:l) + 1 | else | let s:l += 1 | endif | endwhile')
    let cur = getcurpos(id)
    call add(s:out, printf('  win %d %s w=%d h=%d cur=%d:%d top=%d folds=%s cwd=%s',
          \ w, fnamemodify(bufname(info.bufnr), ':t'), info.width, info.height, cur[1], cur[2],
          \ info.topline, join(closed, ','), fnamemodify(getcwd(w, t), ':t')))
  endfor
endfor
call writefile(s:out, g:dumpfile)
