//! Unicode table generator. `generate` writes `crates/termoil-vt/src/unicode/tables.rs` from the vendored data
//! files in `corpus/unicode/<version>/`; `check` exits non zero when the checked in file is stale.

mod trie;
mod ucd;

use std::path::PathBuf;
use std::process::ExitCode;

/// The Unicode version of the vendored data files.
const VERSION: &str = "17.0.0";

/// Root of the workspace, found relative to this crate.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Path of the generated tables file.
fn output_path() -> PathBuf {
    workspace_root().join("crates/termoil-vt/src/unicode/tables.rs")
}

/// Generates the tables source text in memory from the vendored data files.
fn generate_source() -> String {
    let dir = workspace_root().join("corpus/unicode").join(VERSION);
    let data = ucd::load(&dir);
    trie::render(&trie::build(&data), VERSION)
}

/// Normalises line endings so a checkout with CRLF conversion still compares equal.
fn normalise(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Compares the generated source with the checked in file; true when they are identical.
fn is_current() -> bool {
    match std::fs::read_to_string(output_path()) {
        Ok(existing) => normalise(&existing) == generate_source(),
        Err(_) => false,
    }
}

/// Entry point: `generate` writes the file, `check` verifies it.
fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("generate") => {
            let source = generate_source();
            std::fs::write(output_path(), &source).expect("cannot write tables.rs");
            println!("wrote {} ({} bytes)", output_path().display(), source.len());
            ExitCode::SUCCESS
        }
        Some("check") if is_current() => ExitCode::SUCCESS,
        Some("check") => {
            eprintln!("tables.rs is stale: run `cargo run -p unicode-tables -- generate`");
            ExitCode::FAILURE
        }
        _ => {
            eprintln!("usage: unicode-tables generate|check");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    /// The checked in tables.rs must equal what the generator produces from the vendored data files.
    #[test]
    fn checked_in_tables_are_current() {
        assert!(super::is_current(), "crates/termoil-vt/src/unicode/tables.rs is stale");
    }
}
