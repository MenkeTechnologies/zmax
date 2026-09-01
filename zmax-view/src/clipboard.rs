// Implementation reference: https://github.com/neovim/neovim/blob/f2906a4669a2eef6d7bf86a29648793d63c98949/runtime/autoload/provider/clipboard.vim#L68-L152

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use thiserror::Error;

#[derive(Clone, Copy)]
pub enum ClipboardType {
    Clipboard,
    Selection,
}

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error("could not convert terminal output to UTF-8: {0}")]
    FromUtf8Error(#[from] std::string::FromUtf8Error),
    #[cfg(windows)]
    #[error("Windows API error: {0}")]
    WinAPI(#[from] clipboard_win::ErrorCode),
    #[error("clipboard provider command failed")]
    CommandFailed,
    #[error("failed to write to clipboard provider's stdin")]
    StdinWriteFailed,
    #[error("clipboard provider did not return any contents")]
    MissingStdout,
    #[error("This clipboard provider does not support reading")]
    ReadingNotSupported,
}

type Result<T> = std::result::Result<T, ClipboardError>;

#[cfg(not(target_arch = "wasm32"))]
pub use external::ClipboardProvider;
#[cfg(target_arch = "wasm32")]
pub use noop::ClipboardProvider;

// Clipboard not supported for wasm
#[cfg(target_arch = "wasm32")]
mod noop {
    use super::*;

    #[derive(Debug, Clone)]
    pub enum ClipboardProvider {}

    impl ClipboardProvider {
        pub fn detect() -> Self {
            Self
        }

        pub fn name(&self) -> Cow<str> {
            "none".into()
        }

        pub fn get_contents(&self, _clipboard_type: ClipboardType) -> Result<String> {
            Err(ClipboardError::ReadingNotSupported)
        }

        pub fn set_contents(&self, _content: &str, _clipboard_type: ClipboardType) -> Result<()> {
            Ok(())
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod external {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct Command {
        command: Cow<'static, str>,
        #[serde(default)]
        args: Cow<'static, [Cow<'static, str>]>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "kebab-case")]
    pub struct CommandProvider {
        yank: Command,
        paste: Command,
        yank_primary: Option<Command>,
        paste_primary: Option<Command>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "kebab-case")]
    #[allow(clippy::large_enum_variant)]
    pub enum ClipboardProvider {
        Pasteboard,
        Wayland,
        XClip,
        XSel,
        Win32Yank,
        Tmux,
        #[cfg(windows)]
        Windows,
        Termux,
        #[cfg(feature = "term")]
        Termcode,
        Custom(CommandProvider),
        None,
    }

    impl Default for ClipboardProvider {
        #[cfg(windows)]
        fn default() -> Self {
            use zmax_stdx::env::binary_exists;

            if binary_exists("win32yank.exe") {
                Self::Win32Yank
            } else {
                Self::Windows
            }
        }

        #[cfg(target_os = "macos")]
        fn default() -> Self {
            use zmax_stdx::env::{binary_exists, env_var_is_set};

            // The pasteboard comes FIRST, ahead of tmux. `tmux save-buffer -`
            // reads tmux's own paste buffer, which is a different store from
            // the macOS pasteboard: a copy made outside tmux (Cmd-C in any
            // other app) never reaches a tmux buffer, so a tmux-provider read
            // returns whatever stale buffer tmux last recorded instead of what
            // was copied. pbcopy/pbpaste reach the real pasteboard from inside
            // tmux, so there is no reason to prefer tmux when they exist.
            if binary_exists("pbcopy") && binary_exists("pbpaste") {
                Self::Pasteboard
            } else if env_var_is_set("TMUX") && binary_exists("tmux") {
                Self::Tmux
            } else {
                #[cfg(feature = "term")]
                return Self::Termcode;
                #[cfg(not(feature = "term"))]
                return Self::None;
            }
        }

        #[cfg(not(any(windows, target_os = "macos")))]
        fn default() -> Self {
            use zmax_stdx::env::{binary_exists, env_var_is_set};

            fn is_exit_success(program: &str, args: &[&str]) -> bool {
                std::process::Command::new(program)
                    .args(args)
                    .output()
                    .ok()
                    .and_then(|out| out.status.success().then_some(()))
                    .is_some()
            }

            // As on macOS, tmux ranks BELOW every provider that talks to a real
            // system clipboard (see the comment there): tmux's paste buffer
            // misses anything copied outside tmux. It stays as the fallback for
            // a session with no display server, where it is the only route.
            if binary_exists("termux-clipboard-set") && binary_exists("termux-clipboard-get") {
                Self::Termux
            } else if env_var_is_set("WEZTERM_UNIX_SOCKET") && binary_exists("wezterm") {
                Self::Termcode
            } else if env_var_is_set("WAYLAND_DISPLAY")
                && binary_exists("wl-copy")
                && binary_exists("wl-paste")
            {
                Self::Wayland
            } else if env_var_is_set("DISPLAY") && binary_exists("xclip") {
                Self::XClip
            } else if env_var_is_set("DISPLAY")
                && binary_exists("xsel")
                // FIXME: check performance of is_exit_success
                && is_exit_success("xsel", &["-o", "-b"])
            {
                Self::XSel
            } else if binary_exists("win32yank.exe") {
                Self::Win32Yank
            } else if env_var_is_set("TMUX") && binary_exists("tmux") {
                Self::Tmux
            } else if cfg!(feature = "term") {
                Self::Termcode
            } else {
                Self::None
            }
        }
    }

    impl ClipboardProvider {
        pub fn name(&self) -> Cow<'_, str> {
            fn builtin_name<'a>(
                name: &'static str,
                provider: &'static CommandProvider,
            ) -> Cow<'a, str> {
                if provider.yank.command != provider.paste.command {
                    Cow::Owned(format!(
                        "{} ({}+{})",
                        name, provider.yank.command, provider.paste.command
                    ))
                } else {
                    Cow::Owned(format!("{} ({})", name, provider.yank.command))
                }
            }

            match self {
                // These names should match the config option names from Serde
                Self::Pasteboard => builtin_name("pasteboard", &PASTEBOARD),
                Self::Wayland => builtin_name("wayland", &WL_CLIPBOARD),
                Self::XClip => builtin_name("x-clip", &XCLIP),
                Self::XSel => builtin_name("x-sel", &XSEL),
                Self::Win32Yank => builtin_name("win32-yank", &WIN32),
                Self::Tmux => builtin_name("tmux", &TMUX),
                Self::Termux => builtin_name("termux", &TERMUX),
                #[cfg(windows)]
                Self::Windows => "windows".into(),
                #[cfg(feature = "term")]
                Self::Termcode => "termcode".into(),
                Self::Custom(command_provider) => Cow::Owned(format!(
                    "custom ({}+{})",
                    command_provider.yank.command, command_provider.paste.command
                )),
                Self::None => "none".into(),
            }
        }

        pub fn get_contents(&self, clipboard_type: &ClipboardType) -> Result<String> {
            fn yank_from_builtin(
                provider: CommandProvider,
                clipboard_type: &ClipboardType,
            ) -> Result<String> {
                match clipboard_type {
                    ClipboardType::Clipboard => execute_command(&provider.yank, None, true)?
                        .ok_or(ClipboardError::MissingStdout),
                    ClipboardType::Selection => {
                        if let Some(cmd) = provider.yank_primary.as_ref() {
                            return execute_command(cmd, None, true)?
                                .ok_or(ClipboardError::MissingStdout);
                        }

                        Ok(String::new())
                    }
                }
            }

            match self {
                Self::Pasteboard => yank_from_builtin(PASTEBOARD, clipboard_type),
                Self::Wayland => yank_from_builtin(WL_CLIPBOARD, clipboard_type),
                Self::XClip => yank_from_builtin(XCLIP, clipboard_type),
                Self::XSel => yank_from_builtin(XSEL, clipboard_type),
                Self::Win32Yank => yank_from_builtin(WIN32, clipboard_type),
                Self::Tmux => yank_from_builtin(TMUX, clipboard_type),
                Self::Termux => yank_from_builtin(TERMUX, clipboard_type),
                #[cfg(target_os = "windows")]
                Self::Windows => match clipboard_type {
                    ClipboardType::Clipboard => {
                        let contents =
                            clipboard_win::get_clipboard(clipboard_win::formats::Unicode)?;
                        Ok(contents)
                    }
                    ClipboardType::Selection => Ok(String::new()),
                },
                #[cfg(feature = "term")]
                Self::Termcode => Err(ClipboardError::ReadingNotSupported),
                Self::Custom(command_provider) => {
                    execute_command(&command_provider.yank, None, true)?
                        .ok_or(ClipboardError::MissingStdout)
                }
                Self::None => Err(ClipboardError::ReadingNotSupported),
            }
        }

        pub fn set_contents(&self, content: &str, clipboard_type: ClipboardType) -> Result<()> {
            fn paste_to_builtin(
                provider: CommandProvider,
                content: &str,
                clipboard_type: ClipboardType,
            ) -> Result<()> {
                let cmd = match clipboard_type {
                    ClipboardType::Clipboard => &provider.paste,
                    ClipboardType::Selection => {
                        if let Some(cmd) = provider.paste_primary.as_ref() {
                            cmd
                        } else {
                            return Ok(());
                        }
                    }
                };

                execute_command(cmd, Some(content), false).map(|_| ())
            }

            match self {
                Self::Pasteboard => paste_to_builtin(PASTEBOARD, content, clipboard_type),
                Self::Wayland => paste_to_builtin(WL_CLIPBOARD, content, clipboard_type),
                Self::XClip => paste_to_builtin(XCLIP, content, clipboard_type),
                Self::XSel => paste_to_builtin(XSEL, content, clipboard_type),
                Self::Win32Yank => paste_to_builtin(WIN32, content, clipboard_type),
                Self::Tmux => paste_to_builtin(TMUX, content, clipboard_type),
                Self::Termux => paste_to_builtin(TERMUX, content, clipboard_type),
                #[cfg(target_os = "windows")]
                Self::Windows => match clipboard_type {
                    ClipboardType::Clipboard => {
                        clipboard_win::set_clipboard(clipboard_win::formats::Unicode, content)?;
                        Ok(())
                    }
                    ClipboardType::Selection => Ok(()),
                },
                #[cfg(feature = "term")]
                Self::Termcode => {
                    use std::io::Write;
                    use termina::escape::osc::{self, Osc};
                    let selection = match clipboard_type {
                        ClipboardType::Clipboard => osc::Selection::CLIPBOARD,
                        ClipboardType::Selection => osc::Selection::PRIMARY,
                    };
                    // NOTE: it would be ideal to have the terminal execute this but it _should_
                    // work to send this over stdout instead.
                    let mut stdout = std::io::stdout().lock();
                    write!(stdout, "{}", Osc::SetSelection(selection, content))?;
                    stdout.flush()?;
                    Ok(())
                }
                Self::Custom(command_provider) => match clipboard_type {
                    ClipboardType::Clipboard => {
                        execute_command(&command_provider.paste, Some(content), false).map(|_| ())
                    }
                    ClipboardType::Selection => {
                        if let Some(cmd) = &command_provider.paste_primary {
                            execute_command(cmd, Some(content), false).map(|_| ())
                        } else {
                            Ok(())
                        }
                    }
                },
                Self::None => Ok(()),
            }
        }
    }

    macro_rules! command_provider {
        ($name:ident,
         yank => $yank_cmd:literal $( , $yank_arg:literal )* ;
         paste => $paste_cmd:literal $( , $paste_arg:literal )* ; ) => {
            const $name: CommandProvider = CommandProvider {
                yank: Command {
                    command: Cow::Borrowed($yank_cmd),
                    args: Cow::Borrowed(&[ $( Cow::Borrowed($yank_arg) ),* ])
                },
                paste: Command {
                    command: Cow::Borrowed($paste_cmd),
                    args: Cow::Borrowed(&[ $( Cow::Borrowed($paste_arg) ),* ])
                },
                yank_primary: None,
                paste_primary: None,
            };
        };
        ($name:ident,
         yank => $yank_cmd:literal $( , $yank_arg:literal )* ;
         paste => $paste_cmd:literal $( , $paste_arg:literal )* ;
         yank_primary => $yank_primary_cmd:literal $( , $yank_primary_arg:literal )* ;
         paste_primary => $paste_primary_cmd:literal $( , $paste_primary_arg:literal )* ; ) => {
            const $name: CommandProvider = CommandProvider {
                yank: Command {
                    command: Cow::Borrowed($yank_cmd),
                    args: Cow::Borrowed(&[ $( Cow::Borrowed($yank_arg) ),* ])
                },
                paste: Command {
                    command: Cow::Borrowed($paste_cmd),
                    args: Cow::Borrowed(&[ $( Cow::Borrowed($paste_arg) ),* ])
                },
                yank_primary: Some(Command {
                    command: Cow::Borrowed($yank_primary_cmd),
                    args: Cow::Borrowed(&[ $( Cow::Borrowed($yank_primary_arg) ),* ])
                }),
                paste_primary: Some(Command {
                    command: Cow::Borrowed($paste_primary_cmd),
                    args: Cow::Borrowed(&[ $( Cow::Borrowed($paste_primary_arg) ),* ])
                }),
            };
        };
    }

    command_provider! {
        TMUX,
        yank => "tmux", "save-buffer", "-";
        paste => "tmux", "load-buffer", "-w", "-";
    }
    command_provider! {
        PASTEBOARD,
        yank => "pbpaste";
        paste => "pbcopy";
    }
    command_provider! {
        WL_CLIPBOARD,
        yank => "wl-paste", "--no-newline";
        paste => "wl-copy", "--type", "text/plain";
        yank_primary => "wl-paste", "-p", "--no-newline";
        paste_primary => "wl-copy", "-p", "--type", "text/plain";
    }
    command_provider! {
        XCLIP,
        yank => "xclip", "-o", "-selection", "clipboard";
        paste => "xclip", "-i", "-selection", "clipboard";
        yank_primary => "xclip", "-o";
        paste_primary => "xclip", "-i";
    }
    command_provider! {
        XSEL,
        yank => "xsel", "-o", "-b";
        paste => "xsel", "-i", "-b";
        yank_primary => "xsel", "-o";
        paste_primary => "xsel", "-i";
    }
    command_provider! {
        WIN32,
        yank => "win32yank.exe", "-o", "--lf";
        paste => "win32yank.exe", "-i", "--crlf";
    }
    command_provider! {
        TERMUX,
        yank => "termux-clipboard-get";
        paste => "termux-clipboard-set";
    }

    fn execute_command(
        cmd: &Command,
        input: Option<&str>,
        pipe_output: bool,
    ) -> Result<Option<String>> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let stdin = input.map(|_| Stdio::piped()).unwrap_or_else(Stdio::null);
        let stdout = pipe_output.then(Stdio::piped).unwrap_or_else(Stdio::null);

        let mut command: Command = Command::new(cmd.command.as_ref());

        #[allow(unused_mut)]
        let mut command_mut: &mut Command = command
            .args(cmd.args.iter().map(AsRef::as_ref))
            .stdin(stdin)
            .stdout(stdout)
            .stderr(Stdio::null());

        // upstream fix for clipboard provider process handling
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;

            unsafe {
                command_mut = command_mut.pre_exec(|| match libc::setsid() {
                    -1 => Err(std::io::Error::last_os_error()),
                    _ => Ok(()),
                });
            }
        }

        let mut child = command_mut.spawn()?;

        // Emacs `set-selection-coding-system` / `set-next-selection-coding-system`:
        // the coding system the bytes crossing to and from the window system's
        // clipboard are in. This is the transfer, so this is where it is consumed
        // (which is what makes the `next-` form apply to one transfer only).
        // Unset — the normal case — leaves the exchange in UTF-8.
        let coding = zmax_core::coding::take_selection_coding();

        if let Some(input) = input {
            let mut stdin = child.stdin.take().ok_or(ClipboardError::StdinWriteFailed)?;
            stdin
                .write_all(&zmax_core::coding::encode_with(coding, input))
                .map_err(|_| ClipboardError::StdinWriteFailed)?;
        }

        // TODO: add timer?
        let output = child.wait_with_output()?;

        if !output.status.success() {
            log::error!(
                "clipboard provider {} failed with stderr: \"{}\"",
                cmd.command,
                String::from_utf8_lossy(&output.stderr)
            );
            return Err(ClipboardError::CommandFailed);
        }

        if pipe_output {
            // A coding system decodes the clipboard's bytes with it; with none
            // set the exchange stays strict UTF-8, as it has always been.
            Ok(Some(match coding {
                Some(encoding) => encoding.decode(&output.stdout).0.into_owned(),
                None => String::from_utf8(output.stdout)?,
            }))
        } else {
            Ok(None)
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// Every provider a user can name in their config, paired with the name
    /// serde reads and writes for it.
    fn configurable_providers() -> Vec<(&'static str, ClipboardProvider)> {
        vec![
            ("pasteboard", ClipboardProvider::Pasteboard),
            ("wayland", ClipboardProvider::Wayland),
            ("x-clip", ClipboardProvider::XClip),
            ("x-sel", ClipboardProvider::XSel),
            ("win32-yank", ClipboardProvider::Win32Yank),
            ("tmux", ClipboardProvider::Tmux),
            ("termux", ClipboardProvider::Termux),
            ("none", ClipboardProvider::None),
        ]
    }

    /// `name()` is what `:clipboard-provider` reports, and its comment says the
    /// names must match the serde config names -- otherwise the editor tells a
    /// user their provider is `x-clip` while their config has to say something
    /// else. Nothing enforced that until now.
    #[test]
    fn the_reported_name_starts_with_the_serde_config_name() {
        for (config_name, provider) in configurable_providers() {
            let serialized = serde_json::to_string(&provider).unwrap();
            assert_eq!(
                serialized,
                format!("\"{config_name}\""),
                "serde name for {provider:?}"
            );

            let reported = provider.name();
            assert!(
                reported.starts_with(config_name),
                "{provider:?} reports {reported:?}, which does not start with {config_name:?}"
            );
        }
    }

    /// Every one of those names round-trips back to the same provider, so a
    /// config that zmax wrote is a config zmax can read.
    #[test]
    fn config_names_round_trip() {
        for (config_name, provider) in configurable_providers() {
            let parsed: ClipboardProvider =
                serde_json::from_str(&format!("\"{config_name}\"")).unwrap();
            assert_eq!(parsed, provider, "{config_name}");
        }

        assert!(serde_json::from_str::<ClipboardProvider>("\"xclip\"").is_err());
        assert!(serde_json::from_str::<ClipboardProvider>("\"X-Clip\"").is_err());
    }

    /// A provider whose yank and paste are different programs names both, since
    /// knowing only one of them makes a "command not found" hard to place. When
    /// they are the same program it is named once.
    #[test]
    fn the_name_lists_both_programs_only_when_they_differ() {
        // pbpaste reads, pbcopy writes.
        assert_eq!(
            ClipboardProvider::Pasteboard.name(),
            "pasteboard (pbpaste+pbcopy)"
        );
        assert_eq!(
            ClipboardProvider::Termux.name(),
            "termux (termux-clipboard-get+termux-clipboard-set)"
        );
        // tmux does both.
        assert_eq!(ClipboardProvider::Tmux.name(), "tmux (tmux)");
        assert_eq!(ClipboardProvider::XClip.name(), "x-clip (xclip)");
    }

    /// A custom provider is the escape hatch for a clipboard tool zmax does not
    /// ship support for, so the config shape users write has to keep working:
    /// the primary-selection commands are optional, and the name reports both
    /// programs.
    #[test]
    fn a_custom_provider_parses_its_documented_config_shape() {
        let provider: ClipboardProvider = serde_json::from_str(
            r#"{"custom": {
                "yank": {"command": "myget", "args": ["--read"]},
                "paste": {"command": "myset"}
            }}"#,
        )
        .expect("custom providers take yank and paste, with args optional");

        assert_eq!(provider.name(), "custom (myget+myset)");
        assert!(matches!(provider, ClipboardProvider::Custom(_)));

        // Read back through serde rather than the private fields: the optional
        // primary-selection commands stay unset, and `args` defaults to empty.
        let round_tripped = serde_json::to_value(&provider).unwrap();
        let custom = &round_tripped["custom"];
        assert!(
            custom["yank-primary"].is_null() && custom["paste-primary"].is_null(),
            "the primary-selection commands are optional: {custom}"
        );
        assert_eq!(custom["paste"]["args"].as_array().map(Vec::len), Some(0));
    }
}
