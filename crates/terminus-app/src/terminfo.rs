//! The `TERM` value children get on Unix, and the terminfo entry behind it (PRD 5.1:
//! "a terminfo entry `xterm-terminus` shipped with the binary").
//!
//! A program that cannot find the entry its `TERM` names falls back to a dumb terminal or refuses
//! to start, so `xterm-terminus` is only given when the entry can be found. The first session of a
//! process looks for it in the standard terminfo folders; when it is missing and `tic` is
//! installed (macOS has it, as do most Linux systems), the shipped source is compiled into
//! Terminus's data folder and `TERMINFO_DIRS` points there. When neither works, children get
//! `xterm-256color`, which every system has.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// The entry name.
pub const NAME: &str = "xterm-terminus";

/// The fallback when the entry cannot be found or compiled.
pub const FALLBACK: &str = "xterm-256color";

/// What children get: the `TERM` value and, when Terminus compiled the entry itself, the folder
/// to add to `TERMINFO_DIRS`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermChoice {
    /// The value of `TERM`.
    pub term: String,
    /// A folder holding the compiled entry, to put first in `TERMINFO_DIRS`.
    pub terminfo_dir: Option<PathBuf>,
}

/// The choice for this process, worked out once.
pub fn choice() -> &'static TermChoice {
    static CHOICE: OnceLock<TermChoice> = OnceLock::new();
    CHOICE.get_or_init(|| {
        let dirs = search_dirs(|k| std::env::var_os(k).map(PathBuf::from), home());
        let compiled_into = crate::paths::data_dir().map(|d| d.join("terminfo"));
        let source = crate::shell_integration::source_dir().map(|d| d.join("terminfo").join("xterm-terminus.terminfo"));
        decide(&dirs, compiled_into.as_deref(), source.as_deref(), &compile_with_tic)
    })
}

/// The environment entries for a child: `TERM`, and `TERMINFO_DIRS` when needed. `existing` is
/// the current `TERMINFO_DIRS`, kept after Terminus's folder; an empty entry at the end stands for
/// the system's own folders, as ncurses reads it.
pub fn child_env(choice: &TermChoice, existing: Option<&str>) -> Vec<(String, String)> {
    let mut env = vec![("TERM".to_string(), choice.term.clone())];
    if let Some(dir) = &choice.terminfo_dir {
        let rest = existing.filter(|s| !s.is_empty()).unwrap_or("");
        env.push(("TERMINFO_DIRS".to_string(), format!("{}:{rest}", dir.display())));
    }
    env
}

/// Decides the choice from the folders to search, the folder Terminus compiles into, the entry's
/// source, and a compiler. Separated from `choice` so the rules can be tested.
pub fn decide(
    dirs: &[PathBuf], compiled_into: Option<&Path>, source: Option<&Path>, compile: &dyn Fn(&Path, &Path) -> bool,
) -> TermChoice {
    if dirs.iter().any(|d| has_entry(d)) {
        return TermChoice { term: NAME.into(), terminfo_dir: None };
    }
    if let Some(out) = compiled_into
        && (has_entry(out) || source.is_some_and(|src| compile(src, out) && has_entry(out)))
    {
        return TermChoice { term: NAME.into(), terminfo_dir: Some(out.to_path_buf()) };
    }
    TermChoice { term: FALLBACK.into(), terminfo_dir: None }
}

/// True when `dir` holds a compiled `xterm-terminus`: under `x/` (Linux) or `78/` (macOS, which
/// names the folders by the first letter's hexadecimal code).
pub fn has_entry(dir: &Path) -> bool {
    dir.join("x").join(NAME).is_file() || dir.join("78").join(NAME).is_file()
}

/// The folders ncurses searches, in its order: `$TERMINFO`, `~/.terminfo`, each of
/// `$TERMINFO_DIRS`, then the usual system folders.
pub fn search_dirs(var: impl Fn(&str) -> Option<PathBuf>, home: Option<PathBuf>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    dirs.extend(var("TERMINFO"));
    dirs.extend(home.map(|h| h.join(".terminfo")));
    if let Some(list) = var("TERMINFO_DIRS") {
        dirs.extend(list.to_string_lossy().split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    }
    for d in [
        "/etc/terminfo",
        "/lib/terminfo",
        "/usr/share/terminfo",
        "/usr/lib/terminfo",
        "/usr/local/share/terminfo",
        "/opt/homebrew/share/terminfo",
    ] {
        dirs.push(PathBuf::from(d));
    }
    dirs
}

/// Compiles `source` into `out` with `tic -x`. False when `tic` is missing or fails.
fn compile_with_tic(source: &Path, out: &Path) -> bool {
    if std::fs::create_dir_all(out).is_err() {
        return false;
    }
    Command::new("tic")
        .arg("-x")
        .arg("-o")
        .arg(out)
        .arg(source)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The home folder.
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temporary folder for one test.
    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("terminus-terminfo-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Writes a stand in compiled entry under `dir/sub`.
    fn entry(dir: &Path, sub: &str) {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        std::fs::write(dir.join(sub).join(NAME), b"x").unwrap();
    }

    #[test]
    fn an_installed_entry_is_used_as_it_is() {
        let d = temp("installed");
        entry(&d, "78");
        let c = decide(std::slice::from_ref(&d), None, None, &|_, _| panic!("no compile"));
        assert_eq!(c, TermChoice { term: NAME.into(), terminfo_dir: None });
    }

    #[test]
    fn a_missing_entry_is_compiled_into_the_data_folder() {
        let (sys, out) = (temp("sys"), temp("out"));
        let src = sys.join("src.terminfo");
        let c = decide(std::slice::from_ref(&sys), Some(&out), Some(&src), &|_, o| {
            entry(o, "x");
            true
        });
        assert_eq!(c, TermChoice { term: NAME.into(), terminfo_dir: Some(out.clone()) });
        let env = child_env(&c, Some("/opt/x"));
        assert_eq!(env[1], ("TERMINFO_DIRS".into(), format!("{}:/opt/x", out.display())));
        assert_eq!(child_env(&c, None)[1].1, format!("{}:", out.display()));
    }

    #[test]
    fn without_tic_children_get_xterm_256color() {
        let (sys, out) = (temp("sys2"), temp("out2"));
        let c = decide(&[sys], Some(&out), Some(Path::new("/nowhere")), &|_, _| false);
        assert_eq!(c, TermChoice { term: FALLBACK.into(), terminfo_dir: None });
        assert_eq!(child_env(&c, None), vec![("TERM".to_string(), FALLBACK.to_string())]);
    }

    #[cfg(unix)]
    #[test]
    fn searches_the_folders_ncurses_searches_in_order() {
        let dirs = search_dirs(
            |k| match k {
                "TERMINFO" => Some("/t".into()),
                "TERMINFO_DIRS" => Some("/a::/b".into()),
                _ => None,
            },
            Some("/home/me".into()),
        );
        let first: Vec<String> = dirs.iter().take(4).map(|d| d.display().to_string()).collect();
        assert_eq!(first, ["/t", "/home/me/.terminfo", "/a", "/b"]);
    }
}
