" Each layout: build it, then write sessions/<name>.vim. Run with cwd = w/.
function! s:save(name)
  exe 'mksession! ' . g:outdir . '/' . a:name . '.vim'
  silent! %bwipeout!
  silent! tabonly | silent! only
  cd $PWD
endfunction
" stacked: three horizontal windows, sized
e a.txt | 120 | split b.txt | 40 | split c.txt | 10 | resize 8 | wincmd j | resize 20 | wincmd t
call s:save('stacked')
" nested: left column, right column split in two
e a.txt | 150 | vsplit b.txt | 30 | wincmd l | split c.txt | 60 | vertical resize 70 | wincmd h
call s:save('nested')
" folds: marker folds, some closed; manual fold in another window
e m.txt | setlocal foldmethod=marker | normal! zR
11 | normal! zc
31 | normal! zc
vsplit a.txt | setlocal foldmethod=manual | 10,20fold | 40,60fold | 45 | normal! zo | 15
call s:save('folds')
" cwd: global cd, window lcd, tab tcd
e a.txt | vsplit sub/s.txt | lcd sub | 7
tabnew b.txt | exe "tcd " . g:wdir . "/sub" | 33
tabnext 1
call s:save('cwd')
" tabs: three tabs, middle current, different layouts
e a.txt | 50 | tabnew b.txt | vsplit c.txt | 80 | tabnew d.txt | split a.txt | 190 | tabnext 2
call s:save('tabs')
" options: window-local options that sessions restore
e a.txt | setlocal nowrap nonumber list | vsplit b.txt | setlocal number relativenumber colorcolumn=80 | 25
call s:save('options')
" args: arglist and current arg
args a.txt b.txt c.txt | next | 12
call s:save('args')
qa!
