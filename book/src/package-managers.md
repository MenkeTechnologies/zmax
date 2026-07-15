## Package managers

- [Linux](#linux)
  - [Debian](#debian)
  - [Ubuntu/Mint](#ubuntumint)
  - [Fedora/RHEL](#fedorarhel)
  - [Arch Linux extra](#arch-linux-extra)
  - [NixOS](#nixos)
  - [Flatpak](#flatpak)
  - [Snap](#snap)
  - [AppImage](#appimage)
  - [Linux Homebrew Core](#linux-homebrew-core)
- [macOS](#macos)
  - [Homebrew Core](#homebrew-core)
  - [MacPorts](#macports)
- [Windows](#windows)
  - [Winget](#winget)
  - [Scoop](#scoop)
  - [Chocolatey](#chocolatey)
  - [Packably](#packably)
  - [MSYS2](#msys2)

[![Packaging status](https://repology.org/badge/vertical-allrepos/zemacs-editor.svg)](https://repology.org/project/zemacs-editor/versions)

## Linux

The following third party repositories are available:

### Debian

```sh
sudo apt install zemacs
```

If you are running a system older than Debian 13, follow the steps for
[Ubuntu/Mint](#ubuntumint).

### Ubuntu/Mint

Install the Debian package [from the release page](https://github.com/MenkeTechnologies/zemacs/releases/latest).

If you are running a system older than Ubuntu 22.04, Mint 21, or Debian 12, you can build the `.deb` file locally
[from source](./building-from-source.md#building-the-debian-package).

### Fedora/RHEL

```sh
sudo dnf install zemacs
```

### Arch Linux extra

Releases are available in the `extra` repository:

```sh
sudo pacman -S zemacs
```

> 💡 Run Zemacs with the `zemacs` command. For example, `zemacs --health` to check health.

Additionally, a [zemacs-git](https://aur.archlinux.org/packages/zemacs-git/) package is available
in the AUR, which builds the master branch.

### NixOS

Zemacs is available in [nixpkgs](https://github.com/nixos/nixpkgs) through the `zemacs` attribute,
the unstable channel usually carries the latest release.

Zemacs is also available as a [flake](https://wiki.nixos.org/wiki/Flakes) in the project
root. Use `nix develop` to spin up a reproducible development shell. Outputs are
cached for each push to master using [Cachix](https://www.cachix.org/). The
flake is configured to automatically make use of this cache assuming the user
accepts the new settings on first use.

If you are using a version of Nix without flakes enabled,
[install Cachix CLI](https://docs.cachix.org/installation) and use
`cachix use zemacs` to configure Nix to use cached outputs when possible.

### Flatpak

Zemacs is available on [Flathub](https://flathub.org/en-GB/apps/com.menketechnologies.Zemacs):

```sh
flatpak install flathub com.menketechnologies.Zemacs
flatpak run com.menketechnologies.Zemacs
```

### Snap

Zemacs is available on [Snapcraft](https://snapcraft.io/zemacs) and can be installed with:

```sh
snap install --classic zemacs
```

This will install Zemacs as `/snap/bin/zemacs`, so make sure `/snap/bin` is in your `PATH`.

### AppImage

Install Zemacs using the Linux [AppImage](https://appimage.org/) format.
Download the official Zemacs AppImage from the [latest releases](https://github.com/MenkeTechnologies/zemacs/releases/latest) page.

```sh
chmod +x zemacs-*.AppImage # change permission for executable mode
./zemacs-*.AppImage # run zemacs
```

You can optionally [add the `.desktop` file](./building-from-source.md#configure-the-desktop-shortcut). Zemacs must be installed in `PATH` with the name `zemacs`. For example:
```sh
mkdir -p "$HOME/.local/bin"
mv zemacs-*.AppImage "$HOME/.local/bin/zemacs"
```

and make sure `~/.local/bin` is in your `PATH`.

### Linux Homebrew Core

Checkout the [macOS](#homebrew-core) instructions below.

## macOS

### Homebrew Core

Install the latest release:

```sh
brew install zemacs
```

Or, install the latest nightly version:

```sh
brew install --HEAD zemacs
```

### MacPorts

```sh
sudo port install zemacs
```

## Windows

Install on Windows using [Winget](https://learn.microsoft.com/en-us/windows/package-manager/winget/), [Scoop](https://scoop.sh/), [Chocolatey](https://chocolatey.org/), [Packably](https://www.packably.com.br/)
or [MSYS2](https://msys2.org/).

### Winget
Windows Package Manager winget command-line tool is by default available on Windows 11 and modern versions of Windows 10 as a part of the App Installer.
You can get [App Installer from the Microsoft Store](https://www.microsoft.com/p/app-installer/9nblggh4nns1#activetab=pivot:overviewtab). If it's already installed, make sure it is updated with the latest version.

```sh
winget install Zemacs.Zemacs
```

### Scoop

```sh
scoop install zemacs
```

### Chocolatey

```sh
choco install zemacs
```

### Packably

```sh
packl install zemacs
```

### MSYS2

For 64-bit Windows 8.1 or above:

```sh
pacman -S mingw-w64-ucrt-x86_64-zemacs
```
