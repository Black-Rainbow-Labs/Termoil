//! Crash reports (`docs/TDD.md` section 4.8): a panic writes a report with the message, the place,
//! the thread, the version and a backtrace to a file under the data folder. Nothing is ever sent
//! anywhere.

use std::io::Write;

/// Installs the panic hook. The default hook still runs after the report is written.
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(path) = write_report(&info.to_string()) {
            eprintln!("termoil: crash report written to {}", path.display());
        }
        default(info);
    }));
}

/// Writes one report and returns its path.
fn write_report(message: &str) -> Option<std::path::PathBuf> {
    let dir = crate::paths::data_dir()?.join("crashes");
    std::fs::create_dir_all(&dir).ok()?;
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = dir.join(format!("crash-{secs}-{}.txt", std::process::id()));
    let mut f = std::fs::File::create(&path).ok()?;
    let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
    let _ = writeln!(
        f,
        "Termoil {} ({})",
        termoil_protocol::VERSION,
        option_env!("TERMOIL_BUILD_DATE").unwrap_or("development build")
    );
    let _ = writeln!(f, "thread: {thread}\n{message}\n");
    let _ = writeln!(f, "{}", std::backtrace::Backtrace::force_capture());
    Some(path)
}
