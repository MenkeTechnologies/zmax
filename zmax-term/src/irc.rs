//! A real IRC client — the faithful core of the Emacs `erc` / `rcirc` chat
//! layers. This module owns the protocol (RFC 1459/2812 message parsing and
//! command formatting) and a blocking TCP [`Connection`] that registers, joins
//! channels, sends messages, and reads the incoming stream. The protocol layer
//! is pure and exhaustively unit-tested; the transport uses only `std::net`.
//!
//! The editor commands in `commands/typed.rs` drive this: `:irc-connect`,
//! `:irc-join`, `:irc-say`, `:irc-quit`, rendering the session transcript into a
//! buffer. It is a functional IRC client (connect / register / join / PRIVMSG /
//! PING-PONG / read) rather than a full live-updating multi-window chat UI, so
//! the erc/rcirc ports are recorded as `partial`.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

/// A parsed IRC protocol message: `[:prefix] COMMAND [params...] [:trailing]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The optional source prefix (`nick!user@host` or a server name).
    pub prefix: Option<String>,
    /// The command verb (`PRIVMSG`, `JOIN`) or a 3-digit numeric reply.
    pub command: String,
    /// Middle params plus the trailing param (the part after `:`) as the last.
    pub params: Vec<String>,
}

impl Message {
    /// Parse one line (without the trailing CRLF) into a [`Message`]. Returns
    /// `None` for an empty line.
    pub fn parse(line: &str) -> Option<Message> {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            return None;
        }
        let mut rest = line;
        let mut prefix = None;
        if let Some(stripped) = rest.strip_prefix(':') {
            let (p, r) = stripped.split_once(' ')?;
            prefix = Some(p.to_string());
            rest = r.trim_start();
        }
        // Split off the trailing parameter (everything after the first " :").
        let (head, trailing) = match rest.split_once(" :") {
            Some((h, t)) => (h, Some(t.to_string())),
            None => (rest, None),
        };
        let mut parts = head.split_whitespace();
        let command = parts.next()?.to_string();
        let mut params: Vec<String> = parts.map(str::to_string).collect();
        if let Some(t) = trailing {
            params.push(t);
        }
        Some(Message {
            prefix,
            command,
            params,
        })
    }

    /// The nick portion of the prefix (`nick!user@host` -> `nick`), if any.
    pub fn nick(&self) -> Option<&str> {
        self.prefix
            .as_deref()
            .map(|p| p.split(['!', '@']).next().unwrap_or(p))
    }

    /// Render this message as a human-readable transcript line for the buffer.
    /// PRIVMSG/NOTICE become `<nick> text`; everything else is shown raw-ish.
    pub fn transcript(&self) -> String {
        match self.command.as_str() {
            "PRIVMSG" | "NOTICE" if self.params.len() >= 2 => {
                let target = &self.params[0];
                let text = &self.params[1];
                let who = self.nick().unwrap_or("?");
                if target.starts_with(['#', '&']) {
                    format!("{target} <{who}> {text}")
                } else {
                    format!("<{who}> {text}")
                }
            }
            "JOIN" => format!("* {} joined {}", self.nick().unwrap_or("?"), self.params.join(" ")),
            "PART" => format!("* {} left {}", self.nick().unwrap_or("?"), self.params.join(" ")),
            "QUIT" => format!("* {} quit ({})", self.nick().unwrap_or("?"), self.params.join(" ")),
            _ => {
                let who = self.nick().map(|n| format!("{n} ")).unwrap_or_default();
                format!("{}{} {}", who, self.command, self.params.join(" "))
            }
        }
    }
}

/// Format a `NICK` registration command line (no CRLF).
pub fn cmd_nick(nick: &str) -> String {
    format!("NICK {nick}")
}

/// Format a `USER` registration command line.
pub fn cmd_user(user: &str, realname: &str) -> String {
    format!("USER {user} 0 * :{realname}")
}

/// Format a `JOIN` command line.
pub fn cmd_join(channel: &str) -> String {
    format!("JOIN {channel}")
}

/// Format a `PRIVMSG` command line to a target (channel or nick).
pub fn cmd_privmsg(target: &str, text: &str) -> String {
    format!("PRIVMSG {target} :{text}")
}

/// Format the `PONG` reply to a server `PING <token>`.
pub fn cmd_pong(token: &str) -> String {
    format!("PONG :{token}")
}

/// A blocking IRC connection over TCP. Not `async`; the editor drives it from a
/// background reader thread (see `commands/typed.rs`).
pub struct Connection {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    pub nick: String,
}

impl Connection {
    /// Connect to `host:port`, register with `nick`, and return the session.
    /// A default port of 6667 is used if `addr` has no `:port`.
    pub fn connect(addr: &str, nick: &str) -> std::io::Result<Connection> {
        let addr = if addr.contains(':') {
            addr.to_string()
        } else {
            format!("{addr}:6667")
        };
        let stream = TcpStream::connect(&addr)?;
        stream.set_read_timeout(Some(Duration::from_millis(400)))?;
        let reader = BufReader::new(stream.try_clone()?);
        let mut conn = Connection {
            stream,
            reader,
            nick: nick.to_string(),
        };
        conn.send(&cmd_nick(nick))?;
        conn.send(&cmd_user(nick, nick))?;
        Ok(conn)
    }

    /// A cloned write handle to the socket, so a command can send while the
    /// reader thread owns the `Connection` for `poll`ing.
    pub fn write_handle(&self) -> std::io::Result<TcpStream> {
        self.stream.try_clone()
    }

    /// Send a raw protocol line, appending CRLF.
    pub fn send(&mut self, line: &str) -> std::io::Result<()> {
        self.stream.write_all(line.as_bytes())?;
        self.stream.write_all(b"\r\n")?;
        self.stream.flush()
    }

    /// Read one line, parse it, and auto-reply to server PINGs. Returns `Ok(None)`
    /// on a read timeout (no data ready) so the reader loop can stay responsive.
    pub fn poll(&mut self) -> std::io::Result<Option<Message>> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed",
            )),
            Ok(_) => {
                let msg = Message::parse(&line);
                if let Some(m) = &msg {
                    if m.command == "PING" {
                        let token = m.params.first().cloned().unwrap_or_default();
                        let _ = self.send(&cmd_pong(&token));
                    }
                }
                Ok(msg)
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_privmsg_with_prefix_and_trailing() {
        let m = Message::parse(":alice!a@host PRIVMSG #rust :hello there\r\n").unwrap();
        assert_eq!(m.prefix.as_deref(), Some("alice!a@host"));
        assert_eq!(m.command, "PRIVMSG");
        assert_eq!(m.params, vec!["#rust", "hello there"]);
        assert_eq!(m.nick(), Some("alice"));
        assert_eq!(m.transcript(), "#rust <alice> hello there");
    }

    #[test]
    fn parses_ping_and_numeric() {
        let ping = Message::parse("PING :server1").unwrap();
        assert_eq!(ping.command, "PING");
        assert_eq!(ping.params, vec!["server1"]);
        let welcome = Message::parse(":srv 001 mynick :Welcome to IRC").unwrap();
        assert_eq!(welcome.command, "001");
        assert_eq!(welcome.params, vec!["mynick", "Welcome to IRC"]);
    }

    #[test]
    fn private_message_has_no_channel_prefix() {
        let m = Message::parse(":bob!b@h PRIVMSG mynick :hi").unwrap();
        assert_eq!(m.transcript(), "<bob> hi");
    }

    #[test]
    fn empty_and_whitespace_lines_are_none() {
        assert_eq!(Message::parse(""), None);
        assert_eq!(Message::parse("\r\n"), None);
    }

    #[test]
    fn command_formatters() {
        assert_eq!(cmd_nick("z"), "NICK z");
        assert_eq!(cmd_user("z", "Z User"), "USER z 0 * :Z User");
        assert_eq!(cmd_join("#chan"), "JOIN #chan");
        assert_eq!(cmd_privmsg("#chan", "hi all"), "PRIVMSG #chan :hi all");
        assert_eq!(cmd_pong("tok"), "PONG :tok");
    }

    #[test]
    fn join_part_quit_transcripts() {
        assert_eq!(
            Message::parse(":n!u@h JOIN #c").unwrap().transcript(),
            "* n joined #c"
        );
        assert_eq!(
            Message::parse(":n!u@h QUIT :bye").unwrap().transcript(),
            "* n quit (bye)"
        );
    }
}
