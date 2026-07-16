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

[![Packaging status](https://repology.org/badge/vertical-allrepos/zmax-editor.svg)](https://repology.org/project/zmax-editor/versions)

## Linux

The following third party repositories are available:

### Debian

```sh
sudo apt install zmax
```

If you are running a system older than Debian 13, follow the steps for
[Ubuntu/Mint](#ubuntumint).

### Ubuntu/Mint

Install the Debian package [from the release page](https://github.com/MenkeTechnologies/zmax/releases/latest).

If you are running a system older than Ubuntu 22.04, Mint 21, or Debian 12, you can build the `.deb` file locally
[from source](./building-from-source.md#building-the-debian-package).

### Fedora/RHEL

```sh
sudo dnf install zmax
```

### Arch Linux extra

Releases are available in the `extra` repository:

```sh
sudo pacman -S zmax
```

> 💡 Run Zmax with the `zmax` command. For example, `zmax --health` to check health.

Additionally, a [zmax-git](https://aur.archlinux.org/packages/zmax-git/) package is available
in the AUR, which builds the master branch.

### NixOS

Zmax is available in [nixpkgs](https://github.com/nixos/nixpkgs) through the `zmax` attribute,
the unstable channel usually carries the latest release.

Zmax is also available as a [flake](https://wiki.nixos.org/wiki/Flakes) in the project
root. Use `nix develop` to spin up a reproducible development shell. Outputs are
cached for each push to master using [Cachix](https://www.cachix.org/). The
flake is configured to automatically make use of this cache assuming the user
accepts the new settings on first use.

If you are using a version of Nix without flakes enabled,
[install Cachix CLI](https://docs.cachix.org/installation) and use
`cachix use zmax` to configure Nix to use cached outputs when possible.

### Flatpak

Zmax is available on [Flathub](https://flathub.org/en-GB/apps/com.menketechnologies.Zmax):

```sh
flatpak install flathub com.menketechnologies.Zmax
flatpak run com.menketechnologies.Zmax
```

### Snap

Zmax is available on [Snapcraft](https://snapcraft.io/zmax) and can be installed with:

```sh
snap install --classic zmax
```

This will install Zmax as `/snap/bin/zmax`, so make sure `/snap/bin` is in your `PATH`.

### AppImage

Install Zmax using the Linux [AppImage](https://appimage.org/) format.
Download the official Zmax AppImage from the [latest releases](https://github.com/MenkeTechnologies/zmax/releases/latest) page.

```sh
chmod +x zmax-*.AppImage # change permission for executable mode
./zmax-*.AppImage # run zmax
```

You can optionally [add the `.desktop` file](./building-from-source.md#configure-the-desktop-shortcut). Zmax must be installed in `PATH` with the name `zmax`. For example:
```sh
mkdir -p "$HOME/.local/bin"
mv zmax-*.AppImage "$HOME/.local/bin/zmax"
```

and make sure `~/.local/bin` is in your `PATH`.

### Linux Homebrew Core

Checkout the [macOS](#homebrew-core) instructions below.

## macOS

### Homebrew Core

Install the latest release:

```sh
brew install zmax
```

Or, install the latest nightly version:

```sh
brew install --HEAD zmax
```

### MacPorts

```sh
sudo port install zmax
```

## Windows

Install on Windows using [Winget](https://learn.microsoft.com/en-us/windows/package-manager/winget/), [Scoop](https://scoop.sh/), [Chocolatey](https://chocolatey.org/), [Packably](https://www.packably.com.br/)
or [MSYS2](https://msys2.org/).

### Winget
Windows Package Manager winget command-line tool is by default available on Windows 11 and modern versions of Windows 10 as a part of the App Installer.
You can get [App Installer from the Microsoft Store](https://www.microsoft.com/p/app-installer/9nblggh4nns1#activetab=pivot:overviewtab). If it's already installed, make sure it is updated with the latest version.

```sh
winget install Zmax.Zmax
```

### Scoop

```sh
scoop install zmax
```

### Chocolatey

```sh
choco install zmax
```

### Packably

```sh
packl install zmax
```

### MSYS2

For 64-bit Windows 8.1 or above:

```sh
pacman -S mingw-w64-ucrt-x86_64-zmax
```
