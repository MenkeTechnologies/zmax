## Building from source

- [Configuring Zmax's runtime files](#configuring-zmaxs-runtime-files)
  - [Linux and macOS](#linux-and-macos)
  - [Windows](#windows)
  - [Multiple runtime directories](#multiple-runtime-directories)
  - [Note to packagers](#note-to-packagers)
- [Validating the installation](#validating-the-installation)
- [Configure the desktop shortcut](#configure-the-desktop-shortcut)
- [Building the Debian package](#building-the-debian-package)

Requirements:

Clone the Zmax GitHub repository into a directory of your choice. The
examples in this documentation assume installation into either `~/src/` on
Linux and macOS, or `%userprofile%\src\` on Windows.

- The [Rust toolchain](https://www.rust-lang.org/tools/install)
- The [Git version control system](https://git-scm.com/)
- A C++14 compatible compiler to build the tree-sitter grammars, for example GCC or Clang

If you are using the `musl-libc` standard library instead of `glibc` the following environment variable must be set during the build to ensure tree-sitter grammars can be loaded correctly:

```sh
RUSTFLAGS="-C target-feature=-crt-static"
```

1. Clone the repository:

   ```sh
   git clone https://github.com/MenkeTechnologies/zmax
   cd zmax
   ```

2. Compile from source:

   ```sh
   # Reproducible
   cargo install --path zmax-term --locked
   ```
   ```sh
   # Optimized
   cargo install \
      --profile opt \
      --config 'build.rustflags=["-C", "target-cpu=native"]' \
      --path zmax-term \
      --locked
   ```

   Either command will create the `zmax` executable and construct the tree-sitter
   grammars in the local `runtime` folder.

> 💡 If you do not want to fetch or build grammars, set an environment variable `ZMAX_DISABLE_AUTO_GRAMMAR_BUILD`

> 💡 Tree-sitter grammars can be fetched and compiled if not pre-packaged. Fetch
> grammars with `zmax --grammar fetch` and compile them with
> `zmax --grammar build`. This will install them in
> the `runtime` directory within the user's zmax config directory (more
> [details below](#multiple-runtime-directories)).

> 💡 If you only want to build _some_ grammars, see [`use-grammars`](./languages.md#choosing-grammars)

### Cargo features

The `zmax-term` crate exposes a few Cargo features (all set on `--release`/`opt`
builds by default):

| Feature     | Default | Effect |
| ----------- | ------- | ------ |
| `git`       | yes     | Git integration in the VCS gutter/diff layer (`zmax-vcs/git`). |
| `scripting` | yes     | The [embedded scripting languages](./scripting.md) and the REPL — elisp, vimscript and awk, plus zsh, stryke, ruby, php, python, node and arb on unix. Pulls in the interpreter crates. |
| `unicode-lines` | no  | Treat Unicode line separators (e.g. `U+2028`) as line breaks. |

To build **without the embedded scripting languages** — dropping every
interpreter crate (`elisprs`, `vimlrs`, `awkrs`, `zshrs`, `strykelang`) from the
dependency graph for a leaner, faster build — disable default features and re-add
just the ones you want:

```sh
cargo install --path zmax-term --locked --no-default-features --features git
```

In a scripting-less build the `:elisp`/`:vim`/`:awk`/`:zsh`/`:stryke`/`:ruby`/`:php`/`:python`/`:node`/`:arb` commands and
the REPL still exist but report that scripting was not compiled in; `init.el` /
`init.vim` are not loaded.

### Configuring Zmax's runtime files

#### Linux and macOS

The **runtime** directory is one below the Zmax source, so either export a
`ZMAX_RUNTIME` environment variable to point to that directory and add it to
your `~/.bashrc` or equivalent:

```sh
export ZMAX_RUNTIME=~/src/zmax/runtime
```

Or, create a symbolic link:

```sh
ln -Tsf $PWD/runtime ~/.zmax/runtime
```

#### Windows

Either set the `ZMAX_RUNTIME` environment variable to point to the runtime files using the Windows setting (search for
`Edit environment variables for your account`) or use the `setx` command in
Cmd:

```sh
setx ZMAX_RUNTIME "%userprofile%\src\zmax\runtime"
```

> 💡 `%userprofile%` resolves to your user directory like
> `C:\Users\Your-Name\` for example.

Or, create a symlink in `%appdata%\zmax\` that links to the source code directory:

| Method     | Command                                                                                |
| ---------- | -------------------------------------------------------------------------------------- |
| PowerShell | `New-Item -ItemType Junction -Target "runtime" -Path "$Env:AppData\zmax\runtime"`     |
| Cmd        | `cd %appdata%\zmax` <br/> `mklink /D runtime "%userprofile%\src\zmax\runtime"`       |

> 💡 On Windows, creating a symbolic link may require running PowerShell or
> Cmd as an administrator.

#### Multiple runtime directories

When Zmax finds multiple runtime directories it will search through them for files in the
following order:

1. `runtime/` sibling directory to `$CARGO_MANIFEST_DIR` directory (this is intended for
  developing and testing zmax only).
2. `runtime/` subdirectory of OS-dependent zmax user config directory.
3. `$ZMAX_RUNTIME`
4. Distribution-specific fallback directory (set at compile time—not run time—
   with the `ZMAX_DEFAULT_RUNTIME` environment variable)
5. `runtime/` subdirectory of path to Zmax executable.

This order also sets the priority for selecting which file will be used if multiple runtime
directories have files with the same name.

#### Note to packagers

If you are making a package of Zmax for end users, to provide a good out of
the box experience, you should set the `ZMAX_DEFAULT_RUNTIME` environment
variable at build time (before invoking `cargo build`) to a directory which
will store the final runtime files after installation. For example, say you want
to package the runtime into `/usr/lib/zmax/runtime`. The rough steps a build
script could follow are:

1. `export ZMAX_DEFAULT_RUNTIME=/usr/lib/zmax/runtime`
1. `cargo build --profile opt --locked`
1. `cp -r runtime $BUILD_DIR/usr/lib/zmax/`
1. `cp target/opt/zmax $BUILD_DIR/usr/bin/zmax`

This way the resulting `zmax` binary will always look for its runtime directory in
`/usr/lib/zmax/runtime` if the user has no custom runtime in `~/.zmax`
or `ZMAX_RUNTIME`.

### Validating the installation

To make sure everything is set up as expected you should run the Zmax health
check:

```sh
zmax --health
```

For more information on the health check results refer to
[Health check](https://github.com/MenkeTechnologies/zmax/wiki/Healthcheck).

### Configure the desktop shortcut

If your desktop environment supports the
[XDG desktop menu](https://specifications.freedesktop.org/menu-spec/menu-spec-latest.html)
you can configure Zmax to show up in the application menu by copying the
provided `.desktop` and icon files to their correct folders:

```sh
cp contrib/Zmax.desktop ~/.local/share/applications
cp contrib/zmax.png ~/.icons # or ~/.local/share/icons
```
It is recommended to convert the links in the `.desktop` file to absolute paths to avoid potential problems:

```sh
sed -i -e "s|Exec=zmax %F|Exec=$(readlink -f ~/.cargo/bin/zmax) %F|g" \
  -e "s|Icon=zmax|Icon=$(readlink -f ~/.icons/zmax.png)|g" ~/.local/share/applications/Zmax.desktop
```

To use another terminal than the system default, you can modify the `.desktop`
file. For example, to use `kitty`:

```sh
sed -i "s|Exec=zmax %F|Exec=kitty zmax %F|g" ~/.local/share/applications/Zmax.desktop
sed -i "s|Terminal=true|Terminal=false|g" ~/.local/share/applications/Zmax.desktop
```

### Building the Debian package

If the `.deb` file provided on the release page uses a `libc` version higher
than that used by your Debian, Ubuntu, or Mint system, you can build the package
from source to match your system's dependencies.

Install `cargo-deb`, the tool used for building the `.deb` file:

```sh
cargo install cargo-deb
```

After cloning and entering the Zmax repository as previously described,
use the following command to build the release binary and package it into a `.deb` file in a single step.

```sh
cargo deb -- --locked
```

> 💡 This locks you into the `--release` profile. But you can also build zmax in any way you like.
> As long as you leave a `target/release/zmax` file, it will get packaged with `cargo deb --no-build`

> 💡 Don't worry about the following:
> ```
> warning: Failed to find dependency specification
> ```
> Cargo deb just reports which packaged files it didn't derive dependencies for. But
> so far the dependency deriving seams very good, even if some of the grammar files are skipped.

You can find the resulted `.deb` in `target/debian/`. It should contain everything it needs, including the

- completions for bash, fish, zsh
- .desktop file
- icon (though desktop environments might use their own since the name of the package is correctly `zmax`)
- launcher to the binary with the runtime
