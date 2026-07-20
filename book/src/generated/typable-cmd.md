| Name | Description |
| --- | --- |
| `:terminal`, `:term` | Open an integrated terminal (PTY shell) running $SHELL. |
| `:ide`, `:workbench` | Enter IDE mode (file-tree sidebar + panels, like `--ide` / F2). |
| `:diff`, `:gdiff` | Open a read-only side-by-side diff of the buffer vs. its git HEAD version. |
| `:diff-buffer-with-file` | Show a unified diff of the buffer's contents vs. its file on disk (emacs diff-buffer-with-file). |
| `:reveal`, `:open-repo` | Open this repository's homepage (GitHub/GitLab/Bitbucket/…) in the browser. |
| `:compare-ref`, `:compare-branch` | Diff the buffer against its version at a git ref (JetBrains Compare with Branch). |
| `:merge`, `:resolve` | Resolve the buffer's git merge conflicts in a 3-pane (ours/result/theirs) view. |
| `:magit`, `:git`, `:gst` | Open the Magit-style git status (stage/unstage/discard/commit changes by section). |
| `:hex`, `:hexview`, `:hexedit` | Open a read-only xxd-style hex viewer of a file's raw bytes (optional path; defaults to the buffer's file). |
| `:snippets`, `:snip` | Open the user snippet library editor (create/edit/delete reusable snippets). |
| `:org-cycle`, `:org-fold` | Toggle a fold over the current org heading's subtree (TAB-style outline cycling). |
| `:org-todo` | Cycle the current org heading's TODO keyword: none -> TODO -> DONE -> none. |
| `:org-promote` | Promote the current org heading one level (remove a leading star). |
| `:org-demote` | Demote the current org heading one level (add a leading star). |
| `:org-move-subtree-down`, `:org-metadown` | Move the org subtree at point down past its next sibling (emacs org-move-subtree-down). |
| `:org-move-subtree-up`, `:org-metaup` | Move the org subtree at point up past its previous sibling (emacs org-move-subtree-up). |
| `:org-schedule` | Add/update a SCHEDULED: timestamp on the heading at point: :org-schedule 2026-07-15 (emacs org-schedule). |
| `:org-deadline` | Add/update a DEADLINE: timestamp on the heading at point: :org-deadline 2026-07-15 (emacs org-deadline). |
| `:org-next-heading` | Move the cursor to the next org heading line. |
| `:org-prev-heading` | Move the cursor to the previous org heading line. |
| `:org-fold-all` | Fold every org heading subtree in the buffer. |
| `:org-unfold-all` | Unfold every fold in the buffer. |
| `:org-agenda`, `:agenda` | Open the org agenda: TODO/DONE items across open .org buffers and *.org files under the working directory, grouped by scheduled/deadline date. |
| `:org-agenda-file-to-front` | Move/add the current file to the top of the agenda file list; `end` adds it to the end instead (emacs org-agenda-file-to-front). |
| `:org-priority` | Cycle the current org heading's priority cookie: none -> [#A] -> [#B] -> [#C] -> none. |
| `:org-capture`, `:capture` | Prompt for a line of text and append it as a '* TODO <text>' entry to an inbox org file (default <working-dir>/inbox.org, or an explicit path argument). |
| `:exit`, `:x`, `:xit` | Write changes to disk if the buffer is modified and then quit. Accepts an optional path (:exit some/path.txt). |
| `:exit!`, `:x!`, `:xit!` | Force write changes to disk, creating necessary subdirectories, if the buffer is modified and then quit. Accepts an optional path (:exit! some/path.txt). |
| `:quit`, `:q` | Close the current view. |
| `:help`, `:h` | Open the inline Help browser (searchable: commands, keybindings, topics). |
| `:wc`, `:words`, `:count` | Show document line/word/char counts (and selection stats). |
| `:blame` | Show git blame for the current line in the status bar. |
| `:reopen`, `:reopen-closed` | Reopen the most recently closed file. |
| `:zen` | Toggle the IDE workbench (Zen / focus mode). |
| `:emmet`, `:zencode` | Expand the emmet/zen HTML abbreviation before the cursor. |
| `:quit!`, `:q!` | Force close the current view, ignoring unsaved changes. |
| `:open`, `:o`, `:edit`, `:e`, `:ex` | Open a file from disk into the current view (vim :edit). |
| `:visual`, `:vi`, `:vis` | Leave Ex mode (gQ) and go back to Normal; outside Ex mode, open a file like :edit (vim :visual). |
| `:args`, `:ar`, `:argglobal`, `:argg`, `:arglocal`, `:argl` | Show the argument list, or set it to the given files and edit the first (vim :args). |
| `:argadd`, `:arga` | Add files to the argument list after the current entry (vim :argadd). |
| `:argedit`, `:arge` | Add a file to the argument list and edit it (vim :argedit). |
| `:argdelete`, `:argd` | Delete argument-list entries matching the given glob patterns (vim :argdelete). |
| `:argdedupe` | Remove duplicate entries from the argument list (vim :argdedupe). |
| `:next`, `:argnext` | Edit the next file in the argument list (vim :next). |
| `:previous`, `:Next`, `:prev`, `:argprev` | Edit the previous file in the argument list (vim :previous / :Next). |
| `:wnext`, `:wn` | Write the current buffer, then edit the next file in the argument list (vim :wnext). |
| `:wprevious`, `:wp`, `:wNext`, `:wN` | Write the current buffer, then edit the previous file in the argument list (vim :wprevious / :wNext). |
| `:find`, `:fin` | Find a file in the 'path' (buffer dir, cwd, then a recursive cwd walk) and edit it (vim :find). |
| `:sfind`, `:sf` | Split the window and edit a file found in the 'path' (vim :sfind). |
| `:first`, `:rewind`, `:rew` | Edit the first file in the argument list (vim :first / :rewind). |
| `:view`, `:vie`, `:find-file-read-only` | Edit a file read-only (vim :view / emacs find-file-read-only). |
| `:sview`, `:svie`, `:find-file-read-only-other-window` | Split and edit a file read-only (vim :sview / emacs find-file-read-only-other-window). |
| `:autocmd`, `:au` | Register an autocommand: :autocmd {events} {pattern} {command} (`:autocmd !` clears the open group, or all). |
| `:augroup`, `:aug`, `:augr`, `:augro`, `:augrou` | Open the autocmd group the following :autocmds join (`:augroup END` closes it, `:augroup! {name}` deletes it) (vim :augroup). |
| `:last`, `:la` | Edit the last file in the argument list (vim :last). |
| `:argument`, `:argu` | Edit the Nth file in the argument list (vim :argument). |
| `:snext`, `:sn` | Split the window and edit the next argument (vim :snext). |
| `:sprevious`, `:sprev`, `:sNext`, `:sN` | Split the window and edit the previous argument (vim :sprevious / :sNext). |
| `:srewind`, `:sre`, `:sfirst`, `:sfir` | Split the window and edit the first argument (vim :srewind / :sfirst). |
| `:slast`, `:sla` | Split the window and edit the last argument (vim :slast). |
| `:sargument`, `:sa` | Split the window and edit the Nth argument (vim :sargument). |
| `:argdo` | Run an Ex command on each file in the argument list (vim :argdo). |
| `:all`, `:sall` | Open a window for each file in the argument list (vim :all / :sall). |
| `:compile` | Run a shell command and collect its errors into the compilation list (emacs compile / M-x compile). |
| `:projectile-test-project`, `:test-project` | Run the project's test command in the project root (projectile projectile-test-project, spacemacs SPC p T). |
| `:dotnet-build` | Build a project or solution, collecting its errors (dotnet.el dotnet-build, spacemacs SPC m p b). |
| `:dotnet-clean` | Clean the .NET build output (dotnet.el dotnet-clean, spacemacs SPC m p c). |
| `:dotnet-publish` | Publish a .NET project for deployment (dotnet.el dotnet-publish, spacemacs SPC m p p). |
| `:dotnet-restore` | Restore the .NET project's dependencies (dotnet.el dotnet-restore, spacemacs SPC m p r s). |
| `:dotnet-run` | Build and run the project in a directory, remembering it (dotnet.el dotnet-run, spacemacs SPC m p r r). |
| `:dotnet-run-with-args` | Build and run the project with the given arguments (dotnet.el dotnet-run-with-args, spacemacs SPC m p r a). |
| `:dotnet-test` | Run a test project's unit tests, remembering it (dotnet.el dotnet-test, spacemacs SPC m p t). |
| `:dotnet-add-package` | Add a package reference to the project (dotnet.el dotnet-add-package, spacemacs SPC m p a p). |
| `:dotnet-add-reference` | Add a project reference: :dotnet-add-reference {reference} {project} (dotnet.el dotnet-add-reference, spacemacs SPC m p a r). |
| `:dotnet-new` | Create a project: :dotnet-new {path} [template] [language] (dotnet.el dotnet-new, spacemacs SPC m p n). |
| `:dotnet-sln-add` | Add a project to a solution: :dotnet-sln-add {solution} {project} (dotnet.el dotnet-sln-add, spacemacs SPC m p s a). |
| `:dotnet-sln-remove` | Remove a project from a solution: :dotnet-sln-remove {solution} {project} (dotnet.el dotnet-sln-remove, spacemacs SPC m p s r). |
| `:dotnet-sln-list` | List the projects a solution holds (dotnet.el dotnet-sln-list, spacemacs SPC m p s l). |
| `:dotnet-sln-new` | Create a solution at a path (dotnet.el dotnet-sln-new, spacemacs SPC m p s n). |
| `:dotnet-goto-sln` | Open the enclosing .sln file (dotnet.el dotnet-goto-sln). |
| `:dotnet-goto-csproj` | Open the enclosing .csproj file (dotnet.el dotnet-goto-csproj). |
| `:dotnet-goto-fsproj` | Open the enclosing .fsproj file (dotnet.el dotnet-goto-fsproj). |
| `:recompile` | Re-run the last compile command (emacs recompile). |
| `:make` | Run `make [args]`, collect errors into the quickfix list, jump to the first (vim :make). |
| `:lmake`, `:lmak` | Run `make [args]` and collect its errors into the location list, jumping to the first (vim :lmake). |
| `:tag`, `:ta` | Jump to the ctags definition of {name} from the tags file, pushing the tag stack (vim :tag). |
| `:tselect`, `:ts` | List every matching tag in a picker; select one to jump (vim :tselect). |
| `:tjump`, `:tj` | Jump to the tag if unique, else show the tag picker (vim :tjump). |
| `:stag`, `:sta` | Open the tag's definition in a new horizontal split and jump there (vim :stag). |
| `:ptag`, `:pt` | Show the tag's definition in the preview window, leaving the cursor where it is (vim :ptag). |
| `:tnext`, `:tn` | Jump to the next matching tag (vim :tnext). |
| `:tprevious`, `:tp`, `:tNext`, `:tN` | Jump to the previous matching tag (vim :tprevious). |
| `:tfirst`, `:trewind`, `:tr` | Jump to the first matching tag (vim :tfirst). |
| `:tlast` | Jump to the last matching tag (vim :tlast). |
| `:pop`, `:po` | Pop the tag stack, returning to where the last :tag jumped from (vim :pop). |
| `:tags` | Show the tag stack depth and the current matching tag (vim :tags). |
| `:messages`, `:mes` | Show the message log — every status/error shown this session (vim :messages). |
| `:Man`, `:man` | Open a man page in the run console (neovim :Man). |
| `:Man-next-manpage`, `:man-next-manpage` | Show the next page of the topic :Man looked up, wrapping at the end (emacs Man-next-manpage). |
| `:Man-previous-manpage`, `:man-previous-manpage` | Show the previous page of the topic :Man looked up, wrapping at the start (emacs Man-previous-manpage). |
| `:redir` | Capture message output to a register/file: :redir @a | > file | >> file | END (vim :redir). |
| `:arduino-compile`, `:averify`, `:arduino-verify` | Compile the sketch with arduino-cli for the selected board; errors go to the compilation list (Arduino IDE Verify). |
| `:arduino-upload`, `:aupload` | Compile and flash the sketch to the connected board (arduino-cli compile --upload), live in a terminal panel. |
| `:arduino-monitor`, `:amonitor`, `:serial-monitor` | Open the serial monitor for the selected port/baud (arduino-cli monitor). |
| `:arduino-compile-verbose`, `:arduino-verify-verbose` | Verbose compile (`arduino-cli compile -v`); diagnostics into *compilation*. |
| `:arduino-compile-clean`, `:arduino-rebuild` | Compile without cached build artifacts (`arduino-cli compile --clean`). |
| `:arduino-compile-jobs`, `:arduino-compile-j` | Compile with N parallel jobs (`arduino-cli compile -j <n>`). |
| `:arduino-compiledb`, `:arduino-compilation-database` | Generate compile_commands.json for the C/C++ LSP (`arduino-cli compile --only-compilation-database`). |
| `:arduino-compile-warnings`, `:arduino-warnings` | Compile at a warning level (`arduino-cli compile --warnings <none|default|more|all>`). |
| `:arduino-compile-profile`, `:arduino-build-profile` | Compile using a sketch build profile (`arduino-cli compile --profile <name>`). |
| `:arduino-compile-debug-opt`, `:arduino-compile-for-debug` | Compile with debug-friendly optimization (`arduino-cli compile --optimize-for-debug`). |
| `:arduino-upload-verify`, `:arduino-upload-verified` | Build + flash, then verify the flashed program (`arduino-cli compile --upload --verify`). |
| `:arduino-upload-programmer`, `:arduino-upload-via-programmer` | Build + flash through a programmer (`arduino-cli compile --upload --programmer <id>`). |
| `:arduino-upload-dir`, `:arduino-upload-input-dir` | Flash a pre-built binary folder without recompiling (`arduino-cli upload --input-dir <dir>`). |
| `:arduino-upload-file`, `:arduino-upload-input-file` | Flash a specific pre-built binary without recompiling (`arduino-cli upload --input-file <file>`). |
| `:arduino-monitor-raw`, `:arduino-monitor-noraw` | Serial monitor without output transformations (`arduino-cli monitor --raw`). |
| `:arduino-monitor-timestamp`, `:arduino-monitor-ts` | Serial monitor prefixing each line with a timestamp (`arduino-cli monitor --timestamp`). |
| `:arduino-board-programmers`, `:arduino-list-programmers` | List programmers the selected board supports (`arduino-cli board details --list-programmers`). |
| `:arduino-board-list-watch`, `:arduino-boards-watch` | Watch for boards connecting/disconnecting (`arduino-cli board list --watch`), live in a panel. |
| `:arduino-lib-list-updatable`, `:arduino-libs-updatable` | Installed libraries with a newer version available (`arduino-cli lib list --updatable`). |
| `:arduino-lib-install-git`, `:arduino-lib-install-url` | Install a library from a git repository (`arduino-cli lib install --git-url <url>`). |
| `:arduino-lib-install-zip`, `:arduino-lib-install-archive` | Install a library from a local .zip (`arduino-cli lib install --zip-path <path>`). |
| `:arduino-compile-quiet`, `:arduino-compile-silent` | Quiet compile, errors only (`arduino-cli compile -q`). |
| `:arduino-compile-properties`, `:arduino-build-properties` | Dump resolved build properties without building (`arduino-cli compile --show-properties`). |
| `:arduino-compile-preprocess`, `:arduino-preprocess` | Output the preprocessed sketch (`arduino-cli compile --preprocess`). |
| `:arduino-compile-dump-profile`, `:arduino-dump-profile` | Print a reproducible build profile for the sketch (`arduino-cli compile --dump-profile`). |
| `:arduino-compile-board-options`, `:arduino-board-options` | Compile with custom board menu options (`arduino-cli compile --board-options <opts>`). |
| `:arduino-compile-build-property`, `:arduino-build-property` | Compile overriding a build property (`arduino-cli compile --build-property <key=value>`). |
| `:arduino-compile-output-dir`, `:arduino-compile-out-dir` | Compile and save build artifacts to a directory (`arduino-cli compile --output-dir <dir>`). |
| `:arduino-upload-verbose`, `:arduino-flash-verbose` | Verbose build + flash (`arduino-cli compile --upload -v`). |
| `:arduino-debug-info`, `:arduino-debug-config` | Print the debugger configuration without starting a session (`arduino-cli debug --info`). |
| `:arduino-debug-programmer`, `:arduino-debug-via-programmer` | Launch the debugger through a programmer (`arduino-cli debug --programmer <id>`). |
| `:arduino-monitor-quiet`, `:arduino-monitor-silent` | Serial monitor suppressing non-error diagnostics (`arduino-cli monitor --quiet`). |
| `:arduino-monitor-describe`, `:arduino-monitor-port-info` | Describe the port's supported monitor settings (`arduino-cli monitor --describe`). |
| `:arduino-core-list-updatable`, `:arduino-cores-updatable` | Installed platforms with a newer version available (`arduino-cli core list --updatable`). |
| `:arduino-core-list-all`, `:arduino-cores-all` | Every installed platform incl. release channels (`arduino-cli core list --all`). |
| `:arduino-lib-list-all`, `:arduino-libs-all` | Installed libraries across all locations incl. built-in (`arduino-cli lib list --all`). |
| `:arduino-update-outdated`, `:arduino-update-show-outdated` | Refresh indexes then list upgradable cores/libraries (`arduino-cli update --show-outdated`). |
| `:arduino-boards-hidden`, `:arduino-board-listall-hidden` | Every known board including platform-hidden variants (`arduino-cli board listall --show-hidden`). |
| `:arduino-lib-search-names`, `:arduino-lib-search-name` | A names-only library search (`arduino-cli lib search <query> --names`). |
| `:arduino-sketch-archive-full`, `:arduino-archive-full` | Archive the sketch with its build output (`arduino-cli sketch archive --include-build-dir`). |
| `:arduino-lib-install-no-deps`, `:arduino-lib-install-nodeps` | Install a library without its declared dependencies (`arduino-cli lib install <name> --no-deps`). |
| `:arduino-board-details-full`, `:arduino-board-info-full` | The complete board detail dump for the selected FQBN (`arduino-cli board details --full`). |
| `:arduino-boards`, `:arduino-board` | Pick the target board (FQBN) from installed platforms (Arduino IDE board selector). |
| `:arduino-ports`, `:arduino-port` | Pick the serial port from connected devices (arduino-cli board list). |
| `:arduino-lib-search`, `:arduino-lib` | Search the Arduino library index and install the pick (Arduino IDE Library Manager). |
| `:arduino-core-install`, `:arduino-core` | Install a board-support core, e.g. `arduino:avr` (Arduino IDE Boards Manager). |
| `:pio-build`, `:pio-run`, `:platformio-build` | Build the PlatformIO project (`pio run`); errors go to the compilation list. |
| `:pio-upload`, `:platformio-upload` | Build and upload the PlatformIO project (`pio run -t upload`), live in a terminal panel. |
| `:pio-exec`, `:platformio-exec`, `:pio-run-exec` | Build and run the native program (`pio run -t exec`); args pass through as --program-arg. |
| `:pio-upload-monitor`, `:platformio-upload-monitor`, `:pio-flash-monitor` | Build, flash, then open the serial monitor in one shot (`pio run -t upload -t monitor`); optional port overrides --monitor-port. |
| `:pio-monitor`, `:platformio-monitor` | Open the PlatformIO serial monitor (`pio device monitor`). |
| `:pio-devices`, `:pio-device-list` | Pick a serial port from `pio device list` and set it for this project. |
| `:pio-device-logical`, `:platformio-device-logical` | List logical (disk) devices (`pio device list --logical`). |
| `:pio-device-mdns`, `:platformio-device-mdns` | List multicast-DNS / network (OTA) devices (`pio device list --mdns`). |
| `:pio-device-serial`, `:platformio-device-serial` | List serial ports only (`pio device list --serial`). |
| `:pio-pkg-list-libraries`, `:platformio-pkg-list-libraries`, `:pio-libs-only` | List installed libraries only (`pio pkg list --only-libraries`). |
| `:pio-pkg-list-platforms`, `:platformio-pkg-list-platforms` | List installed platforms only (`pio pkg list --only-platforms`). |
| `:pio-pkg-list-tools`, `:platformio-pkg-list-tools` | List installed tool packages only (`pio pkg list --only-tools`). |
| `:pio-test-json`, `:platformio-test-json` | Unit-test results as JSON (`pio test --json-output`), shown in a scratch buffer. |
| `:pio-project-config-json`, `:platformio-project-config-json` | Computed project configuration as JSON (`pio project config --json-output`). |
| `:pio-project-metadata-json`, `:platformio-project-metadata-json` | IDE/LSP metadata as JSON (`pio project metadata --json-output`). |
| `:pio-system-info-json`, `:platformio-system-info-json` | System-wide PlatformIO information as JSON (`pio system info --json-output`). |
| `:pio-pkg-search-page`, `:platformio-pkg-search-page` | A specific page of registry search results (`pio pkg search <query> --page <n>`). |
| `:pio-pkg-unpublish-undo`, `:platformio-pkg-unpublish-undo` | Restore a previously unpublished package (`pio pkg unpublish <pkg> --undo`). |
| `:pio-init-no-deps`, `:platformio-init-no-deps` | Scaffold a project without installing dependencies (`pio project init --board <board> --no-install-dependencies`). |
| `:pio-home`, `:platformio-home` | Launch the PlatformIO Home GUI (`pio home`), live in a terminal panel; extra args tune the server. |
| `:pio-init`, `:platformio-init` | Scaffold a PlatformIO project for a board (`pio project init --board <id>`). |
| `:embedded-baud`, `:serial-baud` | Set the serial monitor baud rate for this project (e.g. 9600, 115200). |
| `:arduino-plotter`, `:serial-plotter` | Live-graph the numbers streaming from the serial port (Arduino IDE Serial Plotter). |
| `:pio-plotter`, `:platformio-plotter` | Live-graph the PlatformIO serial monitor output (serial plotter). |
| `:serial-term`, `:serial-terminal` | Open a terminal emulator on a serial port (emacs `serial-term`): `<port> [speed] [line]`. |
| `:arduino-new-sketch`, `:arduino-sketch-new` | Scaffold a new sketch (`arduino-cli sketch new`) and open its .ino (Arduino IDE New Sketch). |
| `:arduino-compile-export`, `:aexport`, `:arduino-export` | Compile and export the built binaries to the sketch folder (Arduino IDE Export Compiled Binary). |
| `:arduino-burn-bootloader`, `:arduino-bootloader` | Flash the bootloader to the selected board (Arduino IDE Burn Bootloader), live in a terminal panel. |
| `:arduino-board-info`, `:arduino-board-details` | Print the selected board's specs and menu options (Arduino IDE Get Board Info). |
| `:arduino-core-search`, `:arduino-boards-manager` | Search the Boards Manager index for a core (Arduino IDE Boards Manager). |
| `:arduino-core-list`, `:arduino-cores` | List installed board-support cores (Boards Manager, installed tab). |
| `:arduino-core-uninstall` | Uninstall a board-support core, e.g. `arduino:avr`. |
| `:arduino-core-update-index`, `:arduino-update-index` | Refresh the Boards Manager package index (arduino-cli core update-index). |
| `:arduino-core-upgrade` | Upgrade all installed board-support cores (arduino-cli core upgrade), live in a terminal panel. |
| `:arduino-lib-list`, `:arduino-libs` | List installed Arduino libraries (Library Manager, installed tab). |
| `:arduino-lib-uninstall` | Uninstall an Arduino library by name (arduino-cli lib uninstall). |
| `:arduino-lib-upgrade` | Upgrade all installed Arduino libraries (arduino-cli lib upgrade), live in a terminal panel. |
| `:arduino-lib-examples` | List a library's example sketches (arduino-cli lib examples <name>). |
| `:arduino-sketch-archive`, `:arduino-archive` | Zip the whole sketch (Arduino IDE Sketch → Archive Sketch). |
| `:pio-clean`, `:platformio-clean` | Remove PlatformIO build artifacts (`pio run -t clean`); output goes to the compilation list. |
| `:pio-test`, `:platformio-test` | Run the PlatformIO project's unit tests (`pio test`); failures go to the compilation list. |
| `:pio-check`, `:platformio-check` | PlatformIO static code analysis (`pio check`); findings go to the compilation list. |
| `:pio-boards`, `:platformio-boards` | The PlatformIO Board Explorer (`pio boards [query]`); output in the Run console. |
| `:pio-boards-json`, `:platformio-boards-json` | The Board Explorer as JSON (`pio boards --json-output [query]`), shown in a scratch buffer. |
| `:pio-lib-install`, `:pio-pkg-install`, `:platformio-lib-install` | Add a library to the PlatformIO project (`pio pkg install -l <name>`), live in a terminal panel. |
| `:pio-lib-list`, `:pio-pkg-list`, `:pio-libs` | List installed PlatformIO project packages (`pio pkg list`). |
| `:pio-lib-uninstall`, `:pio-pkg-uninstall` | Remove a PlatformIO project library (`pio pkg uninstall -l <name>`), live in a terminal panel. |
| `:pio-lib-update`, `:pio-pkg-update` | Update installed PlatformIO project packages (`pio pkg update`), live in a terminal panel. |
| `:arduino-update` | Refresh the core and library indexes together (arduino-cli update). |
| `:arduino-upgrade` | Upgrade all installed cores and libraries (arduino-cli upgrade), live in a terminal panel. |
| `:arduino-outdated` | List cores and libraries with newer versions available (arduino-cli outdated). |
| `:arduino-lib-deps` | Show dependency status for a library (arduino-cli lib deps <name>). |
| `:arduino-config`, `:arduino-config-dump` | Print the active arduino-cli configuration (arduino-cli config dump). |
| `:arduino-debug` | Launch the arduino-cli debugger for the selected board, live in a terminal panel. |
| `:pio-lib-search`, `:pio-pkg-search`, `:platformio-lib-search` | Search the PlatformIO registry for a library (`pio pkg search <query>`). |
| `:pio-lib-outdated`, `:pio-pkg-outdated` | List installed project packages with newer versions available (`pio pkg outdated`). |
| `:pio-lib-show`, `:pio-pkg-show` | Show PlatformIO registry details for a package (`pio pkg show <pkg>`). |
| `:pio-debug`, `:platformio-debug` | Launch the PlatformIO Unified Debugger for the project, live in a terminal panel. |
| `:pio-upgrade`, `:platformio-upgrade` | Upgrade PlatformIO Core itself (`pio upgrade`), live in a terminal panel. |
| `:pio-size`, `:platformio-size` | Show the program size report (`pio run -t size`). |
| `:pio-compiledb`, `:platformio-compiledb` | Generate compile_commands.json for the C/C++ LSP (`pio run -t compiledb`). |
| `:pio-buildfs`, `:platformio-buildfs` | Build the SPIFFS/LittleFS filesystem image (`pio run -t buildfs`). |
| `:pio-uploadfs`, `:platformio-uploadfs` | Flash the filesystem image to the board (`pio run -t uploadfs`), live in a terminal panel. |
| `:pio-uploadeep`, `:platformio-uploadeep` | Flash the EEPROM (`pio run -t uploadeep`, AVR boards), live in a terminal panel. |
| `:pio-bootloader`, `:platformio-bootloader` | Burn the bootloader (`pio run -t bootloader`, AVR boards), live in a terminal panel. |
| `:pio-fuses`, `:platformio-fuses` | Set the microcontroller fuses (`pio run -t fuses`, AVR boards), live in a terminal panel. |
| `:pio-nobuild`, `:platformio-nobuild` | Flash the existing firmware without rebuilding (`pio run -t nobuild`), live in a terminal panel. |
| `:pio-envdump`, `:platformio-envdump` | Dump the resolved build environment (`pio run -t envdump`). |
| `:pio-cleanall`, `:platformio-cleanall` | Remove all build artifacts including dependencies (`pio run -t cleanall`). |
| `:pio-project-config`, `:platformio-project-config` | Show the computed PlatformIO project configuration (`pio project config`). |
| `:pio-project-metadata`, `:platformio-project-metadata` | Dump the IDE/LSP metadata for the project (`pio project metadata`). |
| `:pio-pkg-exec`, `:platformio-pkg-exec` | Run a tool from an installed package (`pio pkg exec -- <argv>`), live in a terminal panel. |
| `:pio-pkg-exec-pkg`, `:platformio-pkg-exec-pkg` | Run a tool from a specific installed package (`pio pkg exec -p <pkg> -- <argv>`), live in a terminal panel. |
| `:pio-platform-install`, `:pio-core-install`, `:platformio-platform-install` | Install a development platform globally (`pio pkg install -g -p <spec>`), live in a terminal panel. |
| `:pio-pkg-pack`, `:platformio-pkg-pack` | Build a tarball of the current package (`pio pkg pack [-o <path>]`), live in a terminal panel. |
| `:pio-pkg-publish`, `:platformio-pkg-publish` | Publish the current package to the PlatformIO registry (`pio pkg publish`); extra args forward --owner/--type/--private/--no-notify. |
| `:pio-project-config-lint`, `:platformio-project-config-lint`, `:pio-lint` | Validate platformio.ini without building (`pio project config --lint`). |
| `:pio-pkg-list-global`, `:platformio-pkg-list-global`, `:pio-pkg-list-g` | Globally installed packages (`pio pkg list -g`). |
| `:pio-pkg-update-global`, `:platformio-pkg-update-global`, `:pio-pkg-update-g` | Update globally installed packages (`pio pkg update -g`), live in a terminal panel. |
| `:pio-pkg-install-skip-deps`, `:platformio-pkg-install-skip-deps` | Install a package without its dependencies (`pio pkg install --skip-dependencies <spec>`). |
| `:pio-test-junit`, `:platformio-test-junit` | Run unit tests and write a JUnit XML report (`pio test --junit-output-path <path>`). |
| `:pio-test-json-path`, `:platformio-test-json-path` | Run unit tests and write a JSON report (`pio test --json-output-path <path>`). |
| `:pio-test-port`, `:platformio-test-port` | Run unit tests over a specific serial port (`pio test --test-port <port>`). |
| `:pio-test-upload-port`, `:platformio-test-upload-port` | Flash the test firmware to a specific serial port (`pio test --upload-port <port>`). |
| `:pio-test-monitor-dtr`, `:platformio-test-monitor-dtr` | Set the DTR line state for the post-test monitor (`pio test --monitor-dtr <0|1>`). |
| `:pio-test-monitor-rts`, `:platformio-test-monitor-rts` | Set the RTS line state for the post-test monitor (`pio test --monitor-rts <0|1>`). |
| `:pio-project-metadata-path`, `:platformio-project-metadata-path` | Write the IDE/LSP metadata JSON to a file (`pio project metadata --json-output-path <path>`). |
| `:pio-check-silent`, `:platformio-check-silent` | Quiet static analysis, warnings/errors only (`pio check -s`). |
| `:pio-upgrade-deps-only`, `:platformio-upgrade-deps-only` | Upgrade only PlatformIO Core's dependencies (`pio upgrade --only-dependencies`). |
| `:pio-init-env-prefix`, `:platformio-init-env-prefix` | Scaffold a project prefixing generated environment names (`pio project init --env-prefix <prefix>`). |
| `:pio-pkg-unpublish`, `:platformio-pkg-unpublish` | Remove a previously published package (`pio pkg unpublish <pkg>`), live in a terminal panel. |
| `:pio-system-info`, `:platformio-system-info` | Show system-wide PlatformIO information (`pio system info`). |
| `:pio-system-prune`, `:platformio-system-prune` | Remove unused PlatformIO caches/packages (`pio system prune -f`), live in a terminal panel. |
| `:pio-prune-cache`, `:platformio-prune-cache` | Prune only cached PlatformIO data (`pio system prune -f --cache`). |
| `:pio-prune-core`, `:platformio-prune-core` | Prune only unnecessary core packages (`pio system prune -f --core-packages`). |
| `:pio-prune-platform`, `:platformio-prune-platform` | Prune only unnecessary development-platform packages (`pio system prune -f --platform-packages`). |
| `:pio-prune-dry-run`, `:platformio-prune-dry-run` | Show what `pio system prune` would remove without deleting (`pio system prune --dry-run`). |
| `:pio-settings-get`, `:platformio-settings-get` | Print PlatformIO Core settings, all or one key (`pio settings get [name]`). |
| `:pio-settings-set`, `:platformio-settings-set` | Change a PlatformIO Core setting (`pio settings set <name> <value>`). |
| `:pio-remote-agent-list`, `:pio-remote-agents`, `:platformio-remote-agent-list` | List active PlatformIO Remote agents (`pio remote agent list`). |
| `:pio-remote-agent-start`, `:platformio-remote-agent-start` | Start a PlatformIO Remote agent (`pio remote agent start [--name <n>] [--share <email>] [--working-dir <dir>]`). |
| `:pio-remote-devices`, `:pio-remote-device-list`, `:platformio-remote-devices` | List serial devices attached to remote agents (`pio remote device list`). |
| `:pio-remote-run`, `:platformio-remote-run` | Build/upload the project via a remote agent (`pio remote run`), live in a terminal panel. |
| `:pio-remote-test`, `:platformio-remote-test` | Run unit tests via a remote agent (`pio remote test`), live in a terminal panel. |
| `:pio-remote-update`, `:platformio-remote-update` | Update platforms/packages/libraries on remote agents (`pio remote update [--dry-run]`). |
| `:pio-account-login`, `:platformio-account-login` | Sign in to a PlatformIO account (`pio account login`), live in a terminal panel. |
| `:pio-account-logout`, `:platformio-account-logout` | Sign out of the PlatformIO account (`pio account logout`). |
| `:pio-account-show`, `:platformio-account-show` | Show the current PlatformIO account information (`pio account show`); extra args forward --offline/--json-output. |
| `:pio-account-token`, `:platformio-account-token` | Print the account auth token (`pio account token [--regenerate] [--json-output] [-p <password>]`). |
| `:pio-env`, `:pio-environment`, `:platformio-env` | Select the PlatformIO build environment (`[env:…]`); no arg picks from platformio.ini, `-` clears. |
| `:pio-list-targets`, `:platformio-list-targets` | Enumerate the project's build targets (`pio run --list-targets`). |
| `:pio-list-tests`, `:platformio-list-tests` | List the project's test suites (`pio test --list-tests`). |
| `:pio-test-filter`, `:platformio-test-filter` | Run only the tests matching a pattern (`pio test -f <pattern>`). |
| `:pio-check-severity`, `:platformio-check-severity` | Static analysis filtered by minimum defect severity (`pio check --severity <low|medium|high>`). |
| `:pio-tool-install`, `:platformio-tool-install` | Install a tool package globally (`pio pkg install -g -t <spec>`), live in a terminal panel. |
| `:pio-boards-installed`, `:platformio-boards-installed` | List boards from installed platforms only (`pio boards --installed`). |
| `:pio-monitor-filter`, `:platformio-monitor-filter` | Add a serial monitor filter for this project (e.g. time, log2file, hexlify, send_on_enter). |
| `:pio-monitor-filters-clear`, `:platformio-monitor-filters-clear` | Remove all configured serial monitor filters. |
| `:pio-monitor-eol`, `:platformio-monitor-eol` | Set the serial monitor end-of-line mode (`CR`, `LF`, or `CRLF`). |
| `:pio-monitor-parity`, `:platformio-monitor-parity` | Set the serial monitor parity (`N`, `E`, `O`, `S`, or `M`). |
| `:pio-monitor-rts`, `:platformio-monitor-rts` | Set the initial RTS line state for the serial monitor (`0` or `1`). |
| `:pio-monitor-dtr`, `:platformio-monitor-dtr` | Set the initial DTR line state for the serial monitor (`0` or `1`). |
| `:pio-monitor-echo`, `:platformio-monitor-echo` | Toggle local echo in the serial monitor (`--echo`). |
| `:pio-monitor-raw`, `:platformio-monitor-raw` | Toggle raw serial monitor mode, disabling output transforms (`--raw`). |
| `:pio-monitor-encoding`, `:platformio-monitor-encoding` | Set the serial monitor encoding (e.g. `UTF-8`, `Latin-1`, `hexlify`); empty resets. |
| `:pio-monitor-flow`, `:platformio-monitor-flow` | Select serial monitor flow control (`none`, `rtscts`, or `xonxoff`). |
| `:pio-monitor-reconnect`, `:platformio-monitor-reconnect` | Toggle serial monitor auto-reconnect (`on` or `off`; `off` = `--no-reconnect`). |
| `:pio-monitor-quiet`, `:platformio-monitor-quiet` | Toggle suppression of non-error serial monitor diagnostics (`--quiet`). |
| `:pio-monitor-exit-char`, `:platformio-monitor-exit-char` | Set the serial monitor exit-char ASCII code (`--exit-char`; default 3 = Ctrl+C); empty resets. |
| `:pio-monitor-menu-char`, `:platformio-monitor-menu-char` | Set the serial monitor menu-char ASCII code (`--menu-char`; default 20 = Ctrl+T); empty resets. |
| `:pio-pkg-show-type`, `:platformio-pkg-show-type` | Registry details scoped to a package type (`pio pkg show --type <library|platform|tool> <pkg>`). |
| `:pio-pkg-exec-call`, `:platformio-pkg-exec-call` | Run a package tool via the call form (`pio pkg exec -c <argv…>`), live in a terminal panel. |
| `:pio-build-verbose`, `:platformio-build-verbose` | Verbose PlatformIO build (`pio run -v`), routed through `*compilation*`. |
| `:pio-build-silent`, `:platformio-build-silent` | Quiet PlatformIO build showing warnings/errors only (`pio run -s`). |
| `:pio-run-jobs`, `:platformio-run-jobs` | Build with N parallel jobs (`pio run -j <n>`). |
| `:pio-build-no-auto-clean`, `:platformio-build-no-auto-clean` | Build without the pre-build clean (`pio run --disable-auto-clean`). |
| `:pio-target`, `:platformio-target` | Run an arbitrary PlatformIO build target (`pio run -t <name>`), live in a terminal panel. |
| `:pio-upload-to`, `:platformio-upload-to` | Build + flash to a specific port (`pio run -t upload --upload-port <port>`). |
| `:pio-test-verbose`, `:platformio-test-verbose` | Verbose PlatformIO unit tests (`pio test -v`). |
| `:pio-test-ignore`, `:platformio-test-ignore` | Run tests except those matching a pattern (`pio test -i <pattern>`). |
| `:pio-test-without-building`, `:platformio-test-without-building` | Test the last build without rebuilding (`pio test --without-building`). |
| `:pio-test-without-uploading`, `:platformio-test-without-uploading` | Run tests without flashing first (`pio test --without-uploading`). |
| `:pio-test-without-testing`, `:platformio-test-without-testing` | Build + upload but skip running the tests (`pio test --without-testing`). |
| `:pio-test-no-reset`, `:platformio-test-no-reset` | Do not reset the board between tests (`pio test --no-reset`). |
| `:pio-check-verbose`, `:platformio-check-verbose` | Verbose static code analysis (`pio check -v`). |
| `:pio-check-json`, `:platformio-check-json` | Static analysis as JSON (`pio check --json-output`), shown in a scratch buffer. |
| `:pio-check-flags`, `:platformio-check-flags` | Static analysis with extra tool flags (`pio check --flags <flags>`). |
| `:pio-check-fail-on`, `:platformio-check-fail-on` | Fail on defects at or above a severity (`pio check --fail-on-defect <low|medium|high>`). |
| `:pio-check-skip-packages`, `:platformio-check-skip-packages` | Analyse only project sources, skipping libraries (`pio check --skip-packages`). |
| `:pio-check-src-filters`, `:platformio-check-src-filters` | Restrict analysis to matching sources (`pio check --src-filters <pattern>`). |
| `:pio-debug-verbose`, `:platformio-debug-verbose` | Verbose PlatformIO debugger session (`pio debug -v`). |
| `:pio-debug-interface`, `:platformio-debug-interface` | Debug with a specific interface (`pio debug --interface <name>`). |
| `:pio-debug-load-mode`, `:platformio-debug-load-mode` | Control firmware reloading on debug start (`pio debug --load-mode <always|modified|manual>`). |
| `:pio-build-conf`, `:platformio-build-conf`, `:pio-build-project-conf` | Build using an alternate platformio.ini (`pio run -c <path>`). |
| `:pio-test-conf`, `:platformio-test-conf` | Run unit tests using an alternate platformio.ini (`pio test -c <path>`). |
| `:pio-check-conf`, `:platformio-check-conf` | Static analysis using an alternate platformio.ini (`pio check -c <path>`). |
| `:pio-debug-conf`, `:platformio-debug-conf` | Debug using an alternate platformio.ini (`pio debug -c <path>`), live in a terminal panel. |
| `:pio-init-ide`, `:platformio-init-ide` | Generate IDE integration files (`pio project init --ide <ide>`). |
| `:pio-init-sample`, `:platformio-init-sample` | Scaffold a project with example code (`pio project init --board <board> --sample-code`). |
| `:pio-init-option`, `:platformio-init-option` | Set a `platformio.ini` option while initialising (`pio project init -O <name=value>`). |
| `:pio-pkg-install-force`, `:platformio-pkg-install-force` | Reinstall a package even if present (`pio pkg install -f <spec>`). |
| `:pio-pkg-install-global`, `:platformio-pkg-install-global` | Install a package globally (`pio pkg install -g <spec>`). |
| `:pio-lib-install-nosave`, `:platformio-lib-install-nosave` | Install a library without writing it to `platformio.ini` (`pio pkg install -l <name> --no-save`). |
| `:pio-pkg-search-sort`, `:platformio-pkg-search-sort` | Registry search with a sort order (`pio pkg search <query> --sort <relevance|popularity|trending|added|updated>`). |
| `:pio-upgrade-dev`, `:platformio-upgrade-dev` | Upgrade PlatformIO Core to the development branch (`pio upgrade --dev`). |
| `:pio-remote-run-force`, `:platformio-remote-run-force` | Force the build to run on the remote agent (`pio remote run -r`). |
| `:pio-remote-agent-start-named`, `:platformio-remote-agent-start-named` | Start a named Remote agent (`pio remote agent start --name <name>`). |
| `:pio-remote-monitor`, `:platformio-remote-monitor` | Serial monitor over a Remote agent (`pio remote device monitor`); extra args forward -p/-b/-f/--eol/--sock etc. |
| `:pio-settings-reset`, `:platformio-settings-reset` | Restore PlatformIO Core settings to their defaults (`pio settings reset`). |
| `:pio-system-completion`, `:platformio-system-completion` | Emit a shell completion script (`pio system completion <bash|zsh|fish|powershell>`). |
| `:pio-ci`, `:platformio-ci` | Build a standalone source tree in an isolated project (`pio ci <src> -b <board>`), live in a terminal panel. |
| `:pio-account-register`, `:platformio-account-register` | Create a new PlatformIO account (`pio account register`), live in a terminal panel. |
| `:pio-account-password`, `:platformio-account-password` | Change the PlatformIO account password (`pio account password`), live in a terminal panel. |
| `:pio-account-update`, `:platformio-account-update` | Update PlatformIO profile information (`pio account update`), live in a terminal panel. |
| `:pio-account-forgot`, `:platformio-account-forgot` | Begin PlatformIO account password recovery (`pio account forgot`), live in a terminal panel. |
| `:pio-account-destroy`, `:platformio-account-destroy` | Permanently destroy the PlatformIO account (`pio account destroy`), live in a terminal panel. |
| `:pio-org-list`, `:platformio-org-list` | List PlatformIO organizations and their members (`pio org list`). |
| `:pio-org-create`, `:platformio-org-create` | Create a new PlatformIO organization (`pio org create <orgname>`), live in a terminal panel. |
| `:pio-org-add`, `:platformio-org-add` | Add an owner to a PlatformIO organization (`pio org add <orgname> <username>`), live in a terminal panel. |
| `:pio-org-remove`, `:platformio-org-remove` | Remove an owner from a PlatformIO organization (`pio org remove <orgname> <username>`), live in a terminal panel. |
| `:pio-org-update`, `:platformio-org-update` | Update a PlatformIO organization (`pio org update <orgname>`), live in a terminal panel. |
| `:pio-org-destroy`, `:platformio-org-destroy` | Destroy a PlatformIO organization (`pio org destroy <orgname>`), live in a terminal panel. |
| `:pio-team-list`, `:platformio-team-list` | List teams in a PlatformIO organization (`pio team list <orgname>`). |
| `:pio-team-create`, `:platformio-team-create` | Create a team in a PlatformIO organization (`pio team create <orgname:team>`), live in a terminal panel. |
| `:pio-team-add`, `:platformio-team-add` | Add a member to a PlatformIO team (`pio team add <orgname:team> <username>`), live in a terminal panel. |
| `:pio-team-remove`, `:platformio-team-remove` | Remove a member from a PlatformIO team (`pio team remove <orgname:team> <username>`), live in a terminal panel. |
| `:pio-team-update`, `:platformio-team-update` | Update a PlatformIO team (`pio team update <orgname:team>`), live in a terminal panel. |
| `:pio-team-destroy`, `:platformio-team-destroy` | Destroy a PlatformIO team (`pio team destroy <orgname:team>`), live in a terminal panel. |
| `:pio-access-list`, `:platformio-access-list` | List published resources and their access levels (`pio access list`). |
| `:pio-access-grant`, `:platformio-access-grant` | Grant access to a resource (`pio access grant <level> <resource> <team>`), live in a terminal panel. |
| `:pio-access-revoke`, `:platformio-access-revoke` | Revoke access to a resource (`pio access revoke <resource> <team>`), live in a terminal panel. |
| `:pio-access-public`, `:platformio-access-public` | Make a published resource public (`pio access public <resource>`), live in a terminal panel. |
| `:pio-access-private`, `:platformio-access-private` | Make a published resource private (`pio access private <resource>`), live in a terminal panel. |
| `:pio`, `:platformio` | Run an arbitrary `pio` command in a terminal panel (any subcommand/flag, including `pio home`). |
| `:arduino-cli`, `:acli`, `:arduino` | Run an arbitrary `arduino-cli` command in a terminal panel (any subcommand/flag). |
| `:arduino-core-download`, `:arduino-download-core` | Fetch a core without installing it (`arduino-cli core download <package>`), live in a terminal panel. |
| `:arduino-lib-download`, `:arduino-download-lib` | Fetch a library without installing it (`arduino-cli lib download <name>`), live in a terminal panel. |
| `:arduino-lib-update-index`, `:arduino-lib-index` | Refresh the arduino-cli library index (`arduino-cli lib update-index`). |
| `:arduino-board-search`, `:arduino-boards-search` | Search the Boards Manager for a board (`arduino-cli board search [query]`). |
| `:arduino-cache-clean`, `:arduino-clean-cache` | Delete the Boards/Library Manager download cache (`arduino-cli cache clean`). |
| `:arduino-completion`, `:arduino-cli-completion` | Emit a shell completion script (`arduino-cli completion <bash|zsh|fish|powershell>`). |
| `:arduino-config-get`, `:arduino-cfg-get` | Read one arduino-cli configuration key (`arduino-cli config get <key>`). |
| `:arduino-config-set`, `:arduino-cfg-set` | Set an arduino-cli configuration value (`arduino-cli config set <key> <value…>`). |
| `:arduino-config-add`, `:arduino-cfg-add` | Append value(s) to an arduino-cli list setting (`arduino-cli config add <key> <value…>`). |
| `:arduino-config-remove`, `:arduino-cfg-remove` | Remove value(s) from an arduino-cli list setting (`arduino-cli config remove <key> <value…>`). |
| `:arduino-config-delete`, `:arduino-cfg-delete` | Delete an arduino-cli settings key and its sub-keys (`arduino-cli config delete <key>`). |
| `:arduino-config-init`, `:arduino-cfg-init` | Write the current configuration to a config file (`arduino-cli config init`). |
| `:arduino-board-attach`, `:arduino-attach` | Attach a sketch to a board (`arduino-cli board attach -p <port> -b <fqbn> [sketch]`), live in a terminal panel. |
| `:arduino-profile-create`, `:arduino-profile-new` | Create/update a build profile in the sketch project file (`arduino-cli profile create`), live in a terminal panel. |
| `:arduino-profile-set-default`, `:arduino-profile-default` | Set the default build profile (`arduino-cli profile set-default <name>`). |
| `:arduino-profile-lib-add`, `:arduino-profile-add-lib` | Add a library to the sketch build profile (`arduino-cli profile lib add <lib>`). |
| `:arduino-profile-lib-remove`, `:arduino-profile-remove-lib` | Remove a library from the sketch build profile (`arduino-cli profile lib remove <lib>`). |
| `:arduino-lib-install`, `:arduino-lib-add` | Install a library by name (`arduino-cli lib install <name>`), live in a terminal panel. |
| `:arduino-daemon`, `:acli-daemon` | Run arduino-cli as a gRPC daemon (`arduino-cli daemon`), live in a terminal panel; extra args tune the server. |
| `:arduino-version`, `:acli-version` | Show the arduino-cli version (`arduino-cli version`; add `--format json`). |
| `:next-error`, `:cnext-error` | Visit the next compilation error's location (emacs next-error / M-g n). |
| `:previous-error`, `:cprevious-error` | Visit the previous compilation error's location (emacs previous-error / M-g p). |
| `:first-error` | Visit the first compilation error's location (emacs first-error). |
| `:abbreviate`, `:ab` | List or define an abbreviation for Insert and Command-line mode (vim :abbreviate). |
| `:list-abbrevs` | Show all defined abbreviations in a buffer (emacs list-abbrevs). |
| `:define-global-abbrev` | Define a global abbreviation: :define-global-abbrev NAME EXPANSION (emacs define-global-abbrev). |
| `:define-mode-abbrev` | Define a major-mode-local abbreviation: :define-mode-abbrev NAME EXPANSION (emacs define-mode-abbrev). |
| `:abbrev-mode` | Toggle abbrev-mode (auto-expand abbrevs on a typed word separator); :abbrev-mode on|off to set (emacs abbrev-mode). |
| `:kill-all-abbrevs` | Remove all defined abbreviations (emacs kill-all-abbrevs). |
| `:iabbrev`, `:ia` | List or define an Insert-mode abbreviation (vim :iabbrev). |
| `:cabbrev`, `:ca` | List or define a Command-line-mode abbreviation (vim :cabbrev). |
| `:noreabbrev`, `:norea` | List or define a non-recursive Insert+Command-line abbreviation (vim :noreabbrev). |
| `:inoreabbrev`, `:inorea` | List or define a non-recursive Insert-mode abbreviation (vim :inoreabbrev). |
| `:cnoreabbrev`, `:cnorea` | List or define a non-recursive Command-line-mode abbreviation (vim :cnoreabbrev). |
| `:unabbreviate`, `:una` | Remove an abbreviation for both modes (vim :unabbreviate). |
| `:iunabbreviate`, `:iuna` | Remove an Insert-mode abbreviation (vim :iunabbreviate). |
| `:cunabbreviate`, `:cuna` | Remove a Command-line-mode abbreviation (vim :cunabbreviate). |
| `:abclear`, `:abc` | Remove all abbreviations for both modes (vim :abclear). |
| `:iabclear`, `:iabc` | Remove all Insert-mode abbreviations (vim :iabclear). |
| `:cabclear`, `:cabc` | Remove all Command-line-mode abbreviations (vim :cabclear). |
| `:bfirst`, `:brewind`, `:brew` | Go to the first buffer in the buffer list (vim :bfirst / :brewind). |
| `:blast`, `:bl` | Go to the last buffer in the buffer list (vim :blast). |
| `:bmodified`, `:bm` | Go to the next modified buffer (vim :bmodified). |
| `:ball`, `:sball`, `:unhide`, `:unh`, `:sunhide`, `:sun` | Open a window for each buffer in the buffer list (vim :ball; :unhide/:sunhide — every zmax buffer is loaded). |
| `:badd` | Add a file to the buffer list without editing it (vim :badd). |
| `:balt` | Add a file to the buffer list and set it as the alternate file (vim :balt). |
| `:bufdo` | Run an Ex command in each listed buffer (vim :bufdo). |
| `:spellwrong`, `:spellw` | Mark words as misspelled (vim :spellwrong). |
| `:spellwrong!`, `:spellw!` | Mark words misspelled in the internal word list (vim :spellwrong!). |
| `:spellrare`, `:spellra` | Flag words as rare (vim :spellrare). |
| `:spellrare!`, `:spellra!` | Flag words as rare in the internal word list (vim :spellrare!). |
| `:spellundo`, `:spellu` | Remove words from the good/bad spell lists (vim :spellundo). |
| `:spellundo!`, `:spellu!` | Remove words from the internal word list only (vim :spellundo!). |
| `:spelldump`, `:spelld` | Open a buffer listing the user's known-good words (vim :spelldump). |
| `:spellinfo`, `:spelli` | Show the spell wordlist location and sizes (vim :spellinfo). |
| `:buffer-close`, `:bc`, `:bclose`, `:bd`, `:bdelete`, `:bun`, `:bunload`, `:bw`, `:bwipe`, `:bwipeout` | Close the current buffer. |
| `:buffer-close!`, `:bc!`, `:bclose!` | Close the current buffer forcefully, ignoring unsaved changes. |
| `:buffer-close-others`, `:bco`, `:bcloseother` | Close all buffers but the currently focused one. |
| `:buffer-close-others!`, `:bco!`, `:bcloseother!` | Force close all buffers but the currently focused one. |
| `:buffer-close-all`, `:bca`, `:bcloseall` | Close all buffers without quitting. |
| `:buffer-close-all!`, `:bca!`, `:bcloseall!` | Force close all buffers ignoring unsaved changes without quitting. |
| `:buffer-next`, `:bn`, `:bnext` | Goto next buffer. |
| `:buffer-previous`, `:bp`, `:bprev`, `:bN`, `:bNext` | Goto previous buffer. |
| `:sbnext`, `:sbn` | Split window and go to the next buffer (vim :sbnext). |
| `:sbprevious`, `:sbp`, `:sbNext`, `:sbN` | Split window and go to the previous buffer (vim :sbprevious / :sbNext). |
| `:sbfirst`, `:sbf`, `:sbrewind`, `:sbr` | Split window and go to the first buffer (vim :sbfirst / :sbrewind). |
| `:sblast`, `:sbl` | Split window and go to the last buffer (vim :sblast). |
| `:sbmodified`, `:sbm` | Split window and go to the next modified buffer (vim :sbmodified). |
| `:sbuffer`, `:sb` | Split window and go to the buffer whose path contains {name} (vim :sbuffer / :sb). |
| `:pedit`, `:ped` | Edit {file} in a preview (horizontal split) window (vim :pedit). |
| `:pbuffer`, `:pb` | Show the buffer whose path contains {name} in a preview (split) window (vim :pbuffer). |
| `:nohlsearch`, `:noh`, `:nohl` | Clear the persistent search highlight (vim :nohlsearch). |
| `:clearjumps` | Clear the current view's jump list (vim :clearjumps). |
| `:project-replace`, `:replace-in-files` | Regex-replace across all matching workspace files (JetBrains Replace in Files). |
| `:buffers`, `:ls`, `:files` | List open buffers in the buffer picker (vim :buffers/:ls/:files). |
| `:jumps` | List the jump list in a picker (vim :jumps). |
| `:oldfiles` | Pick from recently edited files (vim :oldfiles). |
| `:marks` | List the buffer's marks in a picker (vim :marks). |
| `:changes` | List the buffer's changelist in a picker (vim :changes). |
| `:history` | Pick from the command-line history (vim :history). |
| `:delmarks`, `:delm` | Delete the listed named marks (vim :delmarks abc). |
| `:delmarks!`, `:delm!` | Delete all letter marks (vim :delmarks!). |
| `:write`, `:w`, `:sav`, `:saveas` | Write changes to disk. Accepts an optional path (:write some/path.txt) |
| `:write!`, `:w!` | Force write changes to disk creating necessary subdirectories. Accepts an optional path (:write! some/path.txt) |
| `:write-buffer-close`, `:wbc` | Write changes to disk and closes the buffer. Accepts an optional path (:write-buffer-close some/path.txt) |
| `:write-buffer-close!`, `:wbc!` | Force write changes to disk creating necessary subdirectories and closes the buffer. Accepts an optional path (:write-buffer-close! some/path.txt) |
| `:new`, `:n`, `:enew` | Create a new scratch buffer. |
| `:Scratch`, `:scratch` | Open a new scratch buffer, optionally with a language (SPC b S). |
| `:RevealInFinder`, `:reveal-in-finder` | Reveal the current file in the OS file manager (JetBrains Reveal in Finder). |
| `:compose-mail`, `:mail`, `:compose` | Open a message-mode mail draft (emacs compose-mail, C-x m). :compose-mail [to] [subject...] |
| `:compose-mail-other-window` | Open a mail draft in a split (emacs compose-mail-other-window). |
| `:message-send` | Assemble and queue the current mail draft (emacs message-send, C-c C-s). |
| `:message-send-and-exit` | Queue the draft and kill its buffer (emacs message-send-and-exit, C-c C-c). |
| `:message-kill-buffer` | Kill the mail compose buffer (emacs message-kill-buffer, C-c C-k). |
| `:message-goto-to` | Move point to the To: header (emacs message-goto-to, C-c C-f C-t). |
| `:message-goto-subject` | Move point to the Subject: header (emacs message-goto-subject, C-c C-f C-s). |
| `:message-goto-cc` | Move point to the Cc: header (emacs message-goto-cc, C-c C-f C-c). |
| `:message-goto-bcc` | Move point to the Bcc: header (emacs message-goto-bcc, C-c C-f C-b). |
| `:message-goto-body`, `:mail-text` | Move point to the message body (emacs message-goto-body / mail-text, C-c C-b). |
| `:message-insert-signature` | Insert the signature block at point (emacs message-insert-signature, C-c C-w). |
| `:mml-attach-file`, `:mail-add-attachment` | Attach a file as a MIME part (emacs mml-attach-file, C-c C-a). :mml-attach-file <path> |
| `:format`, `:fmt` | Format the file using an external formatter or language server. |
| `:indent-style` | Set the indentation style for editing. ('t' for tabs or 1-16 for number of spaces.) |
| `:line-ending` | Set the document's default line ending. Options: crlf, lf. |
| `:earlier`, `:ear` | Jump back to an earlier point in edit history. Accepts a number of steps or a time span. |
| `:later`, `:lat` | Jump to a later point in edit history. Accepts a number of steps or a time span. |
| `:undotree`, `:undo-tree`, `:UndotreeToggle` | Open the branching undo-history browser (vim undotree). |
| `:undolist`, `:undol` | List the undo states as a text popup (vim :undolist). |
| `:injections`, `:injection-rules` | List the active language-injection rules (defaults + injections.toml). |
| `:injection-info`, `:what-injection` | Report the effective (possibly injected) language at the cursor. |
| `:edit-fragment`, `:edit-injected-fragment` | Edit the injected-language fragment at point in its own buffer (JetBrains Edit Fragment). |
| `:apply-fragment`, `:apply-injected-fragment` | Write the fragment buffer back into its host string. |
| `:inject-language`, `:inject-lang` | Inject a language into the string at point via a /* language=… */ hint (JetBrains inject-here). |
| `:write-quit`, `:wq` | Write changes to disk and close the current view. Accepts an optional path (:wq some/path.txt) |
| `:write-quit!`, `:wq!` | Write changes to disk and close the current view forcefully. Accepts an optional path (:wq! some/path.txt) |
| `:write-all`, `:wa` | Write changes from all buffers to disk. |
| `:write-all!`, `:wa!` | Forcefully write changes from all buffers to disk creating necessary subdirectories. |
| `:write-quit-all`, `:wqa`, `:xa` | Write changes from all buffers to disk and close all views. |
| `:write-quit-all!`, `:wqa!`, `:xa!` | Forcefully write changes from all buffers to disk, creating necessary subdirectories, and close all views (ignoring unsaved changes). |
| `:quit-all`, `:qa`, `:qall`, `:quita`, `:quitall` | Close all views. |
| `:quit-all!`, `:qa!` | Force close all views ignoring unsaved changes. |
| `:cquit`, `:cq` | Quit with exit code (default 1). Accepts an optional integer exit code (:cq 2). |
| `:cquit!`, `:cq!` | Force quit with exit code (default 1) ignoring unsaved changes. Accepts an optional integer exit code (:cq! 2). |
| `:theme`, `:colorscheme`, `:colo` | Change the editor theme (show current theme if no name specified). |
| `:describe-theme` | Show a theme's faces and their fg/bg colors in a scratch buffer (emacs describe-theme). |
| `:disable-theme` | Turn off the active theme, reverting to the default (emacs disable-theme). |
| `:hunk-reset`, `:reset-hunk`, `:hunk-undo` | Undo the git hunk under the cursor, restoring it from HEAD (gitsigns reset_hunk). |
| `:hunk-next`, `:next-hunk` | Move the cursor to the next git hunk. |
| `:hunk-prev`, `:prev-hunk` | Move the cursor to the previous git hunk. |
| `:conflict-ours`, `:diffget-ours`, `:conflict-keep-ours` | Resolve the merge conflict at the cursor by keeping OUR side (HEAD). |
| `:conflict-theirs`, `:diffget-theirs`, `:conflict-keep-theirs` | Resolve the merge conflict at the cursor by keeping THEIR side (incoming). |
| `:conflict-both`, `:conflict-keep-both` | Resolve the merge conflict at the cursor by keeping BOTH sides. |
| `:conflict-next` | Jump to the next merge-conflict marker. |
| `:conflict-prev` | Jump to the previous merge-conflict marker. |
| `:theme-toggle`, `:toggle-theme`, `:light-dark` | Toggle between a dark and light theme (`:theme-toggle [dark] [light]`). |
| `:theme-next` | Switch to the next theme (alphabetical). |
| `:theme-prev` | Switch to the previous theme (alphabetical). |
| `:run`, `:r!` | Run a command in the IDE Run tool window (defaults to `cargo run`). |
| `:search-project`, `:rg` | Search the project (ripgrep) and show jumpable results in the Run console. |
| `:grep`, `:gr` | Run 'grepprg', put the matches in the quickfix list and jump to the first (vim :grep). |
| `:grep!` | Like :grep, but do not jump to the first match. |
| `:grepadd`, `:grepa` | Like :grep, but append the matches to the quickfix list (vim :grepadd). |
| `:lgrep`, `:lgr` | Like :grep, but fill the location list (vim :lgrep). |
| `:lgrepadd`, `:lgrepa` | Like :lgrep, but append to the location list (vim :lgrepadd). |
| `:vimgrep`, `:vimg` | Search files for /{pattern}/, fill the quickfix list and jump to the first match (vim :vimgrep). |
| `:vimgrep!` | Like :vimgrep, but do not jump to the first match. |
| `:vimgrepadd`, `:vimgrepa` | Like :vimgrep, but append the matches to the quickfix list (vim :vimgrepadd). |
| `:lvimgrep`, `:lv`, `:lvim` | Like :vimgrep, but fill the location list (vim :lvimgrep). |
| `:lvimgrepadd`, `:lvimgrepa` | Like :lvimgrep, but append to the location list (vim :lvimgrepadd). |
| `:helpgrep`, `:helpg` | Search the help: open the inline Help browser filtered to {pattern} (vim :helpgrep). |
| `:lhelpgrep`, `:lh` | Location-list variant of :helpgrep — open the Help browser filtered to {pattern} (vim :lhelpgrep). |
| `:copen`, `:cwindow`, `:cw` | Open the quickfix list window. |
| `:cbottom`, `:cbo`, `:cbot`, `:cbott`, `:cbotto` | Scroll the quickfix window to its last entry (vim :cbottom). |
| `:cclose`, `:ccl` | Close the quickfix list window. |
| `:cnext`, `:cn` | Jump to the next quickfix entry. |
| `:cprevious`, `:cprev`, `:cp`, `:cN` | Jump to the previous quickfix entry. |
| `:cfirst`, `:crewind`, `:cr` | Jump to the first quickfix entry. |
| `:clast`, `:cla` | Jump to the last quickfix entry. |
| `:cc` | Jump to the [count]th quickfix entry (or the current one). |
| `:cnfile`, `:cnf` | Jump to the first quickfix entry in the next file. |
| `:cpfile`, `:cpf`, `:cNf`, `:cNfile` | Jump to the last quickfix entry in the previous file. |
| `:cabove`, `:cabo` | Jump to the [count]th quickfix entry above the cursor line. |
| `:cbelow`, `:cbel` | Jump to the [count]th quickfix entry below the cursor line. |
| `:cbefore`, `:cbe` | Jump to the [count]th quickfix entry before the cursor position. |
| `:cafter`, `:caf` | Jump to the [count]th quickfix entry after the cursor position. |
| `:cbuffer`, `:cb` | Read the current buffer as error lines into the quickfix list. |
| `:cgetbuffer` | Read the current buffer into the quickfix list without jumping. |
| `:caddbuffer` | Append the current buffer's error lines to the quickfix list. |
| `:cexpr`, `:cex` | Parse the argument text into the quickfix list and jump to the first entry. |
| `:cgetexpr` | Parse the argument text into the quickfix list without jumping. |
| `:caddexpr` | Append the argument text's entries to the quickfix list. |
| `:cfile`, `:cf` | Read a file of error lines into the quickfix list and jump to the first entry. |
| `:cgetfile` | Read a file of error lines into the quickfix list without jumping. |
| `:lopen`, `:lwindow`, `:lw` | Open the location list window for the current window. |
| `:lbottom`, `:lbo`, `:lbot`, `:lbott`, `:lbotto` | Scroll the location list window to its last entry (vim :lbottom). |
| `:lclose`, `:lcl` | Close the location list window. |
| `:lnext`, `:lne`, `:ln` | Jump to the next location list entry. |
| `:lprevious`, `:lprev`, `:lp`, `:lN` | Jump to the previous location list entry. |
| `:lfirst`, `:lrewind`, `:lr` | Jump to the first location list entry. |
| `:llast`, `:lla` | Jump to the last location list entry. |
| `:ll` | Jump to the [count]th location list entry (or the current one). |
| `:labove`, `:lab` | Jump to the [count]th location list entry above the cursor line. |
| `:lbelow`, `:lbel` | Jump to the [count]th location list entry below the cursor line. |
| `:lbefore`, `:lbe` | Jump to the [count]th location list entry before the cursor position. |
| `:lafter`, `:laf` | Jump to the [count]th location list entry after the cursor position. |
| `:lbuffer`, `:lb` | Read the current buffer as error lines into the location list. |
| `:lgetbuffer` | Read the current buffer into the location list without jumping. |
| `:lexpr`, `:lex` | Parse the argument text into the location list and jump to the first entry. |
| `:lgetexpr` | Parse the argument text into the location list without jumping. |
| `:lfile`, `:lf` | Read a file of error lines into the location list and jump to the first entry. |
| `:lgetfile` | Read a file of error lines into the location list without jumping. |
| `:tabnew`, `:tabe`, `:tabedit` | Open a new tabpage (optionally editing a file). |
| `:tabnext`, `:tabn` | Go to the next tabpage (or tab [count]). |
| `:tabprevious`, `:tabp`, `:tabNext`, `:tabN` | Go to the previous tabpage. |
| `:tabclose`, `:tabc` | Close the current tabpage. |
| `:tab-rename` | Name the current tab, or clear it when given no name (emacs tab-rename). |
| `:tab-switch` | Switch to a tab by name or 1-based number (emacs tab-switch). |
| `:tab-undo` | Reopen the most recently closed tab (emacs tab-undo). |
| `:tab-bar-history-back` | Return to the previously visited tab (emacs tab-bar-history-back). |
| `:tab-bar-history-forward` | Re-visit a tab left via history-back (emacs tab-bar-history-forward). |
| `:tabonly`, `:tabo` | Close all tabpages except the current one. |
| `:tabfirst`, `:tabrewind`, `:tabr` | Go to the first tabpage. |
| `:tablast` | Go to the last tabpage. |
| `:tabmove`, `:tabm` | Move the current tabpage to position [N] (default: last). |
| `:tabdo` | Run an ex-command in every tabpage. |
| `:windo` | Run an ex-command in every window of the current tabpage. |
| `:wincmd` | Run a window (CTRL-W) command by key, e.g. :wincmd h focuses left. |
| `:source`, `:so` | Source a Vimscript file through the embedded vimlrs interpreter. |
| `:runtime`, `:ru` | Source a file from the runtimepath (zmax config dir) via vimlrs (vim :runtime). |
| `:diffthis`, `:difft` | Show the current buffer's changes as a side-by-side diff vs git HEAD (vim :diffthis). |
| `:diffupdate`, `:diffu` | Recompute and redisplay the buffer's diff vs git HEAD (vim :diffupdate). |
| `:diffoff`, `:diffo` | Turn off diff mode: remove the side-by-side diff overlay (vim :diffoff). |
| `:diffsplit`, `:diffs`, `:diffsp`, `:diffspl`, `:diffspli` | Show the differences between this buffer and {file}, side by side (vim :diffsplit). |
| `:diffpatch`, `:diffp`, `:diffpa`, `:diffpat`, `:diffpatc` | Patch a copy of this buffer with {patchfile} (patch(1)) and show the differences (vim :diffpatch). |
| `:match` | Highlight {pattern} in match group 1, or clear it with :match none (vim :match). |
| `:2match` | Highlight {pattern} in match group 2, or clear it with :2match none (vim :2match). |
| `:3match` | Highlight {pattern} in match group 3, or clear it with :3match none (vim :3match). |
| `:helptags`, `:helpt` | Write a `tags` file for the vim-format `*.txt` help files in {dir} (vim :helptags). |
| `:sign`, `:sig` | Define/place/unplace/list/jump gutter signs (vim :sign); e.g. :sign define warn text=>> texthl=WarningMsg then :sign place 1 line=10 name=warn. |
| `:undojoin`, `:undoj` | Join the next change with the previous undo block, so one undo reverts both (vim :undojoin). |
| `:image-mode`, `:image-display`, `:image-toggle-display` | Display the current image file in the terminal (emacs image-mode / image-toggle-display). |
| `:image-rotate` | Rotate the current image 90 degrees and redisplay (emacs image-rotate). |
| `:image-flip-horizontally` | Flip the current image left-to-right and redisplay (emacs image-flip-horizontally). |
| `:image-flip-vertically` | Flip the current image top-to-bottom and redisplay (emacs image-flip-vertically). |
| `:add-file-local-variable` | Set a file-local variable in the Local Variables block (emacs add-file-local-variable). |
| `:add-file-local-variable-prop-line` | Set a file-local variable in the first-line -*- prop line (emacs add-file-local-variable-prop-line). |
| `:delete-file-local-variable` | Remove a file-local variable from the Local Variables block (emacs delete-file-local-variable). |
| `:delete-file-local-variable-prop-line` | Remove a file-local variable from the -*- prop line (emacs delete-file-local-variable-prop-line). |
| `:image-increase-size` | Scale the current image up by 25% and redisplay (emacs image-increase-size). |
| `:image-decrease-size` | Scale the current image down by 20% and redisplay (emacs image-decrease-size). |
| `:image-transform-set-percent` | Set the current image's scale to N percent and redisplay (emacs image-transform-set-percent). |
| `:image-transform-set-scale` | Set the current image's scale from a multiplier, e.g. 1.5 (emacs image-transform-set-scale). |
| `:image-transform-fit-to-window` | Fit the current image to the window (emacs image-transform-fit-to-window). |
| `:image-transform-reset-to-original`, `:image-transform-reset-to-initial` | Drop all rotation/flip/scale transforms and redisplay (emacs image-transform-reset-to-original). |
| `:image-reset-speed` | Reset the current animation's speed to the file's own frame delays, keeping its direction (emacs image-reset-speed). |
| `:image-reverse-speed` | Play the current animation backwards at the same rate (emacs image-reverse-speed). |
| `:image-next-file` | Open the next image file in the directory (emacs image-next-file). |
| `:image-previous-file` | Open the previous image file in the directory (emacs image-previous-file). |
| `:image-goto-frame` | Show frame N (from 1) of a multi-frame image (emacs image-goto-frame). |
| `:image-next-frame` | Switch to the Nth frame after the current one, default 1 (emacs image-next-frame). |
| `:image-previous-frame` | Switch to the Nth frame before the current one, default 1 (emacs image-previous-frame). |
| `:image-increase-speed` | Double the current animated image's playback speed (emacs image-increase-speed). |
| `:image-decrease-speed` | Halve the current animated image's playback speed (emacs image-decrease-speed). |
| `:image-mode-copy-file-name-as-kill` | Copy the image file's path to the clipboard register (emacs image-mode-copy-file-name-as-kill). |
| `:doc-view-mode`, `:doc-view-minor-mode`, `:doc-view-toggle-display` | Render the current document's page in the terminal (emacs doc-view-mode). |
| `:doc-view-next-page`, `:doc-view-scroll-up-or-next-page` | Show the next page of the document (emacs doc-view-next-page). |
| `:doc-view-previous-page`, `:doc-view-scroll-down-or-previous-page` | Show the previous page of the document (emacs doc-view-previous-page). |
| `:doc-view-first-page` | Show the first page of the document (emacs doc-view-first-page). |
| `:doc-view-last-page` | Show the last page of the document (emacs doc-view-last-page). |
| `:doc-view-goto-page` | Show page N of the document (emacs doc-view-goto-page). |
| `:doc-view-enlarge` | Increase the document render resolution and redisplay (emacs doc-view-enlarge). |
| `:doc-view-shrink` | Decrease the document render resolution and redisplay (emacs doc-view-shrink). |
| `:doc-view-open-text` | Extract the document's text into a scratch buffer (emacs doc-view-open-text). |
| `:doc-view-search`, `:doc-view-search-backward` | Search the document's text for a pattern (emacs doc-view-search). |
| `:doc-view-clear-cache`, `:doc-view-kill-proc`, `:doc-view-kill-proc-and-buffer` | Forget the doc-view render state (emacs doc-view-clear-cache). |
| `:doc-view-set-slice` | Crop the displayed page to X Y WIDTH HEIGHT pixels (emacs doc-view-set-slice). |
| `:doc-view-reset-slice` | Drop the doc-view crop slice and show the full page (emacs doc-view-reset-slice). |
| `:append`, `:a` | Insert typed lines after the current line; end input with a line containing only '.' (vim :append). |
| `:insert`, `:i` | Insert typed lines before the current line; end input with a line containing only '.' (vim :insert). |
| `:change`, `:c` | Replace the current line with typed lines; end input with a line containing only '.' (vim :change). |
| `:doautocmd`, `:do`, `:doa` | Fire the autocommands registered for {event} on the current buffer (vim :doautocmd). |
| `:doautoall`, `:doautoa` | Fire the autocommands for {event} on every loaded buffer (vim :doautoall). |
| `:drop`, `:dr` | Jump to a buffer already editing {file}, else edit it (vim :drop). |
| `:lua` | Run a Lua snippet through the system lua interpreter and echo its output (vim :lua). |
| `:luafile`, `:luaf` | Run a Lua script file through the system lua interpreter (vim :luafile). |
| `:perl`, `:pe` | Evaluate a Perl snippet via the embedded stryke interpreter (a Perl superset; vim :perl, in-process, no system perl). |
| `:perlfile`, `:perlf` | Run a Perl script file through the system perl interpreter (vim :perlfile). |
| `:python`, `:py` | Evaluate a Python snippet via the embedded pythonrs interpreter (vim :python; in-process, no system python). |
| `:pyfile`, `:pyf` | Run a Python script file through the system python interpreter (vim :pyfile). |
| `:py3`, `:python3` | Run a Python 3 snippet through the system python3 interpreter and echo its output (vim :py3 / :python3). |
| `:py3file`, `:py3f`, `:python3file` | Run a Python 3 script file through the system python3 interpreter (vim :py3file). |
| `:ruby`, `:rub`, `:rb` | Evaluate a Ruby snippet via the embedded rubylang interpreter (vim :ruby; in-process, no system ruby). |
| `:rubyfile`, `:rubyf` | Run a Ruby script file through the system ruby interpreter (vim :rubyfile). |
| `:tabs` | List the tabpages and switch to the selected one. |
| `:cdo` | Run an ex-command on each quickfix entry. |
| `:cfdo` | Run an ex-command on the first quickfix entry of each file. |
| `:ldo` | Run an ex-command on each location list entry. |
| `:lfdo` | Run an ex-command on the first location entry of each file. |
| `:clist`, `:cl` | Show the quickfix list. |
| `:llist`, `:lli` | Show the location list. |
| `:colder`, `:col` | Go to an older quickfix list. |
| `:cnewer`, `:cnew` | Go to a newer quickfix list. |
| `:chistory`, `:chi` | Show the quickfix list history position. |
| `:lolder`, `:lol` | Go to an older location list (vim :lolder). |
| `:lnewer`, `:lnew` | Go to a newer location list (vim :lnewer). |
| `:lhistory`, `:lhi` | Show the location list history position (vim :lhistory). |
| `:lnfile`, `:lnf` | Jump to the first location-list entry in the next file (vim :lnfile). |
| `:lNfile`, `:lNf` | Jump to the last location-list entry in the previous file (vim :lNfile). |
| `:caddfile` | Append a file of error lines to the quickfix list. |
| `:laddbuffer` | Append the current buffer's error lines to the location list. |
| `:laddexpr` | Append the argument text's entries to the location list. |
| `:laddfile` | Append a file of error lines to the location list. |
| `:lpfile`, `:lpf` | Jump to the last location entry in the previous file. |
| `:shell-quote`, `:sh-quote` | Wrap the selection in safe shell single-quotes. |
| `:wrap-tag` | Wrap each selection in <tag>…</tag>. |
| `:csv-column`, `:csv-col` | Replace the selected CSV/TSV with just its Nth column (1-based). |
| `:code-fence`, `:fence` | Wrap the selection in a fenced Markdown code block with optional language. |
| `:md-table`, `:table-fmt` | Align the selected Markdown pipe table (pad columns, rebuild separator row). |
| `:json-query`, `:json-get` | Replace the selected JSON with the value at a dot-path (e.g. users.0.name). |
| `:json-flatten`, `:json-paths` | Flatten the selected JSON into greppable `path = value` lines. |
| `:json-to-csv`, `:json-csv` | Convert the selected JSON array of objects to CSV (sorted header union). |
| `:json-unflatten`, `:json-unpaths` | Rebuild nested JSON from `path = value` lines (inverse of :json-flatten). |
| `:toml-to-json`, `:toml-json` | Convert the selected TOML to pretty-printed JSON. |
| `:json-to-toml`, `:json-toml` | Convert the selected JSON to pretty-printed TOML. |
| `:json-sort`, `:json-sort-array` | Sort the selected JSON array (optionally by an object field: :json-sort name). |
| `:json-pick`, `:json-select` | Keep only the named keys in the selected JSON object/array (e.g. :json-pick name age). |
| `:json-omit`, `:json-drop` | Drop the named keys from the selected JSON object/array (e.g. :json-omit password). |
| `:json-unique`, `:json-uniq` | Remove duplicate elements from the selected JSON array (optionally by a field). |
| `:json-group-by`, `:json-group` | Group the selected JSON array of objects by a field (e.g. :json-group-by city). |
| `:extract`, `:matches` | Replace the selection with every regex match, one per line (group 1 if present). |
| `:filter`, `:keep-lines`, `:delete-non-matching-lines` | Keep only the selected lines matching a regex (Emacs keep-lines; in-buffer grep). |
| `:reject`, `:remove-lines`, `:flush-lines`, `:delete-matching-lines` | Drop the selected lines matching a regex (Emacs flush-lines; in-buffer grep -v). |
| `:count-matches`, `:count-regex`, `:how-many` | Report how many regex matches (and matching lines) are in the selection (Emacs how-many). |
| `:copy-matching-lines`, `:copy-lines-matching` | Copy lines matching a regex (region, or point to end) to the kill ring (Emacs copy-matching-lines). |
| `:kill-matching-lines`, `:kill-lines-matching` | Delete lines matching a regex (region, or point to end) and save them to the kill ring (Emacs kill-matching-lines). |
| `:uniq-count`, `:frequency` | Collapse the selected lines to `count line`, sorted by frequency (uniq -c | sort -rn). |
| `:stats`, `:describe` | Show count/sum/mean/min/max of the numbers in the selection (non-destructive). |
| `:seq`, `:sequence` | Insert an integer sequence, one per line: :seq <start> <end> [step]. |
| `:field`, `:cut` | Keep only the Nth whitespace field of each selected line (awk '{print $N}'). |
| `:running-total`, `:cumsum` | Replace each numeric line with the cumulative total so far. |
| `:diff-lines`, `:deltas` | Replace each numeric line with its delta from the previous (inverse of running-total). |
| `:sum-column`, `:sumcol` | Sum the Nth whitespace field across the selected lines (non-destructive). |
| `:shuffle`, `:shuf` | Randomly reorder the selected lines (Fisher-Yates). |
| `:sample`, `:random-lines` | Keep N random lines from the selection, preserving order (:sample 10). |
| `:jsonl-to-json`, `:jsonl-json` | Convert the selected JSONL/NDJSON (one value per line) to a JSON array. |
| `:json-to-jsonl`, `:json-jsonl` | Convert the selected JSON array to JSONL (one compact value per line). |
| `:head`, `:first-lines` | Keep only the first N lines of the selection (:head 10). |
| `:tail`, `:last-lines` | Keep only the last N lines of the selection (:tail 10). |
| `:rev`, `:reverse-each-line` | Reverse the characters of each selected line independently (Unix rev). |
| `:json-table`, `:json-tbl` | Render the selected JSON array of objects as an aligned plain-text table. |
| `:hexdump`, `:xxd` | Render the selection as an xxd-style hex dump (offset, hex bytes, ASCII). |
| `:dedup`, `:unique-lines`, `:delete-duplicate-lines` | Remove all duplicate lines globally, keeping first occurrence and order. Emacs delete-duplicate-lines. |
| `:caesar`, `:shift-letters` | Caesar-shift the selection's letters by N (e.g. :caesar 13 = ROT13; N may be negative). |
| `:base32-encode`, `:base32` | Base32-encode the selection (RFC 4648). |
| `:base32-decode`, `:unbase32` | Base32-decode the selection (RFC 4648). |
| `:crc32`, `:checksum` | Show the CRC32 (IEEE) checksum of the selection in hex and decimal (non-destructive). |
| `:rot47`, `:rot-47` | Apply ROT47 to the selection (rotates all printable ASCII; self-inverse). |
| `:morse-encode`, `:morse` | Encode the selection (A-Z, 0-9) to Morse code (words separated by /). |
| `:morse-decode`, `:unmorse` | Decode Morse code in the selection back to text. |
| `:human-bytes`, `:humanize-size` | Convert each numeric line (a byte count) to a human-readable size like 1.5 KiB. |
| `:ordinal`, `:ordinalize` | Convert each numeric line to its ordinal (1 → 1st, 22 → 22nd). |
| `:to-snake`, `:snake-case` | Convert the selected identifier to snake_case. |
| `:to-kebab`, `:kebab-case` | Convert the selected identifier to kebab-case. |
| `:to-camel`, `:camel-case` | Convert the selected identifier to camelCase. |
| `:to-pascal`, `:pascal-case` | Convert the selected identifier to PascalCase. |
| `:to-constant`, `:screaming-snake`, `:upper-snake` | Convert the selected identifier to CONSTANT_CASE. |
| `:to-binary`, `:text-to-binary` | Convert the selection to space-separated 8-bit binary. |
| `:from-binary`, `:binary-to-text` | Convert space-separated binary in the selection back to text. |
| `:natural-sort`, `:sort-natural` | Sort the selected lines in natural order (file2 before file10). |
| `:pad-right`, `:ljust` | Left-justify each selected line, padding with spaces to a minimum width. |
| `:pad-left`, `:rjust` | Right-justify each selected line, padding with spaces to a minimum width. |
| `:json-keys`, `:json-fields` | List the keys of the selected JSON object (or union across an array of objects). |
| `:json-type`, `:json-describe` | Show the JSON type and size of the selection in the status line (non-destructive). |
| `:after`, `:cut-after` | Keep the text after the first <delimiter> on each selected line. |
| `:before`, `:cut-before` | Keep the text before the first <delimiter> on each selected line. |
| `:swapcase`, `:invert-case` | Invert the case of each character in the selection (Hello → hELLO). |
| `:strip-invisible`, `:strip-zero-width` | Remove zero-width / invisible Unicode characters from the selection. |
| `:lines-to-json`, `:lines-to-array` | Wrap the selected lines into a JSON array of strings. |
| `:json-to-lines`, `:array-to-lines` | Unwrap a JSON array in the selection into one line per element. |
| `:checkbox-list`, `:task-list` | Turn the selected lines into a Markdown task list (- [ ] item). |
| `:unwrap-paragraphs`, `:unhardwrap` | Join hard-wrapped lines within each paragraph into single lines. |
| `:sql-in`, `:sql-in-list` | Build a SQL IN-list ('a', 'b', 'c') from the selected lines. |
| `:dec-to-hex`, `:to-hex-num` | Convert each decimal number line to hexadecimal. |
| `:hex-to-dec`, `:from-hex-num` | Convert each hexadecimal number line to decimal. |
| `:unicode-escape`, `:u-escape` | Escape non-ASCII characters in the selection as \u{XXXX}. |
| `:unicode-unescape`, `:u-unescape` | Decode \u{XXXX} and \uXXXX escapes in the selection back to characters. |
| `:sort-by-length`, `:sortlen` | Sort the selected lines by length (shortest first). |
| `:count-unique`, `:distinct-count` | Report the number of distinct vs total selected lines (non-destructive). |
| `:rotate-lines`, `:rotate` | Cyclically rotate the selected lines by N (negative rotates the other way). |
| `:unquote-lines`, `:strip-quotes-lines` | Remove surrounding quotes from each selected line independently. |
| `:quote-lines`, `:quote-each` | Wrap each selected line in double quotes (escaping \ and "). |
| `:repeat`, `:repeat-text` | Repeat the selected text N times (:repeat 3). |
| `:capitalize-lines`, `:capitalize` | Uppercase the first letter of each selected line. |
| `:remove-blank-lines`, `:remove-empty` | Remove all blank (whitespace-only) lines from the selection. |
| `:trim-lines`, `:trim` | Trim leading and trailing whitespace from each selected line. |
| `:kv-to-json`, `:env-to-json` | Convert key=value / key: value lines in the selection to a JSON object. |
| `:json-to-kv`, `:json-to-env` | Convert the selected JSON object to key=value lines. |
| `:json-pluck`, `:json-values-of` | Extract one field's value from each object in a JSON array, one per line. |
| `:to-html-list`, `:html-list` | Convert the selected lines into an HTML <ul> list. |
| `:from-html-list`, `:html-list-to-lines` | Extract <li> item text from an HTML list in the selection, one per line. |
| `:csv-to-html-table`, `:csv-to-html` | Convert the selected CSV/TSV (first row = headers) to an HTML <table>. |
| `:slugify-lines`, `:slug-lines` | Slugify each selected line independently (URL-friendly). |
| `:lines-to-csv-row`, `:join-csv` | Join the selected lines into one CSV row (RFC-4180 quoting). |
| `:csv-row-to-lines`, `:split-csv` | Split a CSV row in the selection into one field per line (quote-aware). |
| `:deslugify`, `:unslugify` | Turn a slug back into a Title Cased phrase (hyphens/underscores to spaces). |
| `:csv-to-tsv`, `:csv-tsv` | Convert the selected CSV to tab-separated values (quote-aware). |
| `:tsv-to-csv`, `:tsv-csv` | Convert the selected TSV to CSV (RFC-4180 quoting). |
| `:strip-line-numbers`, `:unnumber` | Remove a leading line number (and separator) from each selected line. |
| `:markdown-link`, `:md-link` | Wrap the selected text as a Markdown link [text](url). |
| `:extract-urls`, `:urls` | Replace the selection with the http(s) URLs found in it, one per line. |
| `:extract-emails`, `:emails` | Replace the selection with the email addresses found in it, one per line. |
| `:extract-ips`, `:ips` | Replace the selection with the IPv4 addresses found in it, one per line. |
| `:extract-quoted`, `:quoted-strings` | Replace the selection with the contents of double-quoted strings, one per line. |
| `:extract-between`, `:between` | Extract substrings between <start> and <end> delimiters, one per line. |
| `:wrap-with`, `:surround-with` | Wrap the selection with the given text on both sides (:wrap-with **). |
| `:extract-numbers`, `:numbers` | Replace the selection with the numbers found in it, one per line. |
| `:json-validate`, `:json-check` | Report whether the selection is valid JSON (with error location) — non-destructive. |
| `:csv-validate`, `:csv-check` | Check all CSV rows have the same field count (non-destructive). |
| `:ordered-list`, `:numbered-list` | Turn the selected lines into a Markdown ordered list (1. 2. 3.). |
| `:strip-list-markers`, `:unlist` | Strip leading bullet/number/checkbox list markers from each selected line. |
| `:sort-words`, `:sort-fields` | Sort the whitespace-separated words within each selected line. |
| `:unique-words`, `:dedup-words` | Remove duplicate words within each selected line (first occurrence kept). |
| `:sum-fields`, `:row-sum` | Replace each line with the sum of its numeric fields (row total). |
| `:avg-fields`, `:row-avg` | Replace each line with the mean of its numeric fields. |
| `:max-fields`, `:row-max` | Replace each line with the maximum of its numeric fields. |
| `:min-fields`, `:row-min` | Replace each line with the minimum of its numeric fields. |
| `:range-fields`, `:row-range` | Replace each line with the range (max - min) of its numeric fields. |
| `:to-env-export`, `:export-vars` | Prefix each KEY=value line with `export ` (turn a .env into shell exports). |
| `:strip-export`, `:unexport` | Remove a leading `export ` from each selected line. |
| `:dos2unix`, `:crlf-to-lf` | Convert CRLF/CR line endings in the selection to LF. |
| `:unix2dos`, `:lf-to-crlf` | Convert LF line endings in the selection to CRLF. |
| `:percent-of-total`, `:percentages` | Replace each numeric line with its percentage of the column total. |
| `:running-max`, `:cummax` | Replace each numeric line with the running maximum so far. |
| `:running-min`, `:cummin` | Replace each numeric line with the running minimum so far. |
| `:to-fixed`, `:round-to` | Format each numeric line to N decimal places (:to-fixed 2). |
| `:clamp`, `:clip` | Clamp each numeric line to the [min, max] range (:clamp 0 100). |
| `:scale`, `:multiply-by` | Multiply each numeric line by a factor (:scale 1.5). |
| `:offset`, `:add-to-each` | Add N to each numeric line (:offset 10; negative subtracts). |
| `:abs`, `:absolute-value` | Replace each numeric line with its absolute value. |
| `:linkify`, `:auto-link` | Wrap bare URLs in the selection with Markdown link syntax [url](url). |
| `:strip-markdown-links`, `:unlink` | Replace [text](url) Markdown links with just their text. |
| `:strip-emphasis`, `:strip-md-emphasis` | Remove Markdown bold/italic/code emphasis markers from the selection. |
| `:strip-html-comments`, `:strip-comments-html` | Remove <!-- ... --> HTML/Markdown comments from the selection. |
| `:remove-trailing-commas`, `:fix-trailing-commas` | Remove trailing commas before } or ] (JSON5/JS to strict JSON). |
| `:add-trailing-commas`, `:trailing-commas` | Add trailing commas before } or ] (cleaner JS/JSON5 diffs). |
| `:smart-quotes`, `:curly-quotes` | Convert straight quotes to typographic curly quotes (context-aware). |
| `:typographic-dashes`, `:em-dash` | Convert --- to em dash, -- to en dash, ... to ellipsis. |
| `:de-typography`, `:ascii-punctuation` | Normalize curly quotes/dashes/ellipsis back to ASCII punctuation. |
| `:to-ascii`, `:transliterate` | Transliterate accented Latin characters to ASCII (café → cafe). |
| `:nato`, `:phonetic` | Spell the selection in the NATO phonetic alphabet (A → Alfa). |
| `:transpose-grid`, `:transpose-ws` | Transpose a whitespace-separated grid (rows become columns). |
| `:repeat-lines`, `:duplicate-each` | Repeat each selected line N times (:repeat-lines 3). |
| `:rename-word`, `:rename-local` | Rename every whole-word occurrence of the symbol under the cursor in this buffer. |
| `:grep-word`, `:gw`, `:find-references` | Search the project for the whole word under the cursor (jumpable in Run). |
| `:todos`, `:project-todos`, `:fixme` | Scan the whole project for TODO/FIXME/HACK/XXX/BUG/NOTE markers (jumpable in Run). |
| `:registers`, `:reg`, `:display` | Show the contents of all registers. |
| `:yank-join` | Yank joined selections. A separator can be provided as first argument. Default value is newline. |
| `:clipboard-yank` | Yank main selection into system clipboard. |
| `:clipboard-yank-join` | Yank joined selections into system clipboard. A separator can be provided as first argument. Default value is newline. |
| `:primary-clipboard-yank` | Yank main selection into system primary clipboard. |
| `:primary-clipboard-yank-join` | Yank joined selections into system primary clipboard. A separator can be provided as first argument. Default value is newline. |
| `:clipboard-paste-after` | Paste system clipboard after selections. |
| `:clipboard-paste-before` | Paste system clipboard before selections. |
| `:clipboard-paste-replace` | Replace selections with content of system clipboard. |
| `:primary-clipboard-paste-after` | Paste primary clipboard after selections. |
| `:primary-clipboard-paste-before` | Paste primary clipboard before selections. |
| `:primary-clipboard-paste-replace` | Replace selections with content of system primary clipboard. |
| `:show-clipboard-provider` | Show clipboard provider name in status bar. |
| `:change-current-directory`, `:cd`, `:chdir`, `:lcd`, `:lchdir`, `:tcd`, `:tchdir` | Change the current working directory. |
| `:show-directory-stack` | Show the directory stack as a <space> delimited string. |
| `:push-directory`, `:pushd` | Save and then change the current directory. |
| `:pop-directory`, `:popd` | Remove the top entry from the directory stack, and cd to the new top directory.. |
| `:show-directory`, `:pwd` | Show the current working directory. |
| `:encoding` | Set encoding. Based on `https://encoding.spec.whatwg.org`. |
| `:character-info`, `:char`, `:ascii` | Get info about the character under the primary cursor. |
| `:reload`, `:rl` | Discard changes and reload from the source file. |
| `:reload-all`, `:rla` | Discard changes and reload all documents from the source files. |
| `:checktime`, `:checkt` | Reload loaded buffers that changed on disk (vim :checktime). |
| `:filetype`, `:filet` | Report the buffer's detected language / accept on|off|detect|plugin|indent (vim :filetype). |
| `:scriptnames`, `:scr` | List the sourced config files (vim :scriptnames). |
| `:wshada`, `:wsh`, `:wsha`, `:wshad` | Write the registers and every buffer's marks to the shada state file (nvim :wshada; zmax's own line format, not nvim's msgpack). |
| `:rshada`, `:rsh`, `:rsha`, `:rshad` | Read the registers and marks back from the shada state file (nvim :rshada). |
| `:syncbind`, `:sync`, `:syncb`, `:syncbi`, `:syncbin` | Scroll the 'scrollbind' (follow-mode) windows back into step with this one (vim :syncbind). |
| `:uptime`, `:upt`, `:uptim` | Show how long this editor process has been running (nvim :uptime). |
| `:mksession`, `:mks` | Write a session file (cwd + buffers) that :source restores (vim :mksession). |
| `:mkvimrc`, `:mkv` | Write the current runtime mappings to a vimrc file (vim :mkvimrc; mappings only). |
| `:mkexrc`, `:mk` | Write the current runtime mappings to an exrc file (vim :mkexrc; mappings only). |
| `:mkview`, `:mkvie` | Write the current window's view (cursor position) to a file (vim :mkview). |
| `:loadview`, `:lo` | Restore the current window's view by sourcing its :mkview file (vim :loadview). |
| `:swapname`, `:sw` | Show the current buffer's swap file name (vim :swapname). |
| `:preserve`, `:pre` | Flush the buffer to its swap file now (vim :preserve). |
| `:recover`, `:rec` | Replace the buffer with the contents of its swap file (vim :recover). |
| `:wundo`, `:wun` | Write the buffer's undo history to {file} (vim :wundo). |
| `:rundo`, `:rund` | Read the buffer's undo history from {file} (vim :rundo). |
| `:git-stage`, `:stage`, `:git-add` | Stage the current buffer's file (git add). |
| `:vc-root-version-diff`, `:vc-root-diff` | Unified diff of the whole working tree vs a revision, default HEAD (emacs vc-root-version-diff). |
| `:vc-revision-other-window` | Show a past revision of the current file, default HEAD (emacs vc-revision-other-window). |
| `:git-unstage`, `:unstage` | Unstage the current buffer's file (git reset HEAD). |
| `:stash`, `:git-stash` | git stash the working-tree changes (then reload open buffers). |
| `:stash-pop`, `:git-stash-pop` | git stash pop the most recent stash (then reload open buffers). |
| `:update`, `:u` | Write changes only if the file has been modified. |
| `:lsp-workspace-command` | Open workspace command picker |
| `:lsp-restart` | Restarts the given language servers, or all language servers that are used by the current file if no arguments are supplied |
| `:lsp-health`, `:lsp-status` | Show a health report of language servers: which are ready, initializing, or not running, plus each server's supported features |
| `:set`, `:se` | Set options with vim syntax (:set nu, :set nowrap, :set tw=80); no args lists all options. |
| `:setlocal`, `:setl` | Set options for the current buffer only, leaving the global default alone (vim :setlocal). |
| `:setglobal`, `:setg` | Set the global default without changing this buffer's value or behavior (vim :setglobal). |
| `:map` | Map {lhs} to {rhs} in normal+select modes (Vim :map). |
| `:noremap` | Non-recursive :map in normal+select modes. |
| `:nmap` | Map {lhs} to {rhs} in normal mode (Vim :nmap). |
| `:nnoremap` | Non-recursive normal-mode map (Vim :nnoremap). |
| `:imap` | Map {lhs} to {rhs} in insert mode (Vim :imap). |
| `:inoremap` | Non-recursive insert-mode map (Vim :inoremap). |
| `:vmap` | Map {lhs} to {rhs} in select/visual mode (Vim :vmap). |
| `:vnoremap` | Non-recursive select/visual-mode map (Vim :vnoremap). |
| `:xmap` | Map {lhs} to {rhs} in visual mode (Vim :xmap). |
| `:xnoremap` | Non-recursive visual-mode map (Vim :xnoremap). |
| `:smap` | Map {lhs} to {rhs} in select mode (Vim :smap). |
| `:snoremap` | Non-recursive select-mode map (Vim :snoremap). |
| `:omap` | Map {lhs} to {rhs} in operator-pending mode (Vim :omap). |
| `:onoremap` | Non-recursive operator-pending map (Vim :onoremap). |
| `:unmap` | Remove a runtime {lhs} mapping (normal+select). |
| `:nunmap` | Remove a runtime normal-mode {lhs} mapping. |
| `:iunmap` | Remove a runtime insert-mode {lhs} mapping. |
| `:vunmap` | Remove a runtime select/visual-mode {lhs} mapping. |
| `:xunmap` | Remove a runtime visual-mode {lhs} mapping. |
| `:mapclear` | Clear runtime normal+select-mode mappings. |
| `:nmapclear` | Clear runtime normal-mode mappings. |
| `:imapclear` | Clear runtime insert-mode mappings. |
| `:vmapclear` | Clear runtime select/visual-mode mappings. |
| `:xmapclear` | Clear runtime visual-mode mappings (Vim :xmapclear). |
| `:smapclear` | Clear runtime select-mode mappings (Vim :smapclear). |
| `:omapclear` | Clear runtime operator-pending mappings (Vim :omapclear). |
| `:sunmap` | Remove a runtime select-mode {lhs} mapping (Vim :sunmap). |
| `:ounmap` | Remove a runtime operator-pending {lhs} mapping (Vim :ounmap). |
| `:cmap`, `:cm`, `:cma`, `:cnoremap`, `:cno`, `:cnor`, `:cnore` | Map {lhs} to {rhs} in Command-line mode (Vim :cmap/:cnoremap). |
| `:cunmap`, `:cu`, `:cun`, `:cunm` | Remove a Command-line-mode {lhs} mapping (Vim :cunmap). |
| `:cmapclear`, `:cmapc`, `:cmapcl` | Clear all Command-line-mode mappings (Vim :cmapclear). |
| `:lmap`, `:lm`, `:lma`, `:lnoremap`, `:lno`, `:lnor` | Map {lhs} to {rhs} in Lang-Arg mode — the 'keymap' table (Vim :lmap). |
| `:lunmap`, `:lu`, `:lun`, `:lunm` | Remove a Lang-Arg {lhs} mapping (Vim :lunmap). |
| `:lmapclear`, `:lmapc`, `:lmapcl` | Clear all Lang-Arg mappings (Vim :lmapclear). |
| `:describe-input-method`, `:describe-current-input-method` | Show the active (or named) keymap's documentation and key translations (emacs describe-input-method). |
| `:list-input-methods` | List every input method installed in the 'runtimepath' (emacs list-input-methods). |
| `:activate-transient-input-method`, `:transient-input-method` | Enable an input method for the next character only, then disable it (emacs C-x \). |
| `:quail-show-key` | Show which key sequence of the active input method inputs the character after the cursor (emacs quail-show-key). |
| `:toggle-debug-on-error` | Toggle whether a failing command opens a backtrace (emacs toggle-debug-on-error). |
| `:test-buffer` | Run the tests of the current buffer's file (spacemacs SPC m t b). |
| `:tmap`, `:tma`, `:tnoremap`, `:tno`, `:tnor`, `:tnore` | Map {lhs} to {rhs} in Terminal mode (Vim :tmap/:tnoremap). |
| `:tunmap`, `:tunma`, `:tunm` | Remove a Terminal-mode {lhs} mapping (Vim :tunmap). |
| `:tmapclear`, `:tmapc`, `:tmapcl` | Clear all Terminal-mode mappings (Vim :tmapclear). |
| `:loadkeymap`, `:loadk`, `:loadke` | Load the following keymaps until EOF — only in a sourced file (Vim :loadkeymap). |
| `:gui`, `:gu`, `:gvim`, `:gv` | Start the GUI (Vim :gui/:gvim) — zmax is a TUI, so this is E25. |
| `:winsize`, `:wi`, `:win` | Get or set the window size (Vim :winsize) — the editor area, in cells. |
| `:winpos`, `:winp` | Get or set the window position on the screen (Vim :winpos) — E188 in a terminal. |
| `:fclose`, `:fc`, `:fcl` | Close the topmost floating window — picker, popup or panel (Vim :fclose). |
| `:restart` | Restart the editor, re-executing it with the same arguments (Vim :restart). |
| `:diffput`, `:diffpu`, `:dp` | Write the hunk under the selection into the diff base (Vim :diffput). |
| `:filter`, `:filt` | Run a command and show only its output lines matching a pattern (Vim :filter). |
| `:spellrepall`, `:spellr`, `:spellre` | Replace every bad word the last z= replaced, with the same word (Vim :spellrepall). |
| `:menu`, `:me`, `:men` | Add a menu item {path} {rhs} for normal/visual/select/op-pending (Vim :menu); bare :menu lists them. |
| `:noremenu`, `:noreme`, `:norem` | Non-remappable :menu (Vim :noremenu). |
| `:unmenu`, `:unme` | Remove the menu item {path} (`:unmenu *` removes all) (Vim :unmenu). |
| `:amenu`, `:am`, `:ame` | Add a menu item for all modes (Vim :amenu). |
| `:anoremenu`, `:an`, `:anoreme` | Non-remappable :amenu (Vim :anoremenu). |
| `:aunmenu`, `:aun` | Remove an all-mode menu item (Vim :aunmenu). |
| `:nmenu`, `:nme` | Add a normal-mode menu item (Vim :nmenu). |
| `:nnoremenu`, `:nnoreme` | Non-remappable :nmenu (Vim :nnoremenu). |
| `:nunmenu`, `:nunme` | Remove a normal-mode menu item (Vim :nunmenu). |
| `:vmenu`, `:vme` | Add a visual+select-mode menu item (Vim :vmenu). |
| `:vnoremenu`, `:vnoreme` | Non-remappable :vmenu (Vim :vnoremenu). |
| `:vunmenu`, `:vunme` | Remove a visual+select-mode menu item (Vim :vunmenu). |
| `:xmenu`, `:xme` | Add a visual-mode menu item (Vim :xmenu). |
| `:xnoremenu`, `:xnoreme` | Non-remappable :xmenu (Vim :xnoremenu). |
| `:xunmenu`, `:xunme` | Remove a visual-mode menu item (Vim :xunmenu). |
| `:smenu`, `:sme` | Add a select-mode menu item (Vim :smenu). |
| `:snoremenu`, `:snoreme` | Non-remappable :smenu (Vim :snoremenu). |
| `:sunmenu`, `:sunme` | Remove a select-mode menu item (Vim :sunmenu). |
| `:omenu`, `:ome` | Add an operator-pending menu item (Vim :omenu). |
| `:onoremenu`, `:onoreme` | Non-remappable :omenu (Vim :onoremenu). |
| `:ounmenu`, `:ounme` | Remove an operator-pending menu item (Vim :ounmenu). |
| `:imenu`, `:ime` | Add an insert-mode menu item (Vim :imenu). |
| `:inoremenu`, `:inoreme` | Non-remappable :imenu (Vim :inoremenu). |
| `:iunmenu`, `:iunme` | Remove an insert-mode menu item (Vim :iunmenu). |
| `:cmenu`, `:cme` | Add a command-line-mode menu item (Vim :cmenu). |
| `:cnoremenu`, `:cnoreme` | Non-remappable :cmenu (Vim :cnoremenu). |
| `:cunmenu`, `:cunme` | Remove a command-line-mode menu item (Vim :cunmenu). |
| `:tlmenu`, `:tlm` | Add a terminal-mode menu item (Vim :tlmenu). |
| `:tlnoremenu`, `:tln` | Non-remappable :tlmenu (Vim :tlnoremenu). |
| `:tlunmenu`, `:tlu` | Remove a terminal-mode menu item (Vim :tlunmenu). |
| `:tmenu`, `:tm` | Set a menu item's tooltip (Vim :tmenu). |
| `:tunmenu`, `:tu` | Remove a menu item's tooltip (Vim :tunmenu). |
| `:emenu`, `:em` | Execute the menu item {path} (Vim :emenu). |
| `:popup`, `:popu` | Pop up the menu {name} as a picker and run what is chosen (Vim :popup). |
| `:menutranslate`, `:menut` | Translate a menu path component (`:menutranslate clear` drops the table) (Vim :menutranslate). |
| `:echohl`, `:echoh` | Use {group} for the following :echo commands; `:echohl None` stops (vim :echohl). |
| `:mkspell`, `:mksp` | Compile word lists into the spell dictionary {outname} that :set spelllang={outname} then uses (vim :mkspell). |
| `:packdel`, `:packd` | Remove an installed package from the 'packpath' (nvim :packdel). |
| `:packupdate`, `:packu` | Update the installed packages (git pull --ff-only in each) (nvim :packupdate). |
| `:package-menu-filter-upgradable`, `:packupgradable` | List only the installed packages that have updates available (emacs package-menu-filter-upgradable). |
| `:normal`, `:norm`, `:normal!`, `:norm!` | Execute {commands} as normal-mode keystrokes (vim :normal[!]). |
| `:mark`, `:k` | Set mark {x} at the cursor position (vim :mark / :k). |
| `:buffer`, `:buf`, `:b` | Switch to the open buffer whose path contains {name} (vim :buffer / :b). |
| `:resize`, `:res` | Adjust the current window height (vim :resize [+/-]{N}). |
| `:let` | Set a vimscript variable via the embedded interpreter (:let x = 42). |
| `:Files` | Fuzzy-find files with fzf and open the selection (fzf.vim :Files). |
| `:GFiles`, `:GitFiles` | Fuzzy-find git-tracked files with fzf and open the pick (fzf.vim :GFiles). |
| `:Rg`, `:Ag`, `:RG` | Ripgrep the tree with fzf; open the pick at its line (fzf.vim :Rg/:Ag). |
| `:Todo`, `:TODO`, `:Todos` | TODO tool window: ripgrep TODO/FIXME/HACK/XXX across the tree, jump to the pick. |
| `:Locate` | locate(1) files with fzf and open the pick (fzf.vim :Locate). |
| `:BLines` | Fuzzy-search the current buffer's lines with fzf, jump to the pick (fzf.vim :BLines). |
| `:Lines` | Fuzzy-search lines across all open buffers with fzf, open the pick (fzf.vim :Lines). |
| `:History` | Fuzzy-pick a recently opened file with fzf and open it (fzf.vim :History). |
| `:History:` | Fuzzy-pick a past `:` command with fzf and run it (fzf.vim :History:). |
| `:History/` | Fuzzy-pick a past search with fzf and re-run it (fzf.vim :History/). |
| `:Filetypes` | Fuzzy-pick a language with fzf and set the buffer's filetype (fzf.vim :Filetypes). |
| `:Commits` | Fuzzy-pick a repo commit with fzf and show it (fzf.vim :Commits). |
| `:BCommits` | Fuzzy-pick a commit touching the current file with fzf and show it (fzf.vim :BCommits). |
| `:fzf-git-show` | (internal) show a `<sha> subject` fzf pick via git show. |
| `:Jumps` | Fuzzy-pick a jumplist entry with fzf and open it (fzf.vim :Jumps). |
| `:RecentLocations`, `:recent-locations` | Recent Locations (JetBrains): jump ring newest-first, deduped, with context. |
| `:LocalHistory`, `:local-history` | Local History (JetBrains): pick a saved snapshot of this file and open it. |
| `:local-history-open` | (internal) open a `:LocalHistory` snapshot pick. |
| `:Windows` | Fuzzy-pick an open window with fzf and focus it (fzf.vim :Windows). |
| `:Marks` | Fuzzy-pick a mark in the current buffer with fzf and jump to it (fzf.vim :Marks). |
| `:Tags` | Fuzzy-pick a ctags tag across the tree with fzf and jump to it (fzf.vim :Tags). |
| `:BTags` | Fuzzy-pick a ctags tag in the current file with fzf and jump to it (fzf.vim :BTags). |
| `:Snippets` | Fuzzy-pick a snippet with fzf and insert its body (fzf.vim :Snippets). |
| `:Maps` | Fuzzy-browse the current keymaps with fzf (fzf.vim :Maps). |
| `:Helptags` | Fuzzy-pick a help tag with fzf and open the Help browser at it (fzf.vim :Helptags). |
| `:fzf-window` | (internal) focus the Nth window of a `:Windows` fzf pick. |
| `:fzf-snippet` | (internal) insert the body of a `:Snippets` fzf pick. |
| `:fzf-map` | (internal) echo a `:Maps` fzf pick. |
| `:fzf-helptag` | (internal) open the Help browser at a `:Helptags` fzf pick. |
| `:fzf-goto` | (internal) open a `file:line:col:text` fzf pick at the line. |
| `:fzf-line` | (internal) jump to the line of an `N: text` fzf pick. |
| `:Colors` | Fuzzy-pick a colorscheme with fzf (fzf.vim :Colors). |
| `:Buffers` | Fuzzy-pick an open buffer with fzf and switch to it (fzf.vim :Buffers). |
| `:Commands` | Fuzzy-pick a `:` command with fzf and run it (fzf.vim :Commands). |
| `:fold`, `:fo` | Create a fold over the selected/current lines (vim :fold). |
| `:foldopen`, `:foldo` | Open the fold under the cursor (vim :foldopen). |
| `:foldclose`, `:foldc` | Close the fold under the cursor (vim :foldclose). |
| `:&`, `:&&`, `:s-repeat` | Repeat the last :substitute on the current line (vim :& / :&&). |
| `:sleep`, `:sl` | Do nothing for {count} seconds (vim :sleep). |
| `:echomsg`, `:echom` | Echo an expression and save it in the message history (vim :echomsg). |
| `:eval`, `:ev` | Evaluate an expression and discard the result (vim :eval). |
| `:call`, `:cal` | Call a function (vim :call). |
| `:defer`, `:defe` | Call a function when the current function is done; arguments are evaluated now (vim :defer). |
| `:execute`, `:exe` | Execute the string result of an expression as an Ex command (vim :execute). |
| `:const`, `:cons` | Create a variable as a constant (vim :const). |
| `:unlet`, `:unl` | Delete a variable (vim :unlet). |
| `:lockvar`, `:lockv` | Lock variables against :let / :unlet (vim :lockvar [depth] {name}). |
| `:unlockvar`, `:unlo` | Unlock variables locked with :lockvar (vim :unlockvar [depth] {name}). |
| `:ptnext`, `:ptn` | Show the next matching tag in a preview split (vim :ptnext). |
| `:ptprevious`, `:ptp`, `:ptNext`, `:ptN` | Show the previous matching tag in a preview split (vim :ptprevious / :ptNext). |
| `:ptfirst`, `:ptrewind`, `:ptr` | Show the first matching tag in a preview split (vim :ptfirst / :ptrewind). |
| `:ptlast`, `:ptl` | Show the last matching tag in a preview split (vim :ptlast). |
| `:pclose`, `:pc` | Close the preview window (vim :pclose). |
| `:startgreplace`, `:startg` | Start Virtual Replace mode (vim :startgreplace). |
| `:print`, `:p` | Display the selected lines (or current line) in a scratch buffer (vim :print). |
| `:number`, `:nu`, `:#` | Like :print, with line numbers (vim :number / :#). |
| `:list`, `:l` | Like :print, marking each line end with $ (vim :list). |
| `:print-line-number`, `:=` | Echo the last line number of the buffer (vim :=). |
| `:version`, `:ver` | Show the zmax version and compiled feature summary (vim :version). |
| `:intro`, `:int` | Show the introductory message (vim :intro). |
| `:redrawstatus` | Redraw the status line (vim :redrawstatus; approximated by a full redraw). |
| `:redrawtabline` | Redraw the tab line (vim :redrawtabline; approximated by a full redraw). |
| `:silent`, `:sil` | Run {cmd} silently (vim :silent[!]); message suppression is best-effort. |
| `:unsilent` | Run {cmd} with messages shown (vim :unsilent). |
| `:verbose`, `:verb` | Run {cmd} verbosely, optional leading count (vim :verbose). |
| `:noautocmd`, `:noa` | Run {cmd} without triggering autocommands (vim :noautocmd). |
| `:sandbox`, `:san` | Run {cmd} in the sandbox (vim :sandbox; best-effort). |
| `:confirm`, `:conf` | Run {cmd} confirming risky actions (vim :confirm; best-effort). |
| `:browse`, `:bro` | Run {cmd}, picking its file with the file picker when none is given (vim :browse). |
| `:hide`, `:hid` | Run {cmd} keeping the current buffer hidden (vim :hide). |
| `:vertical`, `:vert` | Run {cmd} with vertical split placement (vim :vertical; best-effort). |
| `:horizontal`, `:hor` | Run {cmd} with horizontal split placement (vim :horizontal). |
| `:tab` | Run {cmd} opening its window in a new tab (vim :tab; best-effort placement). |
| `:lsp-stop` | Stops the given language servers, or all language servers that are used by the current file if no arguments are supplied |
| `:tree-sitter-scopes` | Display tree sitter scopes, primarily for theming and development. |
| `:tree-sitter-highlight-name`, `:Inspect` | Display the tree-sitter highlight capture under the cursor (neovim :Inspect). |
| `:tree-sitter-layers` | Display language names of tree-sitter injection layers under the cursor. |
| `:debug-start`, `:dbg` | Start a debug session from a given template with given parameters. |
| `:debug-remote`, `:dbg-tcp` | Connect to a debug adapter by TCP address and start a debugging session from a given template with given parameters. |
| `:debug-eval` | Evaluate expression in current debug context. |
| `:close`, `:clo` | Close the current window (vim :close). Refuses to close the last window. |
| `:only`, `:on` | Close all windows except the current one (vim :only). |
| `:vsplit`, `:vs` | Open the file in a vertical split. |
| `:vsplit-new`, `:vnew` | Open a scratch buffer in a vertical split. |
| `:hsplit`, `:hs`, `:sp`, `:split` | Open the file in a horizontal split. |
| `:hsplit-new`, `:hnew` | Open a scratch buffer in a horizontal split. |
| `:tutor` | Open the tutorial. |
| `:goto`, `:g` | Goto line number. |
| `:set-language`, `:lang`, `:setf`, `:setfiletype` | Set the language of current buffer (show current language if no value specified). |
| `:lpr-buffer`, `:print-buffer` | Print the whole buffer via the external lpr spooler (emacs lpr-buffer). |
| `:lpr-region`, `:print-region` | Print the selected region via the external lpr spooler (emacs lpr-region). |
| `:dictionary-search`, `:dictionary` | Look up a word (or the word at point) with the external dict client (emacs dictionary-search). |
| `:denote`, `:denote-create-note` | Create a timestamped IDENTIFIER--title__keywords.org note in ~/Documents/notes and open it (denote). |
| `:denote-link`, `:denote-insert-link` | Insert an org [[denote:ID][title]] link to a note; with no argument, list the notes (denote-link). |
| `:calendar-hebrew-list-yahrzeits`, `:list-yahrzeit-dates` | List a Hebrew death date's yahrzeit Gregorian dates over N years (emacs calendar-hebrew-list-yahrzeits). |
| `:eldoc-mode`, `:global-eldoc-mode`, `:turn-on-eldoc-mode` | Toggle automatic signature/parameter hints at point (emacs eldoc-mode). |
| `:normal-mode` | Re-detect the buffer's major mode from its file (emacs normal-mode). |
| `:text-mode`, `:fundamental-mode` | Switch the buffer to plain text with no code syntax (emacs text-mode/fundamental-mode). |
| `:set-option` | Set a config option at runtime.<br>For example to disable smart case search, use `:set-option search.smart-case false`. |
| `:toggle-option`, `:toggle` | Toggle a config option at runtime.<br>For example to toggle smart case search, use `:toggle search.smart-case`. |
| `:get-option`, `:get` | Get the current value of a config option. |
| `:move-line-down` | Move the current line down by one (drag down). |
| `:move-line-up` | Move the current line up by one (drag up). |
| `:cycle-case` | Cycle the case style of the symbol under the cursor. |
| `:change-case` | Change the symbol under the cursor to camel|snake|kebab|pascal case. |
| `:left`, `:le` | Left-align line(s), setting leading indent to {n} (default 0) — vim :left. |
| `:right`, `:ri` | Right-align line(s) to width {n} (default 80) — vim :right. |
| `:center`, `:ce` | Center line(s) within width {n} (default 80) — vim :center. |
| `:undo` | Undo the last change (vim :undo). |
| `:redo`, `:red` | Redo the last undone change (vim :redo). |
| `:retab` | Replace tabs with spaces (tab-width per buffer) — vim :retab. |
| `:join`, `:j` | Join the current line(s) with the next, separated by a space (vim :j). |
| `:join!`, `:j!` | Join the current line(s) with the next, no separating space (vim :j!). |
| `:put`, `:pu` | Put (paste) a register's contents as new line(s) below the cursor (vim :put). |
| `:put!`, `:pu!` | Put (paste) a register's contents as new line(s) above the cursor (vim :put!). |
| `:iput`, `:ip` | Put a register's contents below the cursor, indenting to the current line (vim :iput). |
| `:execute-register`, `:@` | Execute a register's contents as Ex command line(s) (vim :@{reg}). |
| `:ijump`, `:ij` | Jump to the first line containing an identifier (vim :ijump). |
| `:djump`, `:dj` | Jump to the first #define of a macro (vim :djump). |
| `:isplit`, `:isp` | Split the window and jump to the first line containing an identifier (vim :isplit). |
| `:dsplit`, `:dsp` | Split the window and jump to the first #define of a macro (vim :dsplit). |
| `:ilist`, `:il` | List every line containing an identifier in a scratch buffer (vim :ilist). |
| `:digraphs`, `:dig` | List the digraph table in a scratch buffer (vim :digraphs). |
| `:z` | Print a window of lines from the cursor into a scratch buffer (vim :z). |
| `:checkpath`, `:checkp` | List the files #included by this buffer that are not found in 'path' (:checkpath! lists all). |
| `:checkpath!` | List every file #included by this buffer, with where it resolved to in 'path'. |
| `:vertical`, `:vert`, `:verti`, `:vertic`, `:vertica` | Run {cmd}; a window it opens is split vertically (vim :vertical). |
| `:horizontal`, `:hor`, `:hori`, `:horiz`, `:horizo`, `:horizon`, `:horizont`, `:horizonta` | Run {cmd}; a window it opens is split horizontally (vim :horizontal). |
| `:tab` | Run {cmd}; where it would open a window, open a new tab page instead (vim :tab). |
| `:silent`, `:sil`, `:sile`, `:silen` | Run {cmd} without its status message (vim :silent). |
| `:silent!`, `:sil!`, `:sile!`, `:silen!` | Run {cmd} without its status message, and swallow its error (vim :silent!). |
| `:unsilent`, `:uns`, `:unsi`, `:unsil`, `:unsile`, `:unsilen` | Run {cmd} with its messages shown, cancelling an enclosing :silent (vim :unsilent). |
| `:noautocmd`, `:noa`, `:noau`, `:noaut`, `:noauto`, `:noautoc`, `:noautocm` | Run {cmd} without firing any autocommand (vim :noautocmd). |
| `:confirm`, `:conf`, `:confi`, `:confir` | Run {cmd} with 'confirm' on: it asks instead of failing on unsaved changes (vim :confirm). |
| `:sandbox`, `:sandb`, `:sandbo` | Run {cmd} in the sandbox: shelling out and writing files are refused (vim :sandbox). |
| `:aboveleft`, `:abo`, `:abov`, `:above`, `:abovel`, `:abovele`, `:abovelef` | Run {cmd}; a window it opens goes above (horizontal) or left (vertical) of this one (vim :aboveleft). |
| `:leftabove`, `:lefta`, `:leftab`, `:leftabo`, `:leftabov` | Run {cmd}; a window it opens goes left (vertical) or above (horizontal) this one (vim :leftabove). |
| `:belowright`, `:bel`, `:belo`, `:below`, `:belowr`, `:belowri`, `:belowrig`, `:belowrigh` | Run {cmd}; a window it opens goes below (horizontal) or right (vertical) of this one (vim :belowright). |
| `:rightbelow`, `:rightb`, `:rightbe`, `:rightbel`, `:rightbelo` | Run {cmd}; a window it opens goes right (vertical) or below (horizontal) this one (vim :rightbelow). |
| `:topleft`, `:to`, `:top`, `:topl`, `:tople`, `:toplef` | Run {cmd}; a window it opens is pushed to the top (horizontal) or far left (vertical) (vim :topleft). |
| `:botright`, `:bo`, `:bot`, `:botr`, `:botri`, `:botrig`, `:botrigh` | Run {cmd}; a window it opens is pushed to the bottom (horizontal) or far right (vertical) (vim :botright). |
| `:keepalt`, `:keepa`, `:keepal` | Run {cmd} without changing the alternate file (vim :keepalt). |
| `:keepjumps`, `:keepj`, `:keepju`, `:keepjum`, `:keepjump` | Run {cmd} without changing the jumplist (vim :keepjumps). |
| `:keepmarks`, `:kee`, `:keep`, `:keepm`, `:keepma`, `:keepmar`, `:keepmark` | Run {cmd} without moving any mark (vim :keepmarks). |
| `:lockmarks`, `:loc`, `:lock`, `:lockm`, `:lockma`, `:lockmar`, `:lockmark` | Run {cmd} without moving any mark, including '[ and '] (vim :lockmarks). |
| `:keeppatterns`, `:keepp`, `:keeppa`, `:keeppat`, `:keeppatt`, `:keeppatte`, `:keeppatter`, `:keeppattern` | Run {cmd} without changing the last search pattern (vim :keeppatterns). |
| `:noswapfile`, `:nos`, `:nosw`, `:noswa`, `:noswap`, `:noswapf`, `:noswapfi`, `:noswapfil` | Run {cmd} without touching the recovery swap file (vim :noswapfile). |
| `:packadd`, `:pa` | Load the package {name} from 'packpath': add it to 'runtimepath' and source its plugin/*.vim (vim :packadd). |
| `:packloadall`, `:packl` | Load every package under pack/*/start/ on the 'packpath' (vim :packloadall). |
| `:language`, `:lan` | Set the locale ($LANG/$LC_*) for the editor and every process it starts; bare form reports it (vim :language). |
| `:log` | Open zmax's log file read-only. |
| `:lsp` | Language-server control: `:lsp info|restart|stop|command`. |
| `:trust` | Trust the current workspace (language servers + local config). `++deny` never prompts, `++remove` revokes (nvim :trust). |
| `:exusage`, `:exu` | List the available Ex commands in a scratch buffer (vim :exusage). |
| `:viusage`, `:viu` | List the Normal-mode commands in a scratch buffer (vim :viusage). |
| `:dlist`, `:dli` | List every #define line of a macro in a scratch buffer (vim :dlist). |
| `:isearch`, `:is` | Echo the first line containing an identifier (vim :isearch). |
| `:dsearch`, `:ds` | Echo the first #define line of a macro (vim :dsearch). |
| `:delete-lines`, `:d`, `:del`, `:delete`, `:kill-whole-line` | Delete the current line(s) into the unnamed register (vim :d / emacs kill-whole-line). |
| `:dl` | Delete the current line(s) and echo the last deleted line in :list format (vim :dl). |
| `:yank-lines`, `:y`, `:ya`, `:yank` | Yank the current line(s) into the unnamed register (vim :y). |
| `:indent-lines` | Indent the current line(s) by one shiftwidth (vim :>). |
| `:dedent-lines` | Dedent the current line(s) by one shiftwidth (vim :<). |
| `:move-lines`, `:m` | Move the current line to after line {address}: :m{addr} (e.g. :m0, :m$, :m.+2). |
| `:copy-lines`, `:t`, `:co`, `:copy` | Copy the current line to after line {address}: :t{addr} (e.g. :t0, :t$). |
| `:global` | Run a command on matching lines: :g/pattern/d (delete). Also :g!/pat/d. |
| `:vglobal` | Run a command on non-matching lines: :v/pattern/d (delete). |
| `:substitute`, `:s` | Substitute: :s/pattern/replacement/[flags]. Also :%s/.../.../g for the whole file. |
| `:smagic` | Substitute forcing 'magic': :smagic/pattern/replacement/[flags] (vim :smagic). |
| `:snomagic` | Substitute forcing 'nomagic' (pattern literal): :snomagic/pattern/replacement/[flags] (vim :snomagic). |
| `:replace-word`, `:rw`, `:subword` | Global replace of the word under the cursor across the file: :replace-word bar → :%s/\bfoo\b/bar/g. Add `i` as a 2nd arg for case-insensitive. |
| `:Subvert`, `:S` | vim-abolish case-preserving substitute: :S/foo/bar/g rewrites foo/Foo/FOO → bar/Bar/BAR. |
| `:Thesaurus`, `:thesaurus` | Look up synonyms for the word under the cursor (or :Thesaurus word) and replace it. |
| `:split-line` | Split the current line at the cursor, keeping the cursor in place. |
| `:just-one-space` | Collapse spaces and tabs around the cursor to a single space. |
| `:delete-horizontal-space` | Delete all spaces and tabs around the cursor (emacs M-\). |
| `:fixup-whitespace` | Collapse whitespace around the cursor to one space, or none by context (emacs fixup-whitespace). |
| `:cycle-spacing` | Cycle the whitespace around the cursor: one space, then none, then restore. |
| `:tabify` | Convert runs of spaces in the region to tabs at tab stops (emacs tabify). |
| `:untabify` | Expand tabs in the region to spaces at the buffer tab width (emacs untabify). |
| `:delete-blank-lines` | Collapse consecutive blank lines down to a single blank line. |
| `:uniquify-lines`, `:uniq` | Delete duplicate lines, keeping the first occurrence. |
| `:reverse`, `:reverse-lines`, `:reverse-region`, `:tac` | Reverse the order of the selected lines (or the whole buffer). Emacs reverse-region. |
| `:uuid`, `:guid` | Insert a random UUID v4 at each cursor (replaces any selection). |
| `:goto-offset`, `:goto-char` | Move the cursor to an absolute character offset. |
| `:goto-byte`, `:go`, `:gob` | Move the cursor to a 1-based byte offset (Vim :goto). |
| `:pad-numbers`, `:zero-pad` | Zero-pad every integer in the selection to <width> digits. |
| `:increment-numbers`, `:incr-numbers` | Add N (default 1; negative to decrement) to every integer in the selection. |
| `:bases`, `:base-info` | Show the selected integer in decimal, hex, octal, and binary. |
| `:lorem`, `:lipsum` | Insert N words (default 30) of lorem-ipsum placeholder text. |
| `:date` | Insert the current UTC date (YYYY-MM-DD) at each cursor. |
| `:datetime`, `:now` | Insert the current UTC date and time (YYYY-MM-DD HH:MM:SS) at each cursor. |
| `:timestamp`, `:epoch` | Insert the current Unix epoch (seconds) at each cursor. |
| `:sum`, `:total` | Sum the numbers in the selection; reports sum/avg/min/max/count in the status line. |
| `:calc`, `:eval-math` | Evaluate an arithmetic expression (+ - * / % ^), or each selection in place. |
| `:join-with`, `:joinw` | Join the selected lines into one with a separator (default ", "). |
| `:split-on`, `:splito` | Split the selected line(s) on a separator (default ",") into one item per line. |
| `:squeeze-blank-lines`, `:squeeze` | Collapse consecutive blank lines in the selection to one (cat -s). |
| `:dedup-adjacent`, `:uniq-adjacent` | Collapse consecutive duplicate lines in the selection (Unix uniq). |
| `:number-lines`, `:nl` | Prepend line numbers to the selected lines (optional start, default 1). |
| `:string-rectangle`, `:string-replace-rectangle` | Replace the selected rectangle's column span with a string on every line (emacs C-x r t). |
| `:string-insert-rectangle` | Insert a string at the rectangle's left column on every selected line, shifting text right (emacs string-insert-rectangle). |
| `:align`, `:tabularize` | Align the selected lines on a delimiter (default `=`) so it shares a column. |
| `:sort-by-field`, `:sortf` | Sort the selected lines by their Nth whitespace field (default 1). |
| `:sort-numeric-fields`, `:sortnf` | Sort the selected lines by the numeric value of their Nth whitespace field (default 1). |
| `:sort-columns`, `:sortc` | Sort the selected lines alphabetically by the character-column range [beg, end). |
| `:sort-paragraphs`, `:sortp` | Sort the paragraphs (blank-line separated blocks) of the selection alphabetically. |
| `:fill-individual-paragraphs` | Fill each paragraph of the selection separately, splitting on indentation changes and using each paragraph's indentation as its fill prefix (emacs fill-individual-paragraphs). |
| `:fill-nonuniform-paragraphs` | Fill each paragraph of the selection separately, splitting only on blank lines and using the smallest indentation of each paragraph as its fill prefix (emacs fill-nonuniform-paragraphs). |
| `:sort-pages` | Sort the ^L-delimited pages in the selection (or buffer) alphabetically (emacs sort-pages). |
| `:sort-lines`, `:sortl` | Sort the selected lines (or the whole buffer) — vim-style line sort. |
| `:highlight-regexp`, `:hi-lock` | Add a persistent highlight for the regexp (emacs highlight-regexp). |
| `:highlight-phrase` | Highlight the phrase, matching across whitespace/line breaks (emacs highlight-phrase). |
| `:highlight-lines-matching-regexp` | Highlight whole lines matching the regexp (emacs highlight-lines-matching-regexp). |
| `:unhighlight-regexp` | Remove the highlight for the regexp, or all highlights if none given (emacs unhighlight-regexp). |
| `:highlight-symbol-at-point`, `:hi-lock-face-symbol-at-point` | Highlight every whole-word occurrence of the symbol under the cursor (emacs highlight-symbol-at-point). |
| `:hi-lock-write-interactive-patterns` | Insert the active Hi-Lock patterns at point as commented Hi-lock: lines (emacs hi-lock-write-interactive-patterns). |
| `:hi-lock-find-patterns` | Activate Hi-Lock highlights from commented Hi-lock: lines at the top of the buffer (emacs hi-lock-find-patterns). |
| `:outline-hide-by-heading-regexp` | Fold the subtree of every heading whose line matches the regexp (emacs outline-hide-by-heading-regexp). |
| `:outline-show-by-heading-regexp` | Reveal the subtree of every heading whose line matches the regexp (emacs outline-show-by-heading-regexp). |
| `:set-fill-column` | Set the fill width to N, or the current cursor column if omitted (emacs set-fill-column). |
| `:customize`, `:customize-browse` | Open Preferences on the Settings tab (emacs customize / customize-browse). |
| `:customize-variable`, `:customize-option` | Open Settings pre-filtered to a variable name (emacs customize-variable). |
| `:customize-group` | Open Settings pre-filtered to a group name (emacs customize-group). |
| `:customize-apropos` | Open Settings pre-filtered to a regexp/substring (emacs customize-apropos). |
| `:customize-unsaved`, `:customize-changed`, `:customize-saved` | Open Settings showing only options changed from their default (emacs customize-unsaved). |
| `:customize-face`, `:customize-themes`, `:customize-create-theme` | Open the Color Scheme (theme/face editor) tab (emacs customize-face / customize-themes). |
| `:set-face-foreground` | Set a theme face's foreground color: :set-face-foreground <face> <color> (emacs set-face-foreground). |
| `:set-face-background` | Set a theme face's background color: :set-face-background <face> <color> (emacs set-face-background). |
| `:set-fringe-style` | Set the gutter (fringe) style: none clears it, default restores it (emacs set-fringe-style). |
| `:set-right-margin` | Re-fill the region to N columns narrower than text-width (emacs set-right-margin). |
| `:write-region` | Write the region (or whole buffer) to a file, overwriting it (emacs write-region). |
| `:append-to-file` | Append the region (or whole buffer) to the end of a file (emacs append-to-file). |
| `:set-justification-left` | Flush the region's lines to the left margin (emacs set-justification-left). |
| `:set-justification-right` | Right-justify the region's lines to the fill width (emacs set-justification-right). |
| `:set-justification-center` | Centre the region's lines within the fill width (emacs set-justification-center). |
| `:set-justification-full` | Justify the region's lines to both margins (emacs set-justification-full). |
| `:set-justification-none` | Turn justification off for the region (emacs set-justification-none). |
| `:set-left-margin` | Set the region's left margin to a column of spaces (emacs set-left-margin). |
| `:increase-left-margin` | Indent the region by standard-indent columns (emacs increase-left-margin). |
| `:decrease-left-margin` | Outdent the region by standard-indent columns (emacs decrease-left-margin). |
| `:append-to-buffer` | Insert the region at the end of another open buffer (emacs append-to-buffer). |
| `:prepend-to-buffer` | Insert the region at the start of another open buffer (emacs prepend-to-buffer). |
| `:copy-to-buffer` | Replace another open buffer's contents with the region (emacs copy-to-buffer). |
| `:rename-buffer` | Give the current buffer an explicit display name (emacs rename-buffer). |
| `:rename-uniquely` | Rename the current buffer to a unique name with a numeric suffix (emacs rename-uniquely). |
| `:desktop-save` | Save file-visiting buffers and point to a desktop file (emacs desktop-save). |
| `:desktop-read` | Reopen the buffers recorded in a desktop file (emacs desktop-read). |
| `:desktop-change-dir` | Switch to the desktop saved in another directory (emacs desktop-change-dir). |
| `:desktop-revert` | Re-read the last loaded desktop (emacs desktop-revert). |
| `:desktop-clear` | Close all buffers and erase the desktop file (emacs desktop-clear). |
| `:expand-region-abbrevs` | Expand every abbrev found in the region (emacs expand-region-abbrevs). |
| `:getenv` | Report the value of an environment variable (emacs getenv). |
| `:setenv` | Set (or, with no value, unset) an environment variable (emacs setenv). |
| `:apropos-command` | List commands whose name matches a regexp (emacs apropos-command). |
| `:apropos` | List commands (by name or doc) and config variables matching a regexp (emacs apropos). |
| `:apropos-documentation` | List commands whose documentation matches a regexp (emacs apropos-documentation). |
| `:apropos-variable` | List config variables whose name matches a regexp (emacs apropos-variable). |
| `:apropos-user-option` | List user-customizable options whose name matches a regexp (emacs apropos-user-option). |
| `:apropos-value` | List config variables whose value matches a regexp (emacs apropos-value). |
| `:goto-line-relative` | Go to a line counting from the narrowed region start (emacs goto-line-relative). |
| `:emacs-version` | Report the running editor version (emacs emacs-version). |
| `:add-name-to-file` | Hard-link the current buffer's file to a new name (emacs add-name-to-file). |
| `:browse-url` | Open a URL in the OS default browser (emacs browse-url). |
| `:set-visited-file-name` | Change the file the current buffer is visiting (emacs set-visited-file-name). |
| `:make-symbolic-link` | Create a symbolic link (emacs make-symbolic-link): <target> <linkname>, or <linkname> for the current file. |
| `:set-file-modes` | Set the mode bits of a file, chmod-style, MODE as octal (emacs set-file-modes). |
| `:copy-directory` | Recursively copy a directory to a destination (emacs copy-directory). |
| `:insert-file-literally` | Insert the raw contents of a file at point (emacs insert-file-literally). |
| `:list-directory` | Show a listing of a directory in a scratch buffer (emacs list-directory). |
| `:write-abbrev-file` | Write every abbrev to the named file (emacs write-abbrev-file). |
| `:read-abbrev-file` | Read abbrevs from the named file, merging them into the table (emacs read-abbrev-file). |
| `:bookmark-write` | Write every bookmark to the named file (emacs bookmark-write). |
| `:bookmark-load` | Load bookmarks from the named file, merging them into the store (emacs bookmark-load). |
| `:multi-occur-in-matching-buffers`, `:multi-occur` | List matches for SEARCH-REGEXP across all buffers whose name matches BUFFER-REGEXP (emacs multi-occur-in-matching-buffers). |
| `:transpose-regions` | Swap two char ranges: START1 END1 START2 END2 (emacs transpose-regions). |
| `:transpose-words` | Transpose the word before the cursor with the word after it. |
| `:transpose-chars` | Transpose the two characters around the cursor. |
| `:duplicate-line`, `:dup` | Duplicate the current line below. |
| `:delete-trailing-whitespace`, `:dtw` | Delete trailing whitespace from every line in the buffer. |
| `:sort` | Sort ranges in selection. |
| `:reflow` | Hard-wrap the current selection of lines to a given width. |
| `:tree-sitter-subtree`, `:ts-subtree`, `:InspectTree` | Display the smallest tree-sitter subtree that spans the primary selection (neovim :InspectTree). |
| `:config-reload` | Refresh user config. |
| `:keymap` | Switch the active keymap preset: spacemacs, vim, helix, or emacs. |
| `:config-open` | Open the user config.toml file. |
| `:config-open-workspace` | Open the workspace config.toml file. |
| `:init-open`, `:init-el-open` | Open the Emacs Lisp init file (`<config-dir>/init.el`) sourced at startup. |
| `:log-open` | Open the zmax log file. |
| `:insert-output` | Run shell command, inserting output before each selection. |
| `:append-output` | Run shell command, appending output after each selection. |
| `:pipe`, `:\|` | Pipe each selection to the shell command. |
| `:pipe-to` | Pipe each selection to the shell command, ignoring output. |
| `:run-shell-command`, `:sh`, `:!` | Run a shell command |
| `:elisp`, `:eval-expression`, `:el` | Evaluate an Emacs Lisp expression against the editor, asking for it when none is given (embedded elisprs). |
| `:vim`, `:viml`, `:vimscript` | Evaluate a Vimscript (VimL) expression via the embedded vimlrs interpreter. |
| `:awk`, `:awk-filter` | Filter the selection (or whole buffer) through an awk program (embedded awkrs). |
| `:zsh`, `:zshell` | Run a command in the embedded zsh shell (state persists); output shown in a popup. |
| `:stryke`, `:st` | Evaluate stryke (strykelang) source via the embedded interpreter (state persists). |
| `:php` | Evaluate PHP source via the embedded phplang interpreter. |
| `:node`, `:js`, `:javascript` | Evaluate JavaScript source via the embedded node-js interpreter. |
| `:arb`, `:arb-filter` | Filter the selection (or whole buffer) through an arb spec's `out` pipeline (embedded arblang). |
| `:zwire-host`, `:zh` | Send a raw JSON request to the zwire-host daemon; show the reply (e.g. {"cmd":"hostinfo"}). |
| `:zwire-sysinfo`, `:zsys` | Show live system stats (cpu/mem/load) from the shared zwire-host daemon. |
| `:zwire-hostinfo` | Show machine facts (os/arch/cpus/hostname) from the shared zwire-host daemon. |
| `:zwire-exec`, `:zx` | Run a command through the zwire-host daemon and insert its output at the cursor. |
| `:zwire-crawl`, `:zwc` | Recursively crawl the filesystem via zwire-host and insert matching paths at the cursor. |
| `:zwire-job`, `:zj` | Ship a long-running command to the zwire-host daemon; get notified on the status line when it finishes. |
| `:zwire-jobs`, `:jobs`, `:zjs` | List background zwire-host jobs still running, plus recent completions. |
| `:zwire-job-output`, `:zjo` | Insert a finished background job's output at the cursor (most recent, or a given id). |
| `:plugin` | Manage native (compiled Rust) plugins: `:plugin load <path>…`, `:plugin unload <name>…`, `:plugin list`. |
| `:repl` | Open the embedded-language REPL (elisp/viml/stryke/awk/zsh); optional starting language. |
| `:reset-diff-change`, `:diffget`, `:diffg` | Reset the diff change at the cursor position. |
| `:clear-register` | Clear given register. If no argument is provided, clear all registers. |
| `:set-register` | Set contents of the given register. |
| `:redraw` | Clear and re-render the whole UI |
| `:profile`, `:prof` | Profile functions and scripts (start|func|file|pause|stop|dump) — maps to the command profiler |
| `:profdel`, `:profd` | Stop profiling a function or script (halts the profiler, keeping samples) |
| `:scriptencoding`, `:scripte` | Declare the encoding used in a sourced script (zmax reads scripts as UTF-8) |
| `:org-export`, `:org-export-dispatch` | Export the current Org buffer to Markdown (scratch buffer, or to a path arg) — Emacs C-c C-e |
| `:editorconfig`, `:editorconfig-mode` | Apply the nearest .editorconfig settings to the current buffer (Emacs editorconfig-mode) |
| `:eww`, `:browse-web` | Fetch a URL and render the HTML to text in a buffer (Emacs eww, in-editor web browser) |
| `:eww-open-file` | Render a local HTML file in the eww buffer (Emacs eww-open-file) |
| `:eww-search-words`, `:eww-search` | Web-search the given words (or word under cursor) and render results (Emacs eww-search-words) |
| `:translate`, `:google-translate` | Translate the word under cursor (or given text) between the configured languages |
| `:translate-set-languages` | Set source and target languages for :translate (e.g. en fr, or auto en) |
| `:translate-reverse` | Reverse the source and target translate languages |
| `:irc-connect`, `:erc`, `:irc` | Connect to an IRC server and register: :irc-connect <host[:port]> <nick> (Emacs erc/rcirc) |
| `:irc-join` | Join an IRC channel on the active session: :irc-join #channel |
| `:irc-say`, `:irc-msg` | Send a message on the active IRC session: :irc-say <target> <text> |
| `:irc-view`, `:irc-buffer` | Show the current IRC session transcript in a buffer (Emacs erc buffer) |
| `:irc-quit` | Disconnect the active IRC session: :irc-quit [message] |
| `:ietf-docs-open`, `:ietf` | Open an IETF document (rfc2119, draft-…, std66) named by the argument or the word under the cursor; `!` re-downloads (spacemacs ietf layer). |
| `:slack-start` | Authenticate a Slack token and start a session: :slack-start [token] (else $SLACK_TOKEN). |
| `:slack-select-rooms`, `:slack-channel-select` | List the Slack team's rooms, or select one: :slack-select-rooms [#channel|@user]. |
| `:slack-buffer` | Show the selected Slack room's recent history: :slack-buffer [count]. |
| `:slack-message` | Post a message to the selected Slack room: :slack-message <text>. |
| `:slack-quit` | Close the Slack session (emacs-slack slack-ws-close). |
| `:sudo-edit` | Open a file with elevated privileges, reading it through sudo when it is root-only: :sudo-edit [file] (spacemacs SPC f E). |
| `:sudo-write` | Write the current buffer to a root-owned file through sudo tee: :sudo-write [file]. |
| `:regenerate-tags`, `:projectile-regenerate-tags` | Rebuild the project's TAGS file with `ctags -Re` and visit it (projectile-regenerate-tags, SPC p G). |
| `:syntime` | Profile syntax highlighting: :syntime on|off|clear|report. |
| `:move`, `:mv` | Move the current buffer and its corresponding file to a different path |
| `:move!`, `:mv!` | Move the current buffer and its corresponding file to a different path creating necessary subdirectories |
| `:delete-file`, `:remove-file` | Delete the current buffer's file from disk and close the buffer (vim-eunuch :Delete). |
| `:chmod-x`, `:chmodx`, `:make-executable` | Make the current file executable (chmod a+x). Unix only. |
| `:mkdir` | Create a directory and any missing parents; with no arg, the current file's parent. |
| `:yank-diagnostic` | Yank diagnostic(s) under primary cursor to register, or clipboard by default |
| `:read`, `:r` | Load a file into buffer, or the output of a shell command with `:r !cmd` |
| `:insert-file` | Insert a file's contents at the cursor, asking for the file when none is given (emacs insert-file). |
| `:insert-buffer` | Insert another open buffer's contents at the cursor, asking for the buffer when none is given (emacs insert-buffer). |
| `:echo` | Prints the given arguments to the statusline. |
| `:echoerr`, `:echoe` | Prints the given arguments to the statusline as an error (vim :echoerr). |
| `:noop` | Does nothing. |
| `:workspace-trust` | Allow language servers and local config for the current workspace. |
| `:workspace-untrust` | Revoke the current workspace's trust grant or exclusion. |
| `:workspace-exclude` | Mark the current workspace as never-prompt. Never prompts for trust again. |
| `:stjump`, `:stj` | Jump to a tag in a new split, offering a picker when several tags match (vim :stjump). |
| `:ptjump`, `:ptj` | Like :tjump, showing the tag in the preview window and leaving the cursor where it is (vim :ptjump). |
| `:stselect`, `:sts` | List matching tags in a picker; the chosen one opens in a new split (vim :stselect). |
| `:ptselect`, `:pts` | Like :tselect, showing the chosen tag in the preview window — a split here (vim :ptselect). |
| `:ppop`, `:pp` | Pop the tag stack into the preview window — a split here (vim :ppop). |
| `:ltag`, `:lt` | Jump to a tag and put every matching tag in the location list (vim :ltag). |
| `:psearch`, `:ps` | Show the first line matching an identifier in the preview window, keeping focus (vim :psearch). |
| `:perldo`, `:perld` | Run a Perl snippet on every line ($_ is the line, and replaces it) (vim :perldo). |
| `:rubydo`, `:rubyd` | Run a Ruby snippet on every line ($_ is the line, and replaces it) (vim :rubydo). |
| `:luado`, `:luad` | Run a Lua chunk on every line (args line, linenr; the return value replaces the line) (vim :luado). |
| `:pydo`, `:pyd` | Run a Python function body on every line (args line, linenr; the return value replaces the line) (vim :pydo). |
| `:py3do`, `:py3d`, `:pyxdo`, `:pyxd` | Run a Python 3 function body on every line (args line, linenr) (vim :py3do / :pyxdo). |
| `:pyx`, `:pythonx` | Run a Python snippet through python3 and echo its output (vim :pyx / :pythonx). |
| `:pyxfile`, `:pyxf` | Run a Python script file through python3 (vim :pyxfile). |
| `:command`, `:com` | Define a user command (:command Ll :lopen), or list the ones defined (vim :command). |
| `:command!`, `:com!` | Define a user command, replacing an existing definition of that name (vim :command!). |
| `:delcommand`, `:delc` | Delete a user-defined command (vim :delcommand). |
| `:comclear`, `:comc` | Delete every user-defined command (vim :comclear). |
| `:folddoopen`, `:foldd` | Run a command on every line that is not inside a closed fold (vim :folddoopen). |
| `:folddoclosed`, `:folddoc` | Run a command on every line inside a closed fold (vim :folddoclosed). |
| `:breakadd`, `:breaka` | Set a debugger breakpoint: :breakadd here | :breakadd file [lnum] [file] (vim :breakadd). |
| `:breakdel`, `:breakd` | Delete a debugger breakpoint by :breaklist number, by position, or all with * (vim :breakdel). |
| `:breaklist`, `:breakl` | List the debugger breakpoints, numbered for :breakdel (vim :breaklist). |
| `:debug`, `:deb`, `:debu` | Run an Ex command under the script debugger prompt (vim :debug). |
| `:debuggreedy`, `:debugg`, `:debuggr`, `:debuggre`, `:debuggree`, `:debuggreed` | Read debug mode commands from the normal input stream instead of the prompt (vim :debuggreedy). |
| `:0debuggreedy`, `:0debugg` | Undo :debuggreedy — debug mode commands are typed at the prompt again (vim :0debuggreedy). |
| `:highlight`, `:hi` | List, show or set the theme's highlight groups (:hi Comment guifg=#5c6370 gui=italic) (vim :highlight). |
| `:syntax`, `:sy`, `:syn` | Turn the buffer's syntax highlighting on/off, or report the language (vim :syntax). |
| `:compiler`, `:comp` | Select the compiler for :make by setting makeprg (:compiler cargo) (vim :compiler). |
| `:checkhealth`, `:che` | Run the health checks (clipboard, language servers, grammars) and show the report (nvim :checkhealth). |
| `:helpclose`, `:helpc` | Close the help window (vim :helpclose). |
| `:options`, `:opt` | Open the options window — Preferences on the Settings tab (vim :options). |
| `:mode`, `:mod` | Redraw the screen (vim :mode). |
| `:startreplace`, `:startr` | Start Replace mode (vim :startreplace). |
| `:tabfind`, `:tabf` | Find a file in the 'path' and edit it in a new tab page (vim :tabfind). |
| `:spellgood!`, `:spe!` | Add words to the known-good internal word list (vim :spellgood!). |
| `:spellgood`, `:spe` | Add words to the known-good spell list (vim :spellgood). |
| `:~` | Repeat the last :substitute (vim :~). |
