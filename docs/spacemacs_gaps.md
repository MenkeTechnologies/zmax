# Spacemacs keybindings still missing

Generated from port/data + live vim keymap. 358 absent of 702 tracked.

## Conflict-free (337) — can wire with zero collision

Leader-group counts: SPC t (37), SPC x (34), SPC l (32), SPC k (30), SPC w (27), SPC D (26), SPC m (25), SPC g (19), SPC f (15), SPC K (14), SPC b (12), SPC i (11), SPC T (8), SPC n (7), SPC c (5), SPC h (4), SPC a (4), SPC p (4), SPC u (3), SPC q (3), SPC j (2), SPC s (2), SPC 1 (1), SPC 2 (1), SPC 3 (1), SPC 4 (1), SPC 5 (1), SPC 6 (1), SPC 7 (1), SPC 8 (1), SPC 9 (1), SPC e (1), SPC o (1), SPC r (1), SPC * (1)

| key | description |
|---|---|
| `SPC *` | search with the first found tool with default input |
| `SPC 1` | go to window number 1 |
| `SPC 2` | go to window number 2 |
| `SPC 3` | go to window number 3 |
| `SPC 4` | go to window number 4 |
| `SPC 5` | go to window number 5 |
| `SPC 6` | go to window number 6 |
| `SPC 7` | go to window number 7 |
| `SPC 8` | go to window number 8 |
| `SPC 9` | go to window number 9 |
| `SPC D B` | ask for a file and run ediff with its backup file |
| `SPC D b 3` | ask for 3 opened buffers and start an ediff session with them |
| `SPC D b b` | ask for 2 opened buffers and start an ediff session with them |
| `SPC D b p` | ask for a buffer or file that contains a patch to apply to a buffer and start an ediff session with the result |
| `SPC D d 3` | ask for 3 directories and run ediff on them comparing files that have the same name in all of them |
| `SPC D d d` | ask for 2 directories and run ediff on them comparing files that have the same name in both |
| `SPC D d r` | run ediff on a directory comparing its files with their revisions if under version control |
| `SPC D f .` | start an ediff session between your .spacemacs and its default template in Spacemacs core |
| `SPC D f 3` | ask for 3 files and start an ediff session with them |
| `SPC D f f` | ask for 2 files and start an ediff session with them |
| `SPC D f p` | ask for a buffer or file that contains a patch to apply to a file and start an ediff session with the result |
| `SPC D f v` | start ediff between versions of a file |
| `SPC D h` | open ediff documentation within Emacs |
| `SPC D m b 3` | start an ediff merge session between 2 buffers and their ancestor |
| `SPC D m b b` | start an ediff merge session between 2 buffers |
| `SPC D m d 3` | start an ediff merge session between files with the same name in 2 directories and with a 3rd directory containing their ancestor |
| `SPC D m d d` | start an ediff merge session between files with the same name in 2 directories |
| `SPC D m f 3` | start an ediff merge session between 2 files and their ancestor |
| `SPC D m f f` | start an ediff merge session between 2 files |
| `SPC D m r 3` | start an ediff merge session between two revisions of a file with a common ancestor |
| `SPC D m r r` | start an ediff merge session between two revisions of a file |
| `SPC D r l` | start an ediff session between two regions to perform a linewise diff (use this for large regions) |
| `SPC D r w` | start an ediff session between two regions to perform a wordwise diff (use this for small regions) |
| `SPC D s` | display ediff registries |
| `SPC D w l` | compare linewise the portions of visible text of 2 windows which are selected by clicking |
| `SPC D w w` | compare wordwise the portions of visible text of 2 windows which are selected by clicking |
| `SPC K c a` | increment macro counter |
| `SPC K c c` | insert the current value of the macro counter |
| `SPC K c f` | set the display format for the macro counter |
| `SPC K e b` | assign a key binding to the last macro |
| `SPC K e e` | edit last macro in a buffer |
| `SPC K e l` | edit a macro from lossage information (last 300 executed commands) |
| `SPC K e n` | give a name to the last macro |
| `SPC K e r` | write the last macro to a register (use SPC r r to call it) |
| `SPC K e s` | step by step edit of the last macro |
| `SPC K r L` | view head macro in ring |
| `SPC K r d` | delete head macro in ring |
| `SPC K r n` | cycle to next macro in ring |
| `SPC K r p` | cycle to previous macro in ring |
| `SPC K r s` | swap the first two macros in ring |
| `SPC T B` | toggle frame background transparency |
| `SPC T F` | toggle frame fullscreen |
| `SPC T M` | toggle frame maximize |
| `SPC T T` | toggle frame transparency (transient state) |
| `SPC T f` | toggle fringe display |
| `SPC T m` | toggle menu bar |
| `SPC T t` | toggle tool bar |
| `SPC T ~` | display tilde in fringe on empty lines |
| `SPC a c` | calc-dispatch |
| `SPC a k` | launch paradox |
| `SPC a t r d` | deer / dirvish single column |
| `SPC a t r r` | launch ranger / dirvish full layout |
| `SPC b . 1..9` | move current buffer to nth window |
| `SPC b . C-1..C-9` | switch focus to nth window |
| `SPC b . C-d` | bury current buffer |
| `SPC b . M-1..M-9` | swap current buffer with nth window |
| `SPC b . q` | quit transient state |
| `SPC b C-D` | kill buffers using a regular expression |
| `SPC b H` | open or select the *Help* buffer |
| `SPC b M` | kill all buffers matching the regexp |
| `SPC b N I` | create an indirect buffer that is clone of currently opened buffer, and open it in other window |
| `SPC b h` | open *spacemacs* home buffer |
| `SPC b u` | reopen the most recently killed file buffer |
| `SPC b w` | toggle read-only (writable state) |
| `SPC c C` | compile |
| `SPC c P` | invert comment paragraphs |
| `SPC c k` | kill compilation |
| `SPC c m` | helm-make |
| `SPC c r` | recompile |
| `SPC e c` | clear all errors |
| `SPC f D` | delete a file and the associated buffer without confirmation |
| `SPC f E` | open a file with elevated privileges (sudo edit) |
| `SPC f J` | open a junk file, in mode determined by the file extension (defaulting to fundamental mode) |
| `SPC f c` | copy current file to a different location |
| `SPC f e D` | open ediff buffer of ~/.spacemacs and dotspacemacs-template.el |
| `SPC f e E` | reload the environment variables by executing dotspacemacs/user-env |
| `SPC f e I` | open the early-init.el |
| `SPC f e U` | update packages |
| `SPC f e c` | recompile all elpa packages |
| `SPC f e l` | locate an Emacs library |
| `SPC f e v` | display and copy the spacemacs version |
| `SPC f h` | open binary file with hexl (a hex editor) |
| `SPC f v d` | add a directory variable |
| `SPC f v f` | add a local variable to the current file |
| `SPC f v p` | add a local variable to the first line of the current file |
| `SPC g H c` | clear commit highlights |
| `SPC g H h` | highlight regions by age of commits |
| `SPC g H t` | highlight regions by last updated time |
| `SPC g M` | display the last commit message of the current line |
| `SPC g S` | stage current file |
| `SPC g U` | unstage current file |
| `SPC g f d` | diff for current file |
| `SPC g f f` | view a file at a specific branch or commit |
| `SPC g f m` | magit dispatch popup for file operations |
| `SPC g l C` | create link to the file at a commit and copy it |
| `SPC g l L` | create a link to the file highlighting the selected lines |
| `SPC g l P` | create a link with permalink highlighting selected lines |
| `SPC g l c` | browse to the current file at a commit |
| `SPC g l l` | browse to file at current lines position |
| `SPC g l p` | browse to file at current lines position using permalink |
| `SPC g m` | magit dispatch popup |
| `SPC g s` | open a magit status window |
| `SPC g t` | launch the git time machine |
| `SPC g u` | select a remote repository |
| `SPC h P k` | stop the profiler |
| `SPC h P r` | display the profiler report |
| `SPC h P s` | start the profiler |
| `SPC h P w` | write the report to file |
| `SPC i U 1` | insert UUIDv1 (use universal argument to insert with CID format) |
| `SPC i U 4` | insert UUIDv4 (use universal argument to insert with CID format) |
| `SPC i U U` | insert UUIDv4 (use universal argument to insert with CID format) |
| `SPC i l l` | insert lorem-ipsum list |
| `SPC i l p` | insert lorem-ipsum paragraph |
| `SPC i l s` | insert lorem-ipsum sentence |
| `SPC i p 1` | insert simple password |
| `SPC i p 2` | insert stronger password |
| `SPC i p 3` | insert password for paranoids |
| `SPC i p n` | insert a numerical password |
| `SPC i p p` | insert a phonetically easy password |
| `SPC j S` | split a quoted string or s-expression, insert a new line and auto-indent |
| `SPC j s` | split a quoted string or s-expression in place |
| `SPC k %` | go to other paren of the same pair |
| `SPC k (` | insert sexp before current one |
| `SPC k )` | insert sexp after current one |
| `SPC k :` | ex command |
| `SPC k B` | barf backward |
| `SPC k Ds` | delete symbol backward |
| `SPC k Dw` | delete word backward |
| `SPC k Dx` | delete sexp backward |
| `SPC k E` | splice, killing backward |
| `SPC k H` | go backward to previous sexp |
| `SPC k J` | join sexp |
| `SPC k L` | go forward to next sexp |
| `SPC k P` | paste before |
| `SPC k S` | slurp backward |
| `SPC k V` | switch to visual state and begin line-wise selection |
| `SPC k W` | unwrap sexp |
| `SPC k ` k` | hybrid delete sexp |
| `SPC k ` p` | hybrid push: <point>as bs -> <point>bs as |
| `SPC k ` s` | hybrid slurp forward |
| `SPC k ` t` | hybrid transpose |
| `SPC k a` | absorb: (a (bs <point> ..)) -> ((bs a <point> ..)) |
| `SPC k b` | barf forward: (a bs c) -> (a bs) c |
| `SPC k c` | convolute: (as (bs <point> ..)) -> (bs (as <point> ..)) |
| `SPC k e` | splice, killing forward: (as (bs <point> cs) ds) -> (as bs ds) |
| `SPC k i` | switch to insert state |
| `SPC k p` | paste after |
| `SPC k r` | raise: (as <point> b ..) -> <point>b |
| `SPC k s` | slurp forward: a (bs) c -> a (bs c) |
| `SPC k t` | transpose: (as <point> bs) -> (bs <point> as) |
| `SPC k u` | undo |
| `SPC l 0..9` | switch to nth layout |
| `SPC l A` | add all the buffers from another layout in the current one |
| `SPC l C-0..C-9` | switch to nth layout and keep the transient state active |
| `SPC l C-h` | previous layout in list |
| `SPC l C-l` | next layout in list |
| `SPC l D` | delete the other layouts and keep their buffers |
| `SPC l L` | load layouts from file |
| `SPC l N` | previous layout in list |
| `SPC l R` | rename current layout |
| `SPC l TAB` | switch to the latest layout |
| `SPC l X` | kill other layouts with their buffers |
| `SPC l b` | select a buffer in the current layout |
| `SPC l d` | delete the current layout and keep its buffers |
| `SPC l h` | go to default layout |
| `SPC l l` | select/create a layout |
| `SPC l n` | next layout in list |
| `SPC l o` | open a custom layout |
| `SPC l p` | previous layout in list |
| `SPC l t` | display a buffer without adding it to the current layout |
| `SPC l w` | initiate transient state |
| `SPC l w 0..9` | switch to nth workspace |
| `SPC l w C-0..C-9` | switch to nth workspace and keep the transient state active |
| `SPC l w N` | switch to previous workspace |
| `SPC l w R` | rename current workspace |
| `SPC l w TAB` | switch to last active workspace |
| `SPC l w d` | close current workspace |
| `SPC l w h` | switch to previous workspace |
| `SPC l w l` | switch to next workspace |
| `SPC l w n` | switch to next workspace |
| `SPC l w p` | switch to previous workspace |
| `SPC l w w` | switch to tagged workspace |
| `SPC l x` | kill current layout with its buffers |
| `SPC m ,` | commit changes with the entered message (git commit buffer) |
| `SPC m C` | checkout pull request (not for issues) (forge) |
| `SPC m D` | delete comment under cursor (forge) |
| `SPC m M` | create mark to use with topics (forge) |
| `SPC m a` | discard message and abort the commit (git commit buffer) |
| `SPC m b` | browse topic (open in web browser) (forge) |
| `SPC m c` | commit changes with the entered message (git commit buffer) |
| `SPC m d` | toggle draft pull request (forge) |
| `SPC m e` | edit topic body (forge) |
| `SPC m e $` | go to end of line and evaluate last sexp |
| `SPC m e b` | evaluate buffer |
| `SPC m e c` | evaluate current form (a def or a set) |
| `SPC m e e` | evaluate last sexp |
| `SPC m e f` | evaluate current defun |
| `SPC m e l` | go to end of line and evaluate last sexp |
| `SPC m e r` | evaluate region |
| `SPC m k` | discard message and abort the commit (git commit buffer) |
| `SPC m m` | edit topic marks (mark is an unshared label) (forge) |
| `SPC m n` | edit personal note (adds to top of topic) (forge) |
| `SPC m r` | edit list of people to review an existing topic (forge) |
| `SPC m s` | change topic state (open, closed, draft, etc.) (forge) |
| `SPC m t` | edit topic title (forge) |
| `SPC m t b` | execute buffer tests |
| `SPC m t q` | ask for test function to execute |
| `SPC m u` | copy URL of topic (add to kill ring) (forge) |
| `SPC n F` | narrow to the current function in an indirect buffer |
| `SPC n P` | narrow to the visible page in an indirect buffer |
| `SPC n R` | narrow to the selected text in an indirect buffer |
| `SPC n f` | narrow the buffer to the current function |
| `SPC n p` | narrow the buffer to the visible page |
| `SPC n r` | narrow the buffer to the selected text |
| `SPC n w` | widen, i.e. show the whole buffer again |
| `SPC o c` | example user binding: run org mode capture (SPC o is reserved for user) |
| `SPC p '​` | open a shell in project's root (with the shell layer) |
| `SPC p c` | compile project using projectile |
| `SPC p i` | install the project |
| `SPC p u` | run project using projectile |
| `SPC q T` | Restart Emacs and debug with --adv-timers |
| `SPC q r` | Restart both Emacs and the server, prompting to save any changed buffers |
| `SPC q t` | Restart Emacs and debug with --with-timed-requires |
| `SPC r s` | resume search buffer (completion or converted search buffer) |
| `SPC s c` | clear persistent search highlighting |
| `SPC s w g` | Get Google suggestions in emacs. Opens Google results in Browser. |
| `SPC t -` | centered-cursor mode |
| `SPC t 8` | highlight any character past the 80th column |
| `SPC t C--` | global centered cursor |
| `SPC t C-8` | global toggle highlight of characters for long lines |
| `SPC t C-W` | global automatic whitespace cleanup |
| `SPC t E e` | emacs editing style (holy mode) |
| `SPC t E h` | hybrid editing style (hybrid mode) |
| `SPC t F` | toggle auto-fill mode |
| `SPC t G` | ggtags mode |
| `SPC t I` | toggle aggressive indent mode |
| `SPC t L` | toggle visual lines |
| `SPC t S` | toggle spell checking (flyspell) |
| `SPC t W` | toggle automatic whitespace cleanup |
| `SPC t c` | toggle camel case (subword) motion |
| `SPC t f` | display the fill column (by default the fill column is set to 80) |
| `SPC t g` | toggle golden-ratio mode |
| `SPC t h a` | toggle automatic highlight of symbol under point after ahs-idle-interval seconds |
| `SPC t h s` | toggle syntax highlighting |
| `SPC t k k` | toggle which-key persistent state |
| `SPC t k m` | show persistent major-mode keymap. Toggle off with SPC t k k |
| `SPC t k t` | show persistent top-level keymap. Toggle off with SPC t k k |
| `SPC t m M` | toggle major mode |
| `SPC t m T` | toggle mode line itself |
| `SPC t m V` | toggle new version lighter |
| `SPC t m b` | toggle the battery status |
| `SPC t m c` | toggle the org task clock (available in org layer) |
| `SPC t m m` | toggle the minor mode lighters |
| `SPC t m n` | toggle the cat! (if colors layer is declared in your dotfile) |
| `SPC t m p` | toggle the point character position |
| `SPC t m r` | toggle responsivness of the mode-line |
| `SPC t m s` | toggle system monitor (displayed in the minibuffer) |
| `SPC t m t` | toggle the time |
| `SPC t m v` | toggle the version control info |
| `SPC t n v` | toggle smooth scrolling |
| `SPC t s` | syntax checking (flycheck) |
| `SPC t y` | yasnippet mode |
| `SPC t z` | toggle 0/1 based column indexing |
| `SPC u SPC b D` | kill a visible buffer and its window using ace-window |
| `SPC u SPC h I` | Open Spacemacs GitHub issue page with pre-filled information - include last pressed keys |
| `SPC u SPC w D` | delete another window and its current buffer using ace-window |
| `SPC w . 0..9` | go to window number n |
| `SPC w . H` | move window to the left |
| `SPC w . J` | move window to the bottom |
| `SPC w . K` | move bottom to the top |
| `SPC w . L` | move window to the right |
| `SPC w . R` | rotate windows backward |
| `SPC w . S` | horizontal split and focus new window |
| `SPC w . U` | redo window layout |
| `SPC w . V` | vertical split and focus new window |
| `SPC w . [` | shrink window horizontally (transient state) |
| `SPC w . ]` | enlarge window horizontally (transient state) |
| `SPC w . _` | maximize window horizontally (transient state) |
| `SPC w . a` | call ace window mode |
| `SPC w . g` | toggle golden-ratio on and off |
| `SPC w . m` | toggle maximization of current window |
| `SPC w . r` | rotate windows forward |
| `SPC w . s` | horizontal split |
| `SPC w . u` | undo window layout (used to effectively undo a closed window) |
| `SPC w . v` | vertical split |
| `SPC w . w` | focus other window |
| `SPC w . x` | delete window and kill buffer |
| `SPC w . {` | shrink window vertically (transient state) |
| `SPC w . |` | maximize window vertically (transient state) |
| `SPC w . }` | enlarge window vertically (transient state) |
| `SPC w [` | shrink window horizontally (enter transient state) |
| `SPC w u` | undo window layout (used to effectively undo a closed window) |
| `SPC w {` | shrink window vertically (enter transient state) |
| `SPC x .` | enter the drag stuff transient state |
| `SPC x A` | open all visible links |
| `SPC x O` | use avy to select multiple links in the frame and open them |
| `SPC x U` | set the selected text to upper case |
| `SPC x Y` | use avy to copy multiple links in the frame |
| `SPC x a ¦` | align region at ¦ |
| `SPC x e` | Edit strings in place |
| `SPC x g T` | reverse source and target languages |
| `SPC x g l` | set languages used by translate commands |
| `SPC x g t` | translate current word using Google Translate |
| `SPC x i C` | change symbol style to UpperCamelCase |
| `SPC x i U` | change symbol style to UP_CASE |
| `SPC x i _` | change symbol style to under_score |
| `SPC x l r` | randomize lines in region |
| `SPC x r '` | generate strings given by a regexp given this list is finite |
| `SPC x r /` | Explain the regexp around point with rx |
| `SPC x r c` | Convert regexp around point to the other form and display the result in the minibuffer |
| `SPC x r e '` | generate strings from Emacs Lisp regexp |
| `SPC x r e /` | Explain Emacs Lisp regexp |
| `SPC x r e p` | Convert Emacs Lisp regexp to PCRE |
| `SPC x r e t` | Replace Emacs Lisp regexp by rx form or vice versa |
| `SPC x r e x` | Convert Emacs Lisp regexp to rx form |
| `SPC x r p '` | generate strings from PCRE regexp |
| `SPC x r p /` | Explain PCRE regexp |
| `SPC x r p e` | Convert PCRE regexp to Emacs Lisp |
| `SPC x r p x` | Convert PCRE to rx form |
| `SPC x r t` | Replace regexp around point by the rx form or vice versa |
| `SPC x r x` | Convert regexp around point in rx form and display the result in the minibuffer |
| `SPC x t e` | swap (transpose) the current sexp with the previous one |
| `SPC x t p` | swap (transpose) the current paragraph with the previous one |
| `SPC x t s` | swap (transpose) the current sentence with the previous one |
| `SPC x w d` | show dictionary entry of word from wordnik.com |
| `SPC x w r` | randomize words in region |
| `SPC x y` | use avy to copy a link in the frame |

## Needs a parent chord freed (prefix-leaf) (8)

| key | description | conflicts with |
|---|---|---|
| `SPC k I (insert)` | switch to insert state at beginning of current line | `space k I = move_parent_node_start` |
| `SPC w c .` | center buffer and enable centering transient state | `space w c = wclose` |
| `SPC w c C` | toggle visual distraction free mode | `space w c = wclose` |
| `SPC w c c` | toggle visual centering of the current buffer | `space w c = wclose` |
| `SPC w p m` | open messages buffer in a popup window | `space w p = rotate_view` |
| `SPC w p p` | close the current sticky popup window | `space w p = rotate_view` |
| `SPC z f 0` | reset the frame content size and initiate the frame scaling transient state | `space z = toggle_ide` |
| `SPC z x 0` | reset the font size (no scaling) and initiate the font scaling transient state | `space z = toggle_ide` |

## Exact chord already in use (taken) (8)

| key | description | conflicts with |
|---|---|---|
| `SPC g b` | open a magit blame | `git_blame_line` |
| `SPC g i` | initialize a new git repository | `goto_implementation` |
| `SPC l s` | save layouts to file | `signature_help` |
| `SPC p v` | open project root in vc-dir or magit | `reveal_in_tree` |
| `SPC w =` | balance split windows | `resize_view_equalize` |
| `SPC w b` | force the focus back to the minibuffer | `jump_view_down` |
| `SPC w f` | toggle follow mode | `goto_file_hsplit` |
| `SPC w t` | toggle window dedication (dedicated window cannot be reused by a mode) | `jump_view_up` |

## We model as submap; not a real loss (is-submap) (5)

| key | description | conflicts with |
|---|---|---|
| `SPC b .` | initiate transient state | `space b . N, space b . b, space b . d` |
| `SPC l` | initiate transient state | `space l a, space l f, space l k` |
| `SPC t n` | toggle line numbers | `space t n a, space t n n, space t n r` |
| `SPC u` | universal argument | `space u space b d, space u space b m, space u space w 1` |
| `SPC w .` | initiate window transient state | `space w . -, space w . /, space w . D` |
