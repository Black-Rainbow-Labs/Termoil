//! Shell integration: working out which shell a program is, how Terminus starts it with the
//! integration scripts loaded, and the `+install-shell-integration` command that copies the
//! scripts into the user's data folder and adds one guarded line to the shell's startup file.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The text on the first line of a block this module adds to a startup file. Its presence is how
/// a second install knows the file is already set up.
const MARKER: &str = "# terminus shell integration";

/// The shells Terminus has integration scripts for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// Windows PowerShell 5.1 (`powershell.exe`).
    PowerShell,
    /// PowerShell 7 and later (`pwsh`).
    Pwsh,
    Bash,
    Zsh,
    Fish,
    Nushell,
    /// `cmd.exe`, which can only report the working directory and prompt marks.
    Cmd,
    Unknown,
}

/// The arguments and the extra environment variables one shell needs.
type Parts = (Vec<String>, Vec<(String, String)>);

/// How to start a shell with the integration loaded: the arguments to pass and the environment
/// variables to add to the child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Injection {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Names the shell a program path refers to, by file name only. Case and the `.exe` suffix are
/// ignored, and a leading `-` (how a login shell is named) is dropped.
pub fn detect(program: &str) -> Shell {
    let file = program.rsplit(['/', '\\']).next().unwrap_or(program).to_ascii_lowercase();
    let file = file.trim_start_matches('-');
    match file.strip_suffix(".exe").unwrap_or(file) {
        "pwsh" | "pwsh-preview" => Shell::Pwsh,
        "powershell" => Shell::PowerShell,
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        "nu" => Shell::Nushell,
        "cmd" => Shell::Cmd,
        _ => Shell::Unknown,
    }
}

/// How the terminal starts `shell` with integration loaded from `dir` (the installed
/// shell-integration folder), without the user editing a startup file. Returns `None` when the
/// shell has no integration or `user_args` make it non interactive (a command or script to run).
/// Always sets `TERMINUS_SHELL_INTEGRATION=1` and `TERMINUS_SHELL_INTEGRATION_DIR=<dir>`.
pub fn injection(shell: Shell, dir: &Path, user_args: &[String]) -> Option<Injection> {
    let (args, mut extra) = match shell {
        Shell::PowerShell | Shell::Pwsh => powershell_parts(dir, user_args)?,
        Shell::Bash => bash_parts(dir, user_args)?,
        Shell::Zsh => zsh_parts(dir, user_args),
        Shell::Fish => fish_parts(dir, user_args),
        Shell::Nushell => nushell_parts(dir, user_args)?,
        Shell::Cmd => cmd_parts(dir, user_args)?,
        Shell::Unknown => return None,
    };
    let mut env = base_env(dir);
    env.append(&mut extra);
    Some(Injection { args, env })
}

/// The variables every integrated shell gets: the switch the scripts check and the folder they
/// were loaded from.
fn base_env(dir: &Path) -> Vec<(String, String)> {
    vec![
        ("TERMINUS_SHELL_INTEGRATION".to_string(), "1".to_string()),
        ("TERMINUS_SHELL_INTEGRATION_DIR".to_string(), dir.to_string_lossy().into_owned()),
    ]
}

/// A path with forward slashes, which every shell here (including Git Bash and nushell on
/// Windows) accepts.
fn forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// True when `args` holds any of `flags`, compared ignoring case.
fn has_flag(args: &[String], flags: &[&str]) -> bool {
    args.iter().any(|a| flags.iter().any(|f| a.eq_ignore_ascii_case(f)))
}

/// PowerShell and pwsh: keep the user's own arguments, load the profile as usual, and run the
/// script last with `-NoExit`. The script is read and run as a script block so a restricted
/// execution policy (the Windows PowerShell default on a client) does not block it. A
/// `-NoProfile` from the caller is dropped because the profile must load before the script.
fn powershell_parts(dir: &Path, user_args: &[String]) -> Option<Parts> {
    let running = ["-Command", "-c", "-File", "-f", "-EncodedCommand", "-ec", "-NonInteractive", "-NonI"];
    if has_flag(user_args, &running) {
        return None;
    }
    let skipped = ["-NoProfile", "-NoP", "-NoExit", "-NoE"];
    let mut args: Vec<String> =
        user_args.iter().filter(|a| !has_flag(std::slice::from_ref(*a), &skipped)).cloned().collect();
    let script = dir.join("powershell").join("terminus.ps1").to_string_lossy().replace('\'', "''");
    args.push("-NoExit".to_string());
    args.push("-Command".to_string());
    args.push(format!(". ([scriptblock]::Create([IO.File]::ReadAllText('{script}')))"));
    Some((args, Vec::new()))
}

/// bash: `--rcfile` (first, before any short option) a wrapper that loads the user's `~/.bashrc` and then the script. A login
/// shell request (`-l`) cannot use `--rcfile`, so it is turned into `TERMINUS_BASH_LOGIN=1` and
/// the wrapper reads the login files instead. Anything that runs a command or a script returns `None`.
fn bash_parts(dir: &Path, user_args: &[String]) -> Option<Parts> {
    let refused = ["-c", "--rcfile", "--init-file", "--norc", "--posix", "-r", "--restricted"];
    if has_flag(user_args, &refused) {
        return None;
    }
    // bash reads long options such as --rcfile only before the short ones, so --rcfile goes first.
    let rc = forward_slashes(&dir.join("bash").join("terminus-rc.bash"));
    let mut args = vec!["--rcfile".to_string(), rc];
    let mut env = Vec::new();
    let mut previous_takes_value = false;
    for arg in user_args {
        if arg == "-l" || arg == "--login" {
            env.push(("TERMINUS_BASH_LOGIN".to_string(), "1".to_string()));
        } else if !arg.starts_with(['-', '+']) && !previous_takes_value {
            return None;
        } else {
            args.push(arg.clone());
        }
        previous_takes_value = arg == "-o" || arg == "-O";
    }
    Some((args, env))
}

/// zsh: `ZDOTDIR` points at the folder holding the `.zshenv` shim, which gives `ZDOTDIR` back to
/// the user's value (saved here in `TERMINUS_ORIG_ZDOTDIR`, empty when it was unset).
fn zsh_parts(dir: &Path, user_args: &[String]) -> Parts {
    let original = std::env::var("ZDOTDIR").unwrap_or_default();
    let env = vec![
        ("ZDOTDIR".to_string(), dir.join("zsh").to_string_lossy().into_owned()),
        ("TERMINUS_ORIG_ZDOTDIR".to_string(), original),
    ];
    (user_args.to_vec(), env)
}

/// fish: the integration folder goes first in `XDG_DATA_DIRS`, and fish loads
/// `fish/vendor_conf.d/terminus.fish` from it.
fn fish_parts(dir: &Path, user_args: &[String]) -> Parts {
    let existing = std::env::var("XDG_DATA_DIRS").ok().filter(|v| !v.is_empty());
    let rest = existing.unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    let value = format!("{}:{rest}", dir.to_string_lossy());
    (user_args.to_vec(), vec![("XDG_DATA_DIRS".to_string(), value)])
}

/// nushell: `--execute` sources the script after the user's config has loaded, then stays interactive.
fn nushell_parts(dir: &Path, user_args: &[String]) -> Option<Parts> {
    if has_flag(user_args, &["-c", "--commands", "-e", "--execute", "--lsp", "--mcp"]) {
        return None;
    }
    let mut args = user_args.to_vec();
    let script = forward_slashes(&dir.join("nushell").join("terminus.nu")).replace('\'', "");
    args.push("--execute".to_string());
    args.push(format!("source '{script}'"));
    Some((args, Vec::new()))
}

/// cmd: `/k` runs the script and stays open. A caller that already passes `/c` or `/k` gets `None`.
fn cmd_parts(dir: &Path, user_args: &[String]) -> Option<Parts> {
    if has_flag(user_args, &["/c", "/k", "/r"]) {
        return None;
    }
    let mut args = user_args.to_vec();
    args.push("/k".to_string());
    args.push(dir.join("cmd").join("terminus.cmd").to_string_lossy().into_owned());
    Some((args, Vec::new()))
}

/// The places `install` writes to. Built from the real environment by `from_environment`, or
/// from a chosen home folder by `under_home` so a test never touches the real one.
#[derive(Debug, Clone)]
pub struct InstallPaths {
    /// Where the scripts are copied to.
    pub data_dir: PathBuf,
    /// The profile of Windows PowerShell 5.1.
    pub powershell_profile: PathBuf,
    /// The profile of PowerShell 7.
    pub pwsh_profile: PathBuf,
    pub bashrc: PathBuf,
    pub zshrc: PathBuf,
    /// A file in fish's `conf.d` that Terminus owns.
    pub fish_conf: PathBuf,
    pub nushell_config: PathBuf,
    /// Where `tic` writes the compiled terminfo entry.
    pub terminfo_dir: PathBuf,
}

impl InstallPaths {
    /// The locations for the current user: `%LOCALAPPDATA%` and the Documents folder on Windows,
    /// `~/Library/Application Support` on macOS and `~/.local/share` elsewhere.
    pub fn from_environment() -> Result<InstallPaths, String> {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        if cfg!(windows) {
            let profile = var("USERPROFILE").ok_or("USERPROFILE is not set")?;
            let local = var("LOCALAPPDATA").unwrap_or_else(|| profile.join("AppData").join("Local"));
            let roaming = var("APPDATA").unwrap_or_else(|| profile.join("AppData").join("Roaming"));
            let mut paths = InstallPaths::under_home(&profile);
            paths.data_dir = local.join("terminus").join("shell-integration");
            paths.nushell_config = roaming.join("nushell").join("config.nu");
            return Ok(paths);
        }
        let home = var("HOME").ok_or("HOME is not set")?;
        Ok(InstallPaths::under_home(&home))
    }

    /// The Unix style layout under `home`, also used on Windows for the files that live in the
    /// user profile.
    pub fn under_home(home: &Path) -> InstallPaths {
        let docs = home.join("Documents");
        let mac = cfg!(target_os = "macos");
        let support = home.join("Library").join("Application Support");
        let windows = cfg!(windows);
        InstallPaths {
            data_dir: if mac { support.join("terminus") } else { home.join(".local").join("share").join("terminus") }
                .join("shell-integration"),
            powershell_profile: docs.join("WindowsPowerShell").join("Microsoft.PowerShell_profile.ps1"),
            pwsh_profile: if windows { docs.join("PowerShell") } else { home.join(".config").join("powershell") }
                .join("Microsoft.PowerShell_profile.ps1"),
            bashrc: home.join(".bashrc"),
            zshrc: home.join(".zshrc"),
            fish_conf: home.join(".config").join("fish").join("conf.d").join("terminus.fish"),
            nushell_config: if mac { support.join("nushell") } else { home.join(".config").join("nushell") }
                .join("config.nu"),
            terminfo_dir: home.join(".terminfo"),
        }
    }
}

/// `terminus +install-shell-integration`: copies the scripts into the user's data folder and,
/// only for the shells named, appends one guarded line to the startup file that sources them,
/// never twice. On Unix it also compiles the terminfo entry with `tic -x` when `tic` exists.
/// With `dry_run` nothing is written. Returns a line for everything it changed or would change.
pub fn install(shells: &[Shell], source_dir: &Path, dry_run: bool) -> Result<Vec<String>, String> {
    install_with(shells, source_dir, dry_run, &InstallPaths::from_environment()?)
}

/// `install` with the destination folders chosen by the caller.
pub fn install_with(
    shells: &[Shell], source_dir: &Path, dry_run: bool, paths: &InstallPaths,
) -> Result<Vec<String>, String> {
    if !source_dir.join("bash").join("terminus.bash").is_file() {
        return Err(format!("{} does not hold the shell integration scripts", source_dir.display()));
    }
    let verb = if dry_run { "would copy" } else { "copied" };
    let mut report = vec![format!("{verb} the scripts to {}", paths.data_dir.display())];
    if !dry_run {
        copy_tree(source_dir, &paths.data_dir)?;
    }
    let mut done: Vec<Shell> = Vec::new();
    for shell in shells {
        if !done.contains(shell) {
            done.push(*shell);
            report.push(add_startup_line(*shell, paths, dry_run)?);
        }
    }
    if cfg!(unix) {
        report.push(compile_terminfo(paths, dry_run));
    }
    Ok(report)
}

/// Copies every file under `from` into `to`, creating folders, replacing files that exist.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("cannot create {}: {e}", to.display()))?;
    let entries = fs::read_dir(from).map_err(|e| format!("cannot read {}: {e}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(|e| format!("cannot copy to {}: {e}", target.display()))?;
        }
    }
    Ok(())
}

/// Adds the line that sources the integration for `shell` to its startup file unless the file
/// already holds the marker. Returns one line saying what happened.
fn add_startup_line(shell: Shell, paths: &InstallPaths, dry_run: bool) -> Result<String, String> {
    let Some((file, line)) = startup_line(shell, paths) else {
        return Ok(format!("{shell:?}: nothing to add, Terminus starts it with the integration loaded"));
    };
    let existing = fs::read_to_string(&file).unwrap_or_default();
    if existing.contains(MARKER) {
        return Ok(format!("{shell:?}: {} already sources the integration", file.display()));
    }
    if dry_run {
        return Ok(format!("{shell:?}: would add a line to {}", file.display()));
    }
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let mut handle =
        fs::OpenOptions::new().create(true).append(true).open(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let separator = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    write!(handle, "{separator}{MARKER}\n{line}\n").map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(format!("{shell:?}: added a line to {}", file.display()))
}

/// The startup file for `shell` and the guarded line to put in it, or `None` for a shell that
/// needs no file (cmd and unknown shells).
fn startup_line(shell: Shell, paths: &InstallPaths) -> Option<(PathBuf, String)> {
    let data = &paths.data_dir;
    let script = |sub: &[&str]| sub.iter().fold(data.clone(), |p, s| p.join(s));
    match shell {
        Shell::PowerShell | Shell::Pwsh => {
            let file = if shell == Shell::Pwsh { &paths.pwsh_profile } else { &paths.powershell_profile };
            let p = script(&["powershell", "terminus.ps1"]).to_string_lossy().replace('\'', "''");
            let line = format!("if (Test-Path '{p}') {{ . ([scriptblock]::Create([IO.File]::ReadAllText('{p}'))) }}");
            Some((file.clone(), line))
        }
        Shell::Bash => {
            let p = forward_slashes(&script(&["bash", "terminus.bash"]));
            Some((paths.bashrc.clone(), format!("[ -f \"{p}\" ] && . \"{p}\"")))
        }
        Shell::Zsh => {
            let p = forward_slashes(&script(&["zsh", "terminus.zsh"]));
            Some((paths.zshrc.clone(), format!("[[ -f \"{p}\" ]] && source \"{p}\"")))
        }
        Shell::Fish => {
            let p = forward_slashes(&script(&["fish", "vendor_conf.d", "terminus.fish"]));
            Some((paths.fish_conf.clone(), format!("test -f '{p}'; and source '{p}'")))
        }
        Shell::Nushell => {
            let p = forward_slashes(&script(&["nushell", "terminus.nu"]));
            Some((paths.nushell_config.clone(), format!("source \"{p}\"")))
        }
        Shell::Cmd | Shell::Unknown => None,
    }
}

/// Compiles the terminfo entry into `~/.terminfo` with `tic -x` when `tic` is installed.
fn compile_terminfo(paths: &InstallPaths, dry_run: bool) -> String {
    let source = paths.data_dir.join("terminfo").join("xterm-terminus.terminfo");
    let available =
        Command::new("tic").arg("-V").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    if !available {
        return "terminfo: tic is not installed, xterm-terminus was not compiled".to_string();
    }
    if dry_run {
        return format!("terminfo: would compile {} into {}", source.display(), paths.terminfo_dir.display());
    }
    let result = Command::new("tic").arg("-x").arg("-o").arg(&paths.terminfo_dir).arg(&source).output();
    match result {
        Ok(out) if out.status.success() => {
            format!("terminfo: compiled xterm-terminus into {}", paths.terminfo_dir.display())
        }
        Ok(out) => format!("terminfo: tic failed: {}", String::from_utf8_lossy(&out.stderr).trim()),
        Err(e) => format!("terminfo: could not run tic: {e}"),
    }
}

/// Whether `dir` holds the integration scripts.
fn is_script_folder(dir: &Path) -> bool {
    dir.join("bash").join("terminus.bash").is_file()
}

/// The scripts' source folder: `shell-integration` next to the executable (the installed layout)
/// or, in a development build, the repository's `shell-integration` folder, found by looking in
/// each parent of the executable and then relative to this crate's sources.
pub fn source_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut candidates: Vec<PathBuf> = exe.ancestors().skip(1).take(6).map(|a| a.join("shell-integration")).collect();
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        candidates.push(Path::new(manifest).join("..").join("..").join("shell-integration"));
    }
    candidates.into_iter().find(|c| is_script_folder(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder under the system temp directory that is removed when the test ends.
    struct TempDir(PathBuf);

    impl TempDir {
        /// Creates an empty folder whose name includes `tag` and the process id.
        fn new(tag: &str) -> TempDir {
            let path = std::env::temp_dir().join(format!("terminus-si-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        /// Removes the folder.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A small copy of the scripts folder layout, enough for `install_with` to accept.
    fn fake_scripts(root: &Path) -> PathBuf {
        let dir = root.join("scripts");
        for sub in ["bash", "powershell", "terminfo"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        fs::write(dir.join("bash").join("terminus.bash"), "# bash\n").unwrap();
        fs::write(dir.join("powershell").join("terminus.ps1"), "# ps\n").unwrap();
        dir
    }

    /// Looks up an environment entry of an injection.
    fn env_of(inj: &Injection, key: &str) -> Option<String> {
        inj.env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    /// `detect` works by file name, any path, any case, with or without `.exe`.
    #[test]
    fn detect_by_file_name() {
        assert_eq!(detect("C:\\Program Files\\PowerShell\\7\\pwsh.exe"), Shell::Pwsh);
        assert_eq!(detect("C:/Windows/System32/WindowsPowerShell/v1.0/PowerShell.EXE"), Shell::PowerShell);
        assert_eq!(detect("/usr/bin/bash"), Shell::Bash);
        assert_eq!(detect("C:/Program Files/Git/bin/bash.exe"), Shell::Bash);
        assert_eq!(detect("-zsh"), Shell::Zsh);
        assert_eq!(detect("/usr/bin/fish"), Shell::Fish);
        assert_eq!(detect("nu.exe"), Shell::Nushell);
        assert_eq!(detect("C:\\WINDOWS\\system32\\CMD.EXE"), Shell::Cmd);
        assert_eq!(detect("/bin/sh"), Shell::Unknown);
        assert_eq!(detect(""), Shell::Unknown);
    }

    /// Every injection sets the two Terminus variables.
    #[test]
    fn injection_sets_the_switch_and_folder() {
        let dir = Path::new("/opt/terminus/shell-integration");
        for shell in [Shell::PowerShell, Shell::Pwsh, Shell::Bash, Shell::Zsh, Shell::Fish, Shell::Nushell, Shell::Cmd]
        {
            let inj = injection(shell, dir, &[]).unwrap_or_else(|| panic!("{shell:?}"));
            assert_eq!(env_of(&inj, "TERMINUS_SHELL_INTEGRATION").as_deref(), Some("1"), "{shell:?}");
            assert_eq!(
                env_of(&inj, "TERMINUS_SHELL_INTEGRATION_DIR").as_deref(),
                Some("/opt/terminus/shell-integration")
            );
        }
        assert!(injection(Shell::Unknown, dir, &[]).is_none());
    }

    /// PowerShell loads the profile and then the script, and drops `-NoProfile`.
    #[test]
    fn powershell_injection_loads_profile_then_script() {
        let dir = Path::new("C:\\Program Files\\Terminus\\shell-integration");
        let inj = injection(Shell::Pwsh, dir, &["-NoLogo".to_string(), "-NoProfile".to_string()]).unwrap();
        assert_eq!(inj.args[0], "-NoLogo");
        assert!(!inj.args.iter().any(|a| a == "-NoProfile"));
        assert_eq!(&inj.args[1..3], ["-NoExit", "-Command"]);
        assert!(inj.args[3].contains("powershell\\terminus.ps1"), "{}", inj.args[3]);
        assert!(injection(Shell::Pwsh, dir, &["-Command".to_string(), "dir".to_string()]).is_none());
    }

    /// bash gets a `--rcfile` wrapper, a login request becomes an environment variable, and a
    /// command to run is left alone.
    #[test]
    fn bash_injection() {
        let dir = Path::new("C:\\ti");
        let inj = injection(Shell::Bash, dir, &[]).unwrap();
        assert_eq!(inj.args, ["--rcfile", "C:/ti/bash/terminus-rc.bash"]);
        let login = injection(Shell::Bash, dir, &["--login".to_string(), "-i".to_string()]).unwrap();
        assert_eq!(login.args, ["--rcfile", "C:/ti/bash/terminus-rc.bash", "-i"]);
        assert_eq!(env_of(&login, "TERMINUS_BASH_LOGIN").as_deref(), Some("1"));
        assert!(injection(Shell::Bash, dir, &["-c".to_string(), "ls".to_string()]).is_none());
        assert!(injection(Shell::Bash, dir, &["build.sh".to_string()]).is_none());
    }

    /// zsh, fish, nushell and cmd each use their own mechanism.
    #[test]
    fn other_shell_injections() {
        let dir = Path::new("/ti");
        let zsh = injection(Shell::Zsh, dir, &[]).unwrap();
        assert!(env_of(&zsh, "ZDOTDIR").unwrap().ends_with("zsh"));
        assert!(env_of(&zsh, "TERMINUS_ORIG_ZDOTDIR").is_some());
        let fish = injection(Shell::Fish, dir, &[]).unwrap();
        assert!(env_of(&fish, "XDG_DATA_DIRS").unwrap().starts_with("/ti:"));
        let nu = injection(Shell::Nushell, dir, &[]).unwrap();
        assert_eq!(nu.args, ["--execute", "source '/ti/nushell/terminus.nu'"]);
        let cmd = injection(Shell::Cmd, dir, &[]).unwrap();
        assert_eq!(cmd.args[0], "/k");
        assert!(cmd.args[1].ends_with("terminus.cmd"));
        assert!(injection(Shell::Cmd, dir, &["/c".to_string(), "dir".to_string()]).is_none());
    }

    /// A dry run reports what it would do and creates nothing.
    #[test]
    fn install_dry_run_writes_nothing() {
        let tmp = TempDir::new("dry");
        let scripts = fake_scripts(&tmp.0);
        let paths = InstallPaths::under_home(&tmp.0.join("home"));
        let report = install_with(&[Shell::Bash, Shell::Pwsh, Shell::Cmd], &scripts, true, &paths).unwrap();
        assert!(report.iter().any(|l| l.starts_with("would copy")), "{report:?}");
        assert!(report.iter().any(|l| l.contains("Bash") && l.contains("would add")), "{report:?}");
        assert!(!tmp.0.join("home").exists());
    }

    /// A real install into a temp home copies the scripts and adds each line once.
    #[test]
    fn install_adds_one_line_and_not_twice() {
        let tmp = TempDir::new("real");
        let scripts = fake_scripts(&tmp.0);
        let paths = InstallPaths::under_home(&tmp.0.join("home"));
        fs::create_dir_all(tmp.0.join("home")).unwrap();
        fs::write(&paths.bashrc, "alias ll='ls -l'").unwrap();
        install_with(&[Shell::Bash, Shell::Pwsh, Shell::Bash], &scripts, false, &paths).unwrap();
        install_with(&[Shell::Bash, Shell::Pwsh], &scripts, false, &paths).unwrap();
        assert!(paths.data_dir.join("bash").join("terminus.bash").is_file());
        let bashrc = fs::read_to_string(&paths.bashrc).unwrap();
        assert!(bashrc.starts_with("alias ll='ls -l'\n"), "{bashrc}");
        assert_eq!(bashrc.matches(MARKER).count(), 1, "{bashrc}");
        let profile = fs::read_to_string(&paths.pwsh_profile).unwrap();
        assert_eq!(profile.matches(MARKER).count(), 1, "{profile}");
        assert!(!paths.zshrc.exists(), "a shell that was not named must not be touched");
    }

    /// A folder without the scripts is refused.
    #[test]
    fn install_refuses_a_folder_without_scripts() {
        let tmp = TempDir::new("empty");
        let paths = InstallPaths::under_home(&tmp.0.join("home"));
        assert!(install_with(&[Shell::Bash], &tmp.0, false, &paths).is_err());
    }
}
