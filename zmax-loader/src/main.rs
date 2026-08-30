use anyhow::Result;
use zmax_loader::grammar::fetch_grammars;

// Fetches every grammar in one process, so a release build pays the clone cost
// once instead of per-grammar. Not meant to be run manually: `zmax -g fetch` is
// the supported entry point and shares the same `fetch_grammars`.

const STRICT: bool = true;

fn main() -> Result<()> {
    fetch_grammars(STRICT)
}
