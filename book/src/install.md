# Installing Zmax

The typical way to install Zmax is via [your operating system's package manager](./package-managers.md).

Note that:

- To get the latest nightly version of Zmax, you need to
  [build from source](./building-from-source.md).

- To take full advantage of Zmax, install the language servers for your
  preferred programming languages. See the
  [wiki](https://github.com/MenkeTechnologies/zmax/wiki/Language-Server-Configurations)
  for instructions.

## Pre-built binaries

Download pre-built binaries from the [GitHub Releases page](https://github.com/MenkeTechnologies/zmax/releases).
The tarball contents include an `zmax` binary and a `runtime` directory.
To set up Zmax:

1. Add the `zmax` binary to your system's `$PATH` to allow it to be used from the command line.
2. Copy the `runtime` directory to a location that `zmax` searches for runtime files. A typical location on Linux/macOS is `~/.zmax/runtime`.

To see the runtime directories that `zmax` searches, run `zmax --health`. If necessary, you can override the default runtime location by setting the `ZMAX_RUNTIME` environment variable.
