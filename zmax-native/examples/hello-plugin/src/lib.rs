//! Example native zmax plugin. Build with `cargo build` to produce
//! `target/debug/libzmax_native_hello.{dylib,so}`, then inside zmax:
//!
//! ```text
//! :plugin load ~/.../libzmax_native_hello.dylib
//! :hello world          # → status line "hello, world (buffer has N chars)"
//! :hello-insert         # → inserts a line at the cursor
//! :plugin list
//! :plugin unload hello
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// `:hello [args…]` — greet on the status line and report the buffer size,
/// exercising `message` + `buffer_text`.
fn hello(host: &Host, args: &Args) -> c_int {
    let who = args.rest().join(" ");
    let who = if who.is_empty() { "world".to_string() } else { who };
    let chars = host.buffer_text().map(|t| t.chars().count()).unwrap_or(0);
    host.message(&format!("hello, {who} (buffer has {chars} chars)"));
    0
}

/// `:hello-insert` — insert a line at the cursor, exercising `insert_text`.
fn hello_insert(host: &Host, _args: &Args) -> c_int {
    if host.insert_text("-- inserted by the hello plugin --\n") {
        0
    } else {
        host.error("hello-insert: no active buffer");
        1
    }
}

/// `:hello-echo <cmd…>` — run a `:` command line, exercising `eval`.
fn hello_echo(host: &Host, args: &Args) -> c_int {
    let line = args.rest().join(" ");
    if line.is_empty() {
        host.error("hello-echo: expected a command line");
        return 1;
    }
    host.eval(&line)
}

declare_plugin! {
    name: "hello",
    version: "0.1.0",
    commands: {
        "hello" => hello,
        "hello-insert" => hello_insert,
        "hello-echo" => hello_echo,
    },
}
