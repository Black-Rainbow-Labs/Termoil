//! `termoil-bench apps`: the whole application measured against Ghostty on the same machine and the
//! same workloads (task-2481, `docs/decisions/efficiency.md`).
//!
//! Each terminal is started inside a job object, so every process it creates is found. Processes
//! fall into three classes: the application (the terminal's own executable), the pseudoconsole host
//! (`conhost.exe`, or the `OpenConsole.exe` Termoil ships), and the shell with whatever it runs.
//! Both terminals run the same shell and the same command, so the shell is reported and left out of
//! the comparison. The goal is stated on the application; the host is reported beside it.
//!
//! Workloads:
//! - `idle`: the window open at a `cmd.exe` prompt. Memory once the start has settled, and the CPU
//!   time used over the idle period.
//! - `stream`: a batch file waits until the window has settled, writes a start marker, `type`s a file
//!   of the `mixed` gate stream and writes a done marker. CPU time between the two markers, the peak
//!   memory over the whole run, and the memory once it has settled after the output.
//!
//! Memory is three numbers summed over a class: private bytes (commit), the working set, and the
//! private working set (the pages only this process uses, which Task Manager's Memory column shows).
//! GPU memory that the driver keeps on the card is in none of them.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList,
    QueryInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX, QueryWorkingSet,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    QueryFullProcessImageNameW,
};
use windows::core::PWSTR;

/// One terminal under test: its name and how to start it with a command line for its shell.
pub struct Subject {
    pub name: String,
    pub exe: PathBuf,
    /// Arguments before the shell command.
    pub prefix: Vec<String>,
    /// The flag that carries the shell command: `-e` is passed as its own argument, a flag ending
    /// in `=` is joined to the command (`--command=...`).
    pub command_flag: String,
    /// Environment variables set for this terminal only.
    pub env: Vec<(String, String)>,
}

/// Which part of the terminal a process is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    App,
    Host,
    Shell,
}

/// One process seen in a job: its class and last readings.
#[derive(Clone)]
struct Seen {
    pid: u32,
    class: Class,
    cpu: f64,
    private: u64,
    working: u64,
    peak_private: u64,
    peak_working: u64,
}

/// Totals for one class of processes.
#[derive(Clone, Copy, Default, Debug)]
struct Totals {
    cpu_s: f64,
    private_mb: f64,
    working_mb: f64,
    peak_private_mb: f64,
    peak_working_mb: f64,
}

/// A running terminal inside its job object, with the processes seen so far.
struct Running {
    job: HANDLE,
    app_name: String,
    seen: Vec<(HANDLE, Seen)>,
}

/// Seconds in a FILETIME of 100 ns units.
fn secs(t: FILETIME) -> f64 {
    ((u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)) as f64 / 1e7
}

/// The executable file name of a process, lower case.
fn image_name(h: HANDLE) -> String {
    let mut buf = [0u16; 520];
    let mut len = buf.len() as u32;
    // SAFETY: the buffer and its length are valid for the call.
    let ok = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) };
    if ok.is_err() {
        return String::new();
    }
    let full = String::from_utf16_lossy(&buf[..len as usize]);
    full.rsplit('\\').next().unwrap_or("").to_lowercase()
}

/// The class of a process called `name` in a terminal whose executable is `app`.
fn classify(name: &str, app: &str) -> Class {
    if name == app {
        Class::App
    } else if name == "conhost.exe" || name == "openconsole.exe" {
        Class::Host
    } else {
        Class::Shell
    }
}

/// Reads CPU seconds and memory counters of the process behind `h` into `s`.
fn read(h: HANDLE, s: &mut Seen) {
    let (mut c, mut e, mut k, mut u) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: four valid out pointers.
    if unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) }.is_ok() {
        s.cpu = secs(k) + secs(u);
    }
    let mut m = PROCESS_MEMORY_COUNTERS_EX::default();
    // SAFETY: the struct is the size passed.
    let ok = unsafe {
        GetProcessMemoryInfo(
            h,
            (&raw mut m).cast::<PROCESS_MEMORY_COUNTERS>(),
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };
    if ok.is_ok() {
        s.private = m.PrivateUsage as u64;
        s.working = m.WorkingSetSize as u64;
        s.peak_private = s.peak_private.max(m.PeakPagefileUsage as u64);
        s.peak_working = s.peak_working.max(m.PeakWorkingSetSize as u64);
    }
}

/// The private working set of the process behind `h` in bytes: the resident pages it shares with
/// no other process.
fn private_working_set(h: HANDLE) -> u64 {
    let mut buf: Vec<usize> = vec![0; 1 << 16];
    loop {
        // SAFETY: the buffer and its size in bytes are passed together.
        let ok = unsafe { QueryWorkingSet(h, buf.as_mut_ptr().cast(), (buf.len() * size_of::<usize>()) as u32) };
        let entries = buf[0];
        if ok.is_ok() {
            // Each entry: bits 0-4 protection, 5-7 share count, 8 shared, 12 and up the page.
            return buf[1..=entries.min(buf.len() - 1)].iter().filter(|e| (*e >> 8) & 1 == 0).count() as u64 * 4096;
        }
        if entries < buf.len() || buf.len() > 1 << 24 {
            return 0;
        }
        buf.resize(entries + 1024, 0);
    }
}

impl Running {
    /// Starts `subject` running `shell_command` inside a new job object.
    fn start(subject: &Subject, shell_command: &str) -> Running {
        // SAFETY: an unnamed job with default security.
        let job = unsafe { CreateJobObjectW(None, None) }.expect("create a job object");
        let mut cmd = std::process::Command::new(&subject.exe);
        cmd.args(&subject.prefix);
        if subject.command_flag.ends_with('=') {
            cmd.arg(format!("{}{shell_command}", subject.command_flag));
        } else {
            cmd.args([subject.command_flag.as_str(), shell_command]);
        }
        // A measured Termoil sends no usage statistics; other terminals ignore the variable.
        cmd.env("TERMOIL_TELEMETRY", "off");
        for (k, v) in &subject.env {
            cmd.env(k, v);
        }
        let child = cmd.spawn().unwrap_or_else(|e| panic!("start {}: {e}", subject.exe.display()));
        use std::os::windows::io::AsRawHandle;
        let h = HANDLE(child.as_raw_handle());
        // SAFETY: both handles are valid. The child creates its pseudoconsole and shell only after
        // its window is up, long after this, and those inherit the job.
        unsafe { AssignProcessToJobObject(job, h) }.expect("assign to the job");
        std::mem::forget(child);
        let app_name = subject.exe.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        Running { job, app_name, seen: Vec::new() }
    }

    /// The process ids in the job.
    fn job_pids(&self) -> Vec<u32> {
        // The list header is two u32 counts (one usize), followed by the ids.
        let mut list = vec![0usize; 1 + 256];
        // SAFETY: the buffer is large enough for the header and 256 ids, and its size is passed.
        let ok = unsafe {
            QueryInformationJobObject(
                Some(self.job),
                JobObjectBasicProcessIdList,
                list.as_mut_ptr().cast(),
                (list.len() * size_of::<usize>()) as u32,
                None,
            )
        };
        if ok.is_err() {
            return Vec::new();
        }
        // SAFETY: the buffer starts with the list header the call filled in.
        let n = unsafe { &*list.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() }.NumberOfProcessIdsInList as usize;
        list[1..1 + n.min(256)].iter().map(|&p| p as u32).collect()
    }

    /// Finds new processes in the job and refreshes every reading.
    fn sample(&mut self) {
        for pid in self.job_pids() {
            if self.seen.iter().any(|(_, s)| s.pid == pid) {
                continue;
            }
            // SAFETY: opening a process by id for reading; failure is skipped.
            if let Ok(h) = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) } {
                let name = image_name(h);
                let class = classify(&name, &self.app_name);
                let s = Seen { pid, class, cpu: 0.0, private: 0, working: 0, peak_private: 0, peak_working: 0 };
                self.seen.push((h, s));
            }
        }
        for (h, s) in &mut self.seen {
            read(*h, s);
        }
    }

    /// Totals over the processes of `class`.
    fn totals(&self, class: Class) -> Totals {
        let mut t = Totals::default();
        for (_, s) in self.seen.iter().filter(|(_, s)| s.class == class) {
            t.cpu_s += s.cpu;
            t.private_mb += s.private as f64 / 1e6;
            t.working_mb += s.working as f64 / 1e6;
            t.peak_private_mb += s.peak_private as f64 / 1e6;
            t.peak_working_mb += s.peak_working as f64 / 1e6;
        }
        t
    }

    /// The private working set in MB summed over the processes of `class`.
    fn private_working_mb(&self, class: Class) -> f64 {
        self.seen.iter().filter(|(_, s)| s.class == class).map(|(h, _)| private_working_set(*h) as f64 / 1e6).sum()
    }

    /// CPU seconds of the application process by thread name, as one string.
    fn threads(&self) -> String {
        let Some((_, s)) = self.seen.iter().find(|(_, s)| s.class == Class::App) else { return String::new() };
        thread_cpu(s.pid).iter().map(|(n, c)| format!("{n} {c:.2}")).collect::<Vec<_>>().join("; ")
    }

    /// Ends every process in the job and closes the handles.
    fn stop(self) {
        // SAFETY: the job and process handles are owned here and closed once.
        unsafe {
            let _ = TerminateJobObject(self.job, 0);
            for (h, _) in &self.seen {
                let _ = CloseHandle(*h);
            }
            let _ = CloseHandle(self.job);
        }
    }
}

/// Samples the running terminal every 100 ms for `d`.
fn watch(r: &mut Running, d: Duration) {
    let end = Instant::now() + d;
    while Instant::now() < end {
        r.sample();
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Samples the running terminal every 50 ms until `path` exists or `timeout` passes; false on timeout.
fn wait_for(r: &mut Running, path: &Path, timeout: Duration) -> bool {
    let end = Instant::now() + timeout;
    while !path.exists() {
        if Instant::now() > end {
            return false;
        }
        r.sample();
        std::thread::sleep(Duration::from_millis(50));
    }
    r.sample();
    true
}

/// The `idle` workload: start, settle for `settle`, then measure CPU over `idle`.
pub fn idle(subject: &Subject, settle: Duration, idle: Duration) -> serde_json::Value {
    let mut r = Running::start(subject, "cmd.exe /d /k");
    watch(&mut r, settle);
    let start = r.totals(Class::App);
    watch(&mut r, idle);
    let (app, host) = (r.totals(Class::App), r.totals(Class::Host));
    let row = serde_json::json!({
        "workload": "idle", "terminal": subject.name, "settle_s": settle.as_secs_f64(), "idle_s": idle.as_secs_f64(),
        "startup_cpu_s": start.cpu_s, "idle_cpu_s": app.cpu_s - start.cpu_s,
        "private_mb": app.private_mb, "working_mb": app.working_mb, "private_working_mb": r.private_working_mb(Class::App),
        "peak_private_mb": app.peak_private_mb, "peak_working_mb": app.peak_working_mb,
        "host_private_mb": host.private_mb, "host_working_mb": host.working_mb,
        "threads": r.threads(),
    });
    r.stop();
    row
}

/// Writes the batch file the `stream` workload runs: wait `delay_s`, mark the start, type the file,
/// mark the end.
fn stream_script(dir: &Path, file: &Path, delay_s: u32) -> (PathBuf, PathBuf, PathBuf) {
    let (script, started, done) =
        (dir.join("stream.cmd"), dir.join("stream-started.marker"), dir.join("stream-done.marker"));
    let _ = std::fs::remove_file(&started);
    let _ = std::fs::remove_file(&done);
    // `ping` waits without reading the console; the paths have no spaces.
    let body = format!(
        "@ping -n {} 127.0.0.1 > nul\r\n@type nul > \"{}\"\r\n@type \"{}\"\r\n@type nul > \"{}\"\r\n",
        delay_s + 1,
        started.display(),
        file.display(),
        done.display()
    );
    std::fs::write(&script, body).expect("write the stream script");
    (script, started, done)
}

/// The `stream` workload: see the module comment. Measured from the start marker to the done
/// marker, then for `settle` more.
pub fn stream(subject: &Subject, file: &Path, settle: Duration, timeout: Duration) -> serde_json::Value {
    let dir = file.parent().expect("the stream file has a folder");
    let (script, started, done) = stream_script(dir, file, 6);
    let mut r = Running::start(subject, &format!("cmd.exe /d /k {}", script.display()));
    let began = wait_for(&mut r, &started, Duration::from_secs(60));
    let (app0, host0, t0) = (r.totals(Class::App), r.totals(Class::Host), Instant::now());
    let finished = began && wait_for(&mut r, &done, timeout);
    let wall = t0.elapsed().as_secs_f64();
    let (app1, host1, shell1) = (r.totals(Class::App), r.totals(Class::Host), r.totals(Class::Shell));
    let threads = r.threads();
    watch(&mut r, settle);
    let app2 = r.totals(Class::App);
    let bytes = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    let per_100mb = |s: f64| s / (bytes as f64 / 1e8);
    let row = serde_json::json!({
        "workload": "stream", "terminal": subject.name, "bytes": bytes, "finished": finished, "wall_s": wall,
        "cpu_s": app1.cpu_s - app0.cpu_s, "cpu_s_per_100mb": per_100mb(app1.cpu_s - app0.cpu_s),
        "host_cpu_s_per_100mb": per_100mb(host1.cpu_s - host0.cpu_s), "shell_cpu_s": shell1.cpu_s,
        "peak_private_mb": app2.peak_private_mb, "peak_working_mb": app2.peak_working_mb,
        "settled_private_mb": app2.private_mb, "settled_working_mb": app2.working_mb,
        "settled_private_working_mb": r.private_working_mb(Class::App),
        "threads": threads,
    });
    r.stop();
    row
}

/// CPU seconds of each thread of process `pid`, by thread name (unnamed threads are summed under
/// `(unnamed)`), largest first.
pub fn thread_cpu(pid: u32) -> Vec<(String, f64)> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    let mut by_name: Vec<(String, f64)> = Vec::new();
    // SAFETY: a snapshot of every thread; closed below.
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }) else { return by_name };
    let mut e = THREADENTRY32 { dwSize: size_of::<THREADENTRY32>() as u32, ..Default::default() };
    // SAFETY: the entry carries its own size; the snapshot handle is valid.
    let mut more = unsafe { Thread32First(snap, &mut e) }.is_ok();
    while more {
        if e.th32OwnerProcessID == pid {
            let (name, cpu) = one_thread(e.th32ThreadID);
            match by_name.iter_mut().find(|(n, _)| *n == name) {
                Some((_, t)) => *t += cpu,
                None => by_name.push((name, cpu)),
            }
        }
        // SAFETY: as for Thread32First.
        more = unsafe { Thread32Next(snap, &mut e) }.is_ok();
    }
    // SAFETY: closing the snapshot opened above.
    let _ = unsafe { CloseHandle(snap) };
    by_name.sort_by(|a, b| b.1.total_cmp(&a.1));
    by_name
}

/// The name and CPU seconds of thread `tid`.
fn one_thread(tid: u32) -> (String, f64) {
    use windows::Win32::System::Threading::{
        GetThreadDescription, GetThreadTimes, OpenThread, THREAD_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: opening a thread by id for querying; failure gives an empty reading.
    let Ok(h) = (unsafe { OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, tid) }) else {
        return ("(unnamed)".into(), 0.0);
    };
    let (mut c, mut x, mut k, mut u) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: four valid out pointers.
    let cpu =
        if unsafe { GetThreadTimes(h, &mut c, &mut x, &mut k, &mut u) }.is_ok() { secs(k) + secs(u) } else { 0.0 };
    // SAFETY: the description is a valid wide string when the call succeeds; it is copied at once.
    let name = unsafe { GetThreadDescription(h) }.ok().and_then(|p| unsafe { p.to_string() }.ok()).unwrap_or_default();
    // SAFETY: closing the handle opened above.
    let _ = unsafe { CloseHandle(h) };
    (if name.is_empty() { "(unnamed)".into() } else { name }, cpu)
}

/// The two terminals as configured for the comparison: the same window size (120 by 34), font
/// (Cascadia Code at 13 points) and shell. Each reads only the configuration written here.
fn subjects(dir: &Path, termoil: PathBuf, ghostty: PathBuf) -> Vec<Subject> {
    let tcfg = dir.join("termoil-config.toml");
    // TERMOIL_BENCH_RENDERER (auto, gpu, cpu) measures a renderer other than the configured default.
    let renderer = std::env::var("TERMOIL_BENCH_RENDERER").map(|r| format!("renderer = \"{r}\"\n")).unwrap_or_default();
    // TERMOIL_BENCH_SCROLLBACK_BYTES sets the history budget, to compare at Ghostty's 10 MB.
    let scrollback = std::env::var("TERMOIL_BENCH_SCROLLBACK_BYTES")
        .map(|b| format!("\n[scrollback]\nlimit_bytes = {b}\n"))
        .unwrap_or_default();
    let body = format!(
        "# The defaults are Cascadia Code at 13 points.\n[window]\ncolumns = 120\nrows = 34\n{renderer}{scrollback}"
    );
    let _ = std::fs::write(&tcfg, body);
    let gcfg = dir.join("ghostty-config");
    let gbody = "font-family = Cascadia Code\nfont-size = 13\nwindow-width = 120\nwindow-height = 34\nconfirm-close-surface = false\n";
    let _ = std::fs::write(&gcfg, gbody);
    let data = dir.join("termoil-data");
    let _ = std::fs::create_dir_all(&data);
    vec![
        Subject {
            name: "termoil".into(),
            exe: termoil,
            prefix: vec!["--separate".into()],
            command_flag: "-e".into(),
            env: vec![
                ("TERMOIL_CONFIG".into(), tcfg.display().to_string()),
                ("TERMOIL_DATA_DIR".into(), data.display().to_string()),
            ],
        },
        Subject {
            name: "ghostty".into(),
            exe: ghostty,
            prefix: vec!["--config-default-files=false".into(), format!("--config-file={}", gcfg.display())],
            command_flag: "--command=".into(),
            env: vec![],
        },
    ]
}

/// The median of a column over rows, or 0.
fn median(rows: &[&serde_json::Value], key: &str) -> f64 {
    let mut v: Vec<f64> = rows.iter().filter_map(|r| r[key].as_f64()).collect();
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

/// Prints one comparison line: each terminal's median and Termoil as a share of Ghostty; returns
/// the share in percent.
fn line(rows: &[serde_json::Value], workload: &str, key: &str) -> f64 {
    let pick = |t: &str| rows.iter().filter(|r| r["workload"] == workload && r["terminal"] == t).collect::<Vec<_>>();
    let (t, g) = (median(&pick("termoil"), key), median(&pick("ghostty"), key));
    let share = if g > 0.0 { t / g * 100.0 } else { f64::NAN };
    let goal = if share <= 25.0 { "at or under 25 %" } else { "" };
    println!("{workload:<7} {key:<28} termoil {t:>9.3}  ghostty {g:>9.3}  termoil/ghostty {share:>6.1} %  {goal}");
    share
}

/// Writes `mib` MiB of the `mixed` stream to `file` unless it is already there.
fn ensure_stream_file(file: &Path, mib: usize) {
    if std::fs::metadata(file).map(|m| m.len()).unwrap_or(0) == (mib as u64) << 20 {
        return;
    }
    let block = crate::streams::mixed(1 << 20);
    let mut data = Vec::with_capacity(mib << 20);
    while data.len() < mib << 20 {
        data.extend_from_slice(&block);
    }
    data.truncate(mib << 20);
    std::fs::write(file, &data).expect("write the stream file");
}

/// The comparison lines: the metrics the goal is stated on first.
const LINES: [(&str, &str); 11] = [
    ("idle", "private_mb"),
    ("idle", "private_working_mb"),
    ("idle", "working_mb"),
    ("idle", "idle_cpu_s"),
    ("idle", "startup_cpu_s"),
    ("stream", "cpu_s_per_100mb"),
    ("stream", "host_cpu_s_per_100mb"),
    ("stream", "peak_private_mb"),
    ("stream", "settled_private_mb"),
    ("stream", "settled_private_working_mb"),
    ("stream", "wall_s"),
];

/// `apps [runs] [stream MiB]`: the idle and stream workloads on Termoil and Ghostty, alternating,
/// `runs` times each. Termoil is the `termoil-gui.exe` beside this program unless `TERMOIL_BENCH_GUI`
/// names another; Ghostty is `TERMOIL_BENCH_GHOSTTY`. `TERMOIL_BENCH_APPS_ONLY` runs one terminal.
/// Returns the rows.
pub fn command(args: &[String], dir: &Path) -> Vec<serde_json::Value> {
    let runs: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let mib: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(16);
    let own = std::env::current_exe().expect("own path");
    let termoil = std::env::var_os("TERMOIL_BENCH_GUI")
        .map(PathBuf::from)
        .unwrap_or_else(|| own.with_file_name("termoil-gui.exe"));
    let ghostty = std::env::var_os("TERMOIL_BENCH_GHOSTTY").map(PathBuf::from).unwrap_or_default();
    let work = dir.join("apps");
    let _ = std::fs::create_dir_all(&work);
    let file = work.join(format!("mixed-{mib}mib.txt"));
    ensure_stream_file(&file, mib);
    let only = std::env::var("TERMOIL_BENCH_APPS_ONLY").ok();
    let mut rows = Vec::new();
    for run in 0..runs {
        for s in subjects(&work, termoil.clone(), ghostty.clone()) {
            if only.as_deref().is_some_and(|o| o != s.name) || s.exe.as_os_str().is_empty() {
                continue;
            }
            for row in [
                idle(&s, Duration::from_secs(6), Duration::from_secs(10)),
                stream(&s, &file, Duration::from_secs(3), Duration::from_secs(600)),
            ] {
                println!("run {run} {row}");
                rows.push(row);
            }
        }
    }
    for (w, k) in LINES {
        line(&rows, w, k);
    }
    rows
}
