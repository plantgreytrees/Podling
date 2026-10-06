//! Model workers ("sidecars"): separate processes that hold a GPU model,
//! started from a user-level profile and ended when Podling is done.
//!
//! Ending the process is the only reliable way to give the GPU memory back,
//! so a [`Sidecar`] owns its child and stops it in `Drop`.
//!
//! Security: the program and its arguments come only from the user's own
//! `sidecars.toml`, never from an episode file, which is meant to be
//! shareable. The program runs from an argument vector, never through a
//! shell, and the worker binds to `127.0.0.1` only.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::error::{CoreError, ProviderFailure, Result};

/// The sidecar protocol version Podling speaks.
pub const PROTOCOL: u32 = 1;

/// How long a worker may take to print its listening line. Loading the
/// model happens later, on the first request.
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a worker gets to exit after SIGTERM before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// How much of the worker's log an error quotes.
const LOG_TAIL_BYTES: u64 = 2048;

/// One `[sidecars.<name>]` entry: what to run.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarProfile {
    /// The executable: an absolute path, or a name looked up on `PATH`.
    pub program: PathBuf,
    /// Its arguments. Podling appends `--port 0 --run-dir <dir>`.
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfilesFile {
    #[serde(default)]
    sidecars: BTreeMap<String, SidecarProfile>,
}

/// Where profiles live: `$XDG_CONFIG_HOME/podling/sidecars.toml`, else
/// `$HOME/.config/podling/sidecars.toml`. `env` is the process environment
/// in production and a closure in tests.
pub fn default_profiles_path(env: impl Fn(&str) -> Option<String>) -> Result<PathBuf> {
    let non_empty = |name| env(name).filter(|v| !v.is_empty());
    if let Some(config) = non_empty("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config).join("podling/sidecars.toml"));
    }
    if let Some(home) = non_empty("HOME") {
        return Ok(PathBuf::from(home).join(".config/podling/sidecars.toml"));
    }
    Err(config_error(
        "cannot find sidecars.toml: neither XDG_CONFIG_HOME nor HOME is set".into(),
    ))
}

/// Reads profile `name` from the profiles file at `path`. A missing file, a
/// missing profile or a malformed entry is a `Config` error naming the file.
pub fn load_profile(path: &Path, name: &str) -> Result<SidecarProfile> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        config_error(format!(
            "sidecar profile {name:?} needs {}, which cannot be read ({err}); \
             see sidecars/tts/README.md for an example",
            path.display()
        ))
    })?;
    let file: ProfilesFile = toml::from_str(&text)
        .map_err(|err| config_error(format!("{} is not valid: {err}", path.display())))?;
    let profile = file.sidecars.get(name).cloned().ok_or_else(|| {
        config_error(format!(
            "no [sidecars.{name}] in {}; profiles there: {}",
            path.display(),
            if file.sidecars.is_empty() {
                "none".to_owned()
            } else {
                file.sidecars.keys().cloned().collect::<Vec<_>>().join(", ")
            }
        ))
    })?;
    if profile.program.as_os_str().is_empty() {
        return Err(config_error(format!(
            "[sidecars.{name}] in {} has an empty program",
            path.display()
        )));
    }
    Ok(profile)
}

/// The one line a worker prints on stdout once it is listening.
#[derive(Deserialize)]
struct Ready {
    listening: String,
    protocol: u32,
}

/// A running worker. Dropping it stops the process and waits for it.
#[derive(Debug)]
pub struct Sidecar {
    name: String,
    profiles: PathBuf,
    child: Child,
    port: u16,
    log: PathBuf,
    started: Instant,
    /// The worker's descendants once it was ready, so `stop` can still find
    /// them if the worker itself dies first.
    tree: Descendants,
}

impl Sidecar {
    /// Starts profile `name` as `program args… --port 0 --run-dir <run_dir>`
    /// and waits up to `timeout` for its listening line. The worker's stderr
    /// goes to `<run_dir>/sidecar.log`, quoted in errors. `profiles` is the
    /// file the profile came from, for messages.
    pub fn spawn(
        name: &str,
        profile: &SidecarProfile,
        profiles: &Path,
        run_dir: &Path,
        timeout: Duration,
    ) -> Result<Self> {
        let log = run_dir.join("sidecar.log");
        let stderr = File::create(&log).map_err(|err| CoreError::io(&log, err))?;
        let started = Instant::now();
        // `Command` runs the program directly with these arguments: no shell
        // ever parses them, so no quoting or injection is possible.
        let mut child = Command::new(&profile.program)
            .args(&profile.args)
            .arg("--port")
            .arg("0")
            .arg("--run-dir")
            .arg(run_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()
            .map_err(|err| {
                provider_error(format!(
                    "could not start sidecar {name:?} ({}): {err}; check [sidecars.{name}] in {}",
                    profile.program.display(),
                    profiles.display()
                ))
            })?;
        let stdout = child.stdout.take().expect("stdout is piped");
        // From here on, an early return drops `sidecar`, which stops the child.
        let mut sidecar = Self {
            name: name.to_owned(),
            profiles: profiles.to_owned(),
            child,
            port: 0,
            log,
            started,
            tree: Descendants::default(),
        };
        tracing::info!(sidecar = name, pid = sidecar.pid(), "sidecar spawned");

        // Read stdout on a thread so the wait can time out. After the first
        // line the thread keeps draining stdout, so a chatty worker never
        // blocks on a full pipe; it ends when the worker exits.
        let (lines, first) = mpsc::channel();
        let worker = name.to_owned();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            let ready = reader.read_line(&mut line).ok().filter(|&n| n > 0);
            let _ = lines.send(ready.map(|_| line));
            for extra in reader.lines().map_while(std::result::Result::ok) {
                tracing::debug!(sidecar = %worker, line = %extra, "sidecar stdout");
            }
        });

        let line = match first.recv_timeout(timeout) {
            Ok(Some(line)) => line,
            Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                // stdout closes a moment before the exit is reported.
                sidecar.exits_within(Duration::from_millis(500));
                return Err(sidecar.failure("exited before it was ready"));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(sidecar.failure(&format!(
                    "did not report ready within {} s",
                    timeout.as_secs()
                )));
            }
        };
        sidecar.port = parse_ready(&line).map_err(|reason| sidecar.failure(&reason))?;
        // By now a wrapper such as `uv run` has started the real worker.
        sidecar.tree = Descendants::of(sidecar.pid());
        tracing::info!(
            sidecar = name,
            pid = sidecar.pid(),
            children = sidecar.tree.len(),
            port = sidecar.port,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "sidecar ready"
        );
        Ok(sidecar)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// `http://127.0.0.1:<port>`: always loopback, whatever the worker says.
    pub fn base_url(&self) -> String {
        format!(
            "http://{}",
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.port)
        )
    }

    /// Whether the worker exits within `wait`. A dropped connection is how a
    /// dying worker first shows up, a moment before the OS reports its exit.
    pub fn exits_within(&mut self, wait: Duration) -> bool {
        let deadline = Instant::now() + wait;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => return false,
            }
        }
    }

    /// A `Provider` error for something that went wrong with the worker:
    /// `what` happened, plus its exit status if it has exited, the end of
    /// its log, and a hint.
    pub fn failure(&mut self, what: &str) -> CoreError {
        let status = match self.child.try_wait() {
            Ok(Some(status)) => format!(" ({})", describe(status)),
            _ => String::new(),
        };
        let tail = log_tail(&self.log);
        let log = if tail.is_empty() {
            String::new()
        } else {
            format!("\n--- end of the sidecar log ---\n{tail}")
        };
        provider_error(format!(
            "sidecar {:?} {what}{status}. {}{log}",
            self.name,
            self.hint(&tail)
        ))
    }

    /// Advice for a failure, from what the worker logged.
    pub fn hint(&self, log: &str) -> String {
        let lower = log.to_lowercase();
        if ["out of memory", "outofmemory", "gpu memory", "vram"]
            .iter()
            .any(|needle| lower.contains(needle))
        {
            format!(
                "The GPU looks busy: unload other models first (`ollama ps` lists them, \
                 `ollama stop <model>` unloads one), then retry. The worker comes from \
                 [sidecars.{}] in {}.",
                self.name,
                self.profiles.display()
            )
        } else {
            format!(
                "Check [sidecars.{}] in {}.",
                self.name,
                self.profiles.display()
            )
        }
    }

    /// Asks the worker and every process it started to exit (SIGTERM), waits
    /// up to [`STOP_GRACE`] for all of them, then kills what is left, and
    /// reaps the worker so no zombie is left.
    ///
    /// The whole tree matters: behind a wrapper such as `uv run`, the process
    /// holding the GPU is the worker's child, and would outlive a wrapper
    /// that is killed. The tree stays in Podling's process group, so Ctrl-C
    /// in the terminal still reaches all of it.
    ///
    /// The worker may have died first (a crashed `uv run`), leaving the
    /// process that holds the GPU adopted by init. Those are still reached
    /// through the pidfds pinned when the worker reported ready.
    fn stop(&mut self) {
        let pid = self.pid();
        // `mem::take` moves the field out and leaves its `Default` behind:
        // `stop` needs the tree by value while `self` stays borrowed.
        let mut tree = std::mem::take(&mut self.tree);
        let mut status = match self.child.try_wait() {
            Ok(Some(status)) => Some(status),
            _ => None,
        };
        if status.is_none() {
            // Children started since ready can still be found by parent pid.
            tree.extend(Descendants::of(pid));
            terminate(&self.child);
        } else if let Some(exited) = status.filter(|_| tree.running() == 0) {
            self.log_exit(pid, exited);
            return;
        } else {
            tracing::warn!(
                sidecar = %self.name,
                pid,
                processes = tree.running(),
                "sidecar exited but processes it started still run; stopping them"
            );
        }
        tree.terminate();
        let deadline = Instant::now() + STOP_GRACE;
        while Instant::now() < deadline {
            if status.is_none() {
                match self.child.try_wait() {
                    Ok(exited) => status = exited,
                    Err(_) => break,
                }
            }
            if status.is_some() && tree.running() == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let status = status.or_else(|| {
            tracing::warn!(sidecar = %self.name, pid, "sidecar ignored SIGTERM; killing it");
            // Errors here mean the process is already gone.
            let _ = self.child.kill();
            self.child
                .wait()
                .inspect_err(|err| {
                    tracing::warn!(sidecar = %self.name, pid, %err, "could not reap sidecar");
                })
                .ok()
        });
        let left = tree.running();
        if left > 0 {
            tracing::warn!(
                sidecar = %self.name,
                pid,
                processes = left,
                "sidecar's child processes ignored SIGTERM; killing them"
            );
            tree.kill();
        }
        if let Some(status) = status {
            self.log_exit(pid, status);
        }
    }

    fn log_exit(&self, pid: u32, status: ExitStatus) {
        tracing::info!(
            sidecar = %self.name,
            pid,
            elapsed_ms = self.started.elapsed().as_millis() as u64,
            status = %describe(status),
            "sidecar exited"
        );
    }
}

/// `Drop` runs when the owner goes out of scope, on every path: normal
/// return, `?` early return, or a panic unwinding. That makes it the place
/// to give back a resource (here a process holding GPU memory) that must
/// never leak.
impl Drop for Sidecar {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(unix)]
fn terminate(child: &Child) {
    use rustix::process::{Pid, Signal, kill_process};
    // Safe against pid reuse: a child we have not yet reaped keeps its pid,
    // so this can only reach our own worker. An error from `kill` means it
    // has exited already, which `stop` then sees.
    let _ = kill_process(Pid::from_child(child), Signal::TERM);
}

#[cfg(not(unix))]
fn terminate(_: &Child) {
    // No SIGTERM here: `stop` falls through to `kill` after the grace period.
}

/// The processes a worker started, and theirs, each pinned by a pidfd: a
/// signal sent through it reaches that process or none, even if its pid has
/// been reused since, and it still names that process after init adopts it.
///
/// Known limits: a process is found only by a scan while its parent is
/// still the worker's (at ready, and at stop if the worker still runs). So
/// one started after ready by a worker that then dies, or one that
/// double-forks away before the ready scan, is missed; the worker's own
/// parent watch (`sidecars/tts/podling_tts/server.py`, `watch_parent`) is
/// the fallback for those.
#[cfg(target_os = "linux")]
#[derive(Debug, Default)]
struct Descendants(Vec<(u32, rustix::fd::OwnedFd)>);

#[cfg(target_os = "linux")]
impl Descendants {
    fn len(&self) -> usize {
        self.0.len()
    }

    /// Adds `more`, skipping a process already pinned. A pid is the same
    /// process only while the pinned one still runs (a running process keeps
    /// its pid), so exited entries are dropped first.
    fn extend(&mut self, more: Self) {
        self.0.retain(|(_, fd)| is_running(fd));
        for (pid, fd) in more.0 {
            if !self.0.iter().any(|&(pinned, _)| pinned == pid) {
                self.0.push((pid, fd));
            }
        }
    }

    /// Every live descendant of `root`, from one scan of `/proc`.
    fn of(root: u32) -> Self {
        use rustix::process::{Pid, PidfdFlags, pidfd_open};
        let parents: Vec<(u32, u32)> = std::fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
            .filter_map(|pid| Some((pid, proc_stat(pid)?.1)))
            .collect();
        let mut found = Vec::new();
        let mut queue = vec![root];
        while let Some(parent) = queue.pop() {
            for &(pid, _) in parents.iter().filter(|&&(_, ppid)| ppid == parent) {
                let Some(raw) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
                    continue;
                };
                let Ok(fd) = pidfd_open(raw, PidfdFlags::empty()) else {
                    continue; // exited since the scan
                };
                // The pidfd pins whatever has this pid now; it is the process
                // from the scan only if its parent is still the same.
                if proc_stat(pid).is_some_and(|(_, ppid)| ppid == parent) {
                    found.push((pid, fd));
                    queue.push(pid);
                }
            }
        }
        Self(found)
    }

    fn terminate(&self) {
        self.signal(rustix::process::Signal::TERM);
    }

    /// Kills every process still running and waits briefly for them to go.
    fn kill(&self) {
        self.signal(rustix::process::Signal::KILL);
        let deadline = Instant::now() + Duration::from_secs(1);
        while self.running() > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn signal(&self, signal: rustix::process::Signal) {
        for (_, fd) in &self.0 {
            // An error means that process has exited already.
            let _ = rustix::process::pidfd_send_signal(fd, signal);
        }
    }

    /// How many are still running.
    fn running(&self) -> usize {
        self.0.iter().filter(|(_, fd)| is_running(fd)).count()
    }
}

/// Whether the process behind a pidfd still runs. Its pidfd becomes
/// readable once it exits (a zombie has exited), and asking through the fd
/// cannot mistake another process that reuses the pid for it.
#[cfg(target_os = "linux")]
fn is_running(fd: &rustix::fd::OwnedFd) -> bool {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let mut fds = [PollFd::new(fd, PollFlags::IN)];
    let now = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // An error says nothing about the process; count it as gone, so `stop`
    // does not wait on it (it is signalled through the pidfd either way).
    poll(&mut fds, Some(&now)).is_ok_and(|_| !fds[0].revents().contains(PollFlags::IN))
}

/// The state letter and parent pid in `/proc/<pid>/stat`. The command name
/// before them is in parentheses and may itself contain spaces or `)`, so
/// the fields are read after its last `)`.
#[cfg(target_os = "linux")]
fn proc_stat(pid: u32) -> Option<(char, u32)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut fields = stat.rsplit_once(')')?.1.split_whitespace();
    let state = fields.next()?.chars().next()?;
    let ppid = fields.next()?.parse().ok()?;
    Some((state, ppid))
}

/// Elsewhere only the worker itself is stopped.
#[cfg(not(target_os = "linux"))]
#[derive(Debug, Default)]
struct Descendants;

#[cfg(not(target_os = "linux"))]
impl Descendants {
    fn of(_: u32) -> Self {
        Self
    }

    fn len(&self) -> usize {
        0
    }

    fn extend(&mut self, _: Self) {}

    fn terminate(&self) {}

    fn kill(&self) {}

    fn running(&self) -> usize {
        0
    }
}

/// The port from a listening line, which must name `127.0.0.1` and our
/// protocol version.
fn parse_ready(line: &str) -> std::result::Result<u16, String> {
    let ready: Ready = serde_json::from_str(line.trim())
        .map_err(|_| format!("printed {:?} instead of its listening line", line.trim()))?;
    if ready.protocol != PROTOCOL {
        return Err(format!(
            "speaks sidecar protocol {}, but this Podling speaks {PROTOCOL}",
            ready.protocol
        ));
    }
    match ready.listening.parse::<SocketAddrV4>() {
        Ok(addr) if *addr.ip() == Ipv4Addr::LOCALHOST && addr.port() != 0 => Ok(addr.port()),
        _ => Err(format!(
            "listens on {:?}, not on a 127.0.0.1 port",
            ready.listening
        )),
    }
}

fn describe(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => format!("{status}"),
    }
}

/// The last [`LOG_TAIL_BYTES`] of the log, trimmed to whole lines.
fn log_tail(path: &Path) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(LOG_TAIL_BYTES);
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let text = if start > 0 {
        text.split_once('\n').map_or(&*text, |(_, rest)| rest)
    } else {
        &text
    };
    text.trim_end().to_owned()
}

fn config_error(message: String) -> CoreError {
    CoreError::Config { message }
}

fn provider_error(message: String) -> CoreError {
    CoreError::Provider {
        plugin: "sidecar".into(),
        kind: ProviderFailure::Other,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_profiles(text: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sidecars.toml");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    fn config_message(result: Result<SidecarProfile>) -> String {
        match result {
            Err(CoreError::Config { message }) => message,
            other => panic!("expected a Config error, got {other:?}"),
        }
    }

    #[test]
    fn loads_a_profile_by_name() {
        let (_dir, path) = write_profiles(
            r#"
            [sidecars.qwen]
            program = "/usr/bin/uv"
            args = ["run", "podling-tts"]
            "#,
        );
        let profile = load_profile(&path, "qwen").unwrap();
        assert_eq!(profile.program, PathBuf::from("/usr/bin/uv"));
        assert_eq!(profile.args, ["run", "podling-tts"]);
    }

    #[test]
    fn a_missing_profile_or_file_names_the_file() {
        let (dir, path) = write_profiles("[sidecars.qwen]\nprogram = \"/bin/true\"\n");
        let message = config_message(load_profile(&path, "dia2"));
        assert!(message.contains("[sidecars.dia2]"), "{message}");
        assert!(message.contains(&path.display().to_string()), "{message}");
        assert!(message.contains("qwen"), "lists what exists: {message}");

        let missing = dir.path().join("nope.toml");
        let message = config_message(load_profile(&missing, "qwen"));
        assert!(
            message.contains(&missing.display().to_string()),
            "{message}"
        );
    }

    #[test]
    fn malformed_profiles_are_rejected() {
        for text in [
            "[sidecars.qwen]\nprogram = \"x\"\nshell = \"sh -c\"\n",
            "[sidecars.qwen]\nargs = []\n",
            "[sidecars.qwen]\nprogram = \"\"\n",
            "[other]\n",
        ] {
            let (_dir, path) = write_profiles(text);
            config_message(load_profile(&path, "qwen"));
        }
    }

    #[test]
    fn default_path_prefers_xdg() {
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                vars.iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        assert_eq!(
            default_profiles_path(env(&[("XDG_CONFIG_HOME", "/x"), ("HOME", "/h")])).unwrap(),
            PathBuf::from("/x/podling/sidecars.toml")
        );
        assert_eq!(
            default_profiles_path(env(&[("XDG_CONFIG_HOME", ""), ("HOME", "/h")])).unwrap(),
            PathBuf::from("/h/.config/podling/sidecars.toml")
        );
        assert!(default_profiles_path(env(&[])).is_err());
    }

    #[test]
    fn the_listening_line_must_be_loopback_and_our_protocol() {
        assert_eq!(
            parse_ready("{\"listening\": \"127.0.0.1:4321\", \"protocol\": 1}\n"),
            Ok(4321)
        );
        for (line, expected) in [
            (
                "{\"listening\": \"0.0.0.0:4321\", \"protocol\": 1}",
                "not on a 127.0.0.1",
            ),
            (
                "{\"listening\": \"127.0.0.1:0\", \"protocol\": 1}",
                "not on a 127.0.0.1",
            ),
            (
                "{\"listening\": \"127.0.0.1:80\", \"protocol\": 2}",
                "protocol 2",
            ),
            ("Loading model...", "instead of its listening line"),
        ] {
            let err = parse_ready(line).unwrap_err();
            assert!(err.contains(expected), "{line}: {err}");
        }
    }

    #[test]
    fn the_log_tail_keeps_whole_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log");
        let mut text = "x".repeat(3000);
        text.push_str("\nCUDA out of memory\n");
        std::fs::write(&path, text).unwrap();
        assert_eq!(log_tail(&path), "CUDA out of memory");
        assert_eq!(log_tail(&dir.path().join("missing")), "");
    }
}
