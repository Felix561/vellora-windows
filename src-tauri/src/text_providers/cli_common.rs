//! Process control shared by the subscription CLI bridges (M6, personal use).
//!
//! Vellora only starts the user's own installed, unmodified `codex` / `claude` binary that the
//! user signed into. It never reads, stores, proxies or refreshes subscription tokens: the CLI
//! finds its own login through the inherited user environment (`USERPROFILE`, `APPDATA`,
//! `CODEX_HOME`, ...). Status detection only runs the CLIs' own version and status commands.
//!
//! Every run:
//! - starts in a fresh, empty temporary directory (`<temp>/vellora-cli-*/work`) as cwd;
//! - gets the selected text on stdin, never on the command line;
//! - runs inside a Windows Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The process
//!   is created suspended and only resumed after it was assigned, so even its first child is
//!   in the job. Cancel, timeout and the end of the run kill the whole process tree;
//! - has no window (`CREATE_NO_WINDOW`);
//! - reads stdout line by line with a size cap and keeps at most 16 KiB of stderr, which is
//!   only used to classify errors and is never logged or shown (it may echo input);
//! - drops API-key variables (`OPENAI_API_KEY`, `CODEX_API_KEY`, `ANTHROPIC_API_KEY`,
//!   `ANTHROPIC_AUTH_TOKEN`) from the inherited environment, so a run is billed to the plan
//!   login and not silently to an API key. Nothing is injected.

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use super::{CancelToken, ProviderKind, TextError};

/// Total budget of one CLI run (spec section 6.3). Runs also respect a shorter
/// `TextRequest::timeout`.
pub const CLI_TIMEOUT: Duration = Duration::from_secs(120);
/// Budget of one version or status command (a cold `claude auth status` took 8.7 s here).
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(20);

const MAX_STDOUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 2 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 16 * 1024;
/// How long to wait for the killed process tree to exit.
const KILL_WAIT: Duration = Duration::from_secs(2);

/// Which subscription CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CliKind {
    /// OpenAI Codex CLI (ChatGPT plan).
    Codex,
    /// Claude Code (Claude plan).
    Claude,
}

impl CliKind {
    pub fn from_provider(provider: ProviderKind) -> Option<Self> {
        match provider {
            ProviderKind::CodexCli => Some(CliKind::Codex),
            ProviderKind::ClaudeCli => Some(CliKind::Claude),
            _ => None,
        }
    }

    pub fn provider(self) -> ProviderKind {
        match self {
            CliKind::Codex => ProviderKind::CodexCli,
            CliKind::Claude => ProviderKind::ClaudeCli,
        }
    }

    fn program(self) -> &'static str {
        match self {
            CliKind::Codex => "codex",
            CliKind::Claude => "claude",
        }
    }

    /// Inherited variables removed before the CLI starts: API keys (a run must use the plan
    /// login) and markers of an enclosing CLI session.
    pub(crate) fn env_remove(self) -> &'static [&'static str] {
        match self {
            CliKind::Codex => &[
                "OPENAI_API_KEY",
                "CODEX_API_KEY",
                "CODEX_SANDBOX",
                "CODEX_SANDBOX_NETWORK_DISABLED",
            ],
            CliKind::Claude => &[
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "CLAUDECODE",
                "CLAUDE_CODE_ENTRYPOINT",
                "CLAUDE_CODE_SSE_PORT",
            ],
        }
    }

    /// Install locations checked after `PATH` (Vellora may start without the shell's `PATH`).
    fn well_known_paths(self) -> Vec<PathBuf> {
        let env_dir = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let mut paths = Vec::new();
        match self {
            CliKind::Codex => {
                if let Some(local) = env_dir("LOCALAPPDATA") {
                    paths.push(local.join(r"Programs\OpenAI\Codex\bin\codex.exe"));
                }
                if let Some(roaming) = env_dir("APPDATA") {
                    paths.push(roaming.join(r"npm\codex.cmd"));
                }
            }
            CliKind::Claude => {
                if let Some(home) = env_dir("USERPROFILE") {
                    paths.push(home.join(r".local\bin\claude.exe"));
                }
                if let Some(roaming) = env_dir("APPDATA") {
                    paths.push(roaming.join(r"npm\claude.cmd"));
                }
            }
        }
        paths
    }

    /// The native binary behind an npm `.cmd` shim in `shim_dir`, if present. Running it
    /// directly avoids `cmd.exe` argument quoting. The Claude shim only forwards to
    /// `claude.exe`; the Codex shim starts `node codex.js`, which only adds package-manager
    /// hints before starting the vendored `codex.exe`.
    fn npm_native(self, shim_dir: &Path) -> Option<PathBuf> {
        let modules = shim_dir.join("node_modules");
        let candidates: Vec<PathBuf> = match self {
            CliKind::Claude => vec![modules.join(r"@anthropic-ai\claude-code\bin\claude.exe")],
            CliKind::Codex => {
                let codex = modules.join(r"@openai\codex\node_modules\@openai");
                vec![
                    codex.join(r"codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe"),
                    codex.join(r"codex-win32-arm64\vendor\aarch64-pc-windows-msvc\bin\codex.exe"),
                ]
            }
        };
        candidates.into_iter().find(|path| path.is_file())
    }
}

/// What the Models tab shows for a CLI bridge. Detected without a model request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    pub installed: bool,
    /// Version reported by `--version`, e.g. `0.169.0`.
    pub version: Option<String>,
    /// From the CLI's own status command; `None` when it could not be determined.
    pub signed_in: Option<bool>,
    /// `Some(true)` when signed in with the plan login (ChatGPT / claude.ai), `Some(false)`
    /// when the CLI is signed in with an API key (runs would be billed to the API).
    pub plan_login: Option<bool>,
    /// The binary Vellora runs.
    pub path: Option<String>,
}

/// Finds the CLI: the explicit settings path when set (no fallback, so a wrong path shows as
/// not installed), else `PATH` (`.exe`, `.cmd`, `.bat`), else the known install locations.
/// npm shims resolve to the native binary when it sits in the usual place.
pub fn locate(kind: CliKind, explicit: Option<&str>) -> Option<PathBuf> {
    locate_in(
        kind,
        explicit,
        std::env::var_os("PATH"),
        &kind.well_known_paths(),
    )
}

fn locate_in(
    kind: CliKind,
    explicit: Option<&str>,
    path_var: Option<OsString>,
    well_known: &[PathBuf],
) -> Option<PathBuf> {
    if let Some(explicit) = explicit.map(|p| p.trim().trim_matches('"')) {
        if !explicit.is_empty() {
            return runnable(Path::new(explicit)).map(|path| resolve_shim(kind, path));
        }
    }
    let from_path = path_var
        .iter()
        .flat_map(std::env::split_paths)
        .find_map(|dir| {
            ["exe", "cmd", "bat"]
                .iter()
                .map(|ext| dir.join(kind.program()).with_extension(ext))
                .find(|candidate| candidate.is_file())
        });
    from_path
        .or_else(|| well_known.iter().find(|path| path.is_file()).cloned())
        .map(|path| resolve_shim(kind, path))
}

/// `path` itself when it is a runnable file, else a runnable sibling with the same stem (for
/// a setting that points at an extension-less or `.ps1` npm shim).
fn runnable(path: &Path) -> Option<PathBuf> {
    let is_runnable = |p: &Path| p.is_file() && (is_exe(p) || is_batch(p));
    if is_runnable(path) {
        return Some(path.to_path_buf());
    }
    ["exe", "cmd"]
        .iter()
        .map(|ext| path.with_extension(ext))
        .find(|candidate| is_runnable(candidate))
}

fn extension_is(path: &Path, wanted: &[&str]) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| wanted.iter().any(|w| ext.eq_ignore_ascii_case(w)))
}

fn is_exe(path: &Path) -> bool {
    extension_is(path, &["exe", "com"])
}

fn is_batch(path: &Path) -> bool {
    extension_is(path, &["cmd", "bat"])
}

fn resolve_shim(kind: CliKind, path: PathBuf) -> PathBuf {
    if !is_batch(&path) {
        return path;
    }
    path.parent()
        .and_then(|dir| kind.npm_native(dir))
        .unwrap_or(path)
}

/// Installed / version / signed-in status from `--version` and the CLI's own status command
/// (`codex login status`, `claude auth status`; both exit 0 when signed in, 1 when not).
/// Their output can name the account: it is only matched in memory, never stored or logged.
pub async fn cli_status(kind: CliKind, explicit_path: Option<&str>) -> CliStatus {
    cli_status_with(kind, locate(kind, explicit_path), STATUS_TIMEOUT).await
}

pub(crate) async fn cli_status_with(
    kind: CliKind,
    path: Option<PathBuf>,
    timeout: Duration,
) -> CliStatus {
    let Some(path) = path else {
        return CliStatus::default();
    };
    let auth_args: &[&str] = match kind {
        CliKind::Codex => &["login", "status"],
        CliKind::Claude => &["auth", "status"],
    };
    let (version, auth) = tokio::join!(
        probe(kind, &path, &["--version"], timeout),
        probe(kind, &path, auth_args, timeout)
    );
    let mut status = CliStatus {
        installed: true,
        path: Some(path.display().to_string()),
        ..CliStatus::default()
    };
    match version {
        Err(TextError::CliNotInstalled) => {
            status.installed = false;
            return status;
        }
        Ok(output) if output.code == Some(0) => status.version = parse_version(&output.stdout),
        _ => {}
    }
    if let Ok(output) = auth {
        status.signed_in = match output.code {
            Some(0) => Some(true),
            Some(1) => Some(false),
            _ => None,
        };
        if status.signed_in == Some(true) {
            status.plan_login = plan_login(kind, &output);
        }
    }
    status
}

struct ProbeOutput {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

async fn probe(
    kind: CliKind,
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<ProbeOutput, TextError> {
    let dir = RunDir::new()?;
    let mut stdout = String::new();
    let spec = RunSpec {
        program: program.to_path_buf(),
        args: args.iter().map(OsString::from).collect(),
        cwd: dir.work().to_path_buf(),
        stdin: None,
        env_remove: kind.env_remove(),
        timeout,
        pid_probe: None,
    };
    let exit = run(spec, &CancelToken::new(), &mut |line| {
        if stdout.len() < 64 * 1024 {
            stdout.push_str(line);
            stdout.push('\n');
        }
        Ok(())
    })
    .await?;
    Ok(ProbeOutput {
        code: exit.code,
        stdout,
        stderr: exit.stderr,
    })
}

/// `codex-cli 0.169.0` -> `0.169.0`; `2.1.325 (Claude Code)` -> `2.1.325`.
fn parse_version(output: &str) -> Option<String> {
    let line = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let token = line
        .split_whitespace()
        .find(|token| token.starts_with(|c: char| c.is_ascii_digit()))
        .unwrap_or(line);
    Some(token.chars().take(64).collect())
}

/// Whether a signed-in CLI uses the plan login. Only the login method is read.
fn plan_login(kind: CliKind, output: &ProbeOutput) -> Option<bool> {
    match kind {
        // `Logged in using ChatGPT` vs. `Logged in using an API key - ...` (on stderr).
        CliKind::Codex => {
            let text = format!("{}\n{}", output.stdout, output.stderr);
            if text.contains("using ChatGPT") {
                Some(true)
            } else if text.contains("API key") {
                Some(false)
            } else {
                None
            }
        }
        // JSON with `authMethod`: `claude.ai` / `oauth_token` (plan) or `api_key`,
        // `api_key_helper`, `third_party` (billed elsewhere).
        CliKind::Claude => {
            #[derive(Deserialize)]
            struct AuthStatus {
                #[serde(rename = "authMethod")]
                auth_method: Option<String>,
            }
            let parsed: AuthStatus = serde_json::from_str(&output.stdout).ok()?;
            match parsed.auth_method?.as_str() {
                "claude.ai" | "oauth_token" => Some(true),
                "none" => None,
                _ => Some(false),
            }
        }
    }
}

/// Fresh temporary directory for one run: `root/work` is the empty cwd; files the CLI reads
/// (such as the instructions) are written to `root`, outside the cwd. Deleted on drop.
pub(crate) struct RunDir {
    root: tempfile::TempDir,
    work: PathBuf,
}

impl RunDir {
    pub(crate) fn new() -> Result<Self, TextError> {
        let root = tempfile::Builder::new()
            .prefix("vellora-cli-")
            .tempdir()
            .map_err(|_| TextError::BadResponse)?;
        let work = root.path().join("work");
        std::fs::create_dir(&work).map_err(|_| TextError::BadResponse)?;
        Ok(Self { root, work })
    }

    pub(crate) fn work(&self) -> &Path {
        &self.work
    }

    /// Writes `contents` to `root/name` and returns the absolute path.
    pub(crate) fn write_file(&self, name: &str, contents: &str) -> Result<PathBuf, TextError> {
        let path = self.root.path().join(name);
        std::fs::write(&path, contents).map_err(|_| TextError::BadResponse)?;
        Ok(path)
    }
}

/// One CLI process run.
pub(crate) struct RunSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    /// Bytes written to stdin, which is then closed. `None`: stdin is the null device.
    pub stdin: Option<Vec<u8>>,
    pub env_remove: &'static [&'static str],
    pub timeout: Duration,
    /// Receives the process id once the process runs (tests inspect the process tree).
    pub pid_probe: Option<Arc<AtomicU32>>,
}

#[derive(Debug)]
pub(crate) struct RunExit {
    /// Exit code; `None` when the process ended without one.
    pub code: Option<i32>,
    /// At most 16 KiB of stderr, lossily decoded. May echo input: never log or show it.
    pub stderr: String,
}

/// A line handler. Returning an error stops the run and kills the process tree.
pub(crate) type LineHandler<'a> = dyn FnMut(&str) -> Result<(), TextError> + Send + 'a;

enum Stop {
    Cancelled,
    Timeout,
    Done(Result<Option<i32>, TextError>),
}

/// Runs `spec` to completion, passing each stdout line (without the line ending) to
/// `on_line`. Errors: `CliNotInstalled` when the program cannot be started, `Cancelled`,
/// `Timeout`, `BadResponse` (output too large, process control failed) and any error returned
/// by `on_line`. The whole process tree is killed on every error and at the end of the run.
pub(crate) async fn run(
    spec: RunSpec,
    cancel: &CancelToken,
    on_line: &mut LineHandler<'_>,
) -> Result<RunExit, TextError> {
    if cancel.is_cancelled() {
        return Err(TextError::Cancelled);
    }
    let deadline = tokio::time::Instant::now() + spec.timeout;
    let job = job::Job::new().map_err(|_| TextError::BadResponse)?;

    let mut command = tokio::process::Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .creation_flags(job::CREATION_FLAGS);
    for name in spec.env_remove {
        command.env_remove(name);
    }
    let mut child = command.spawn().map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => {
            TextError::CliNotInstalled
        }
        _ => TextError::BadResponse,
    })?;
    let pid = child.id().unwrap_or_default();
    let started = child
        .raw_handle()
        .ok_or(())
        .and_then(|handle| job.assign(handle))
        .and_then(|()| job::resume_process(pid));
    if started.is_err() {
        // Still suspended (or not in the job): kill it before it can run.
        let _ = child.start_kill();
        let _ = tokio::time::timeout(KILL_WAIT, child.wait()).await;
        return Err(TextError::BadResponse);
    }
    if let Some(probe) = &spec.pid_probe {
        probe.store(pid, Ordering::SeqCst);
    }

    let stdin_task = match (child.stdin.take(), spec.stdin) {
        (Some(mut pipe), Some(data)) => Some(tokio::spawn(async move {
            // The CLI may exit before reading everything; a broken pipe is not an error here.
            let _ = pipe.write_all(&data).await;
            let _ = pipe.shutdown().await;
        })),
        _ => None,
    };
    let stderr_task = child
        .stderr
        .take()
        .map(|stderr| tokio::spawn(read_bounded(stderr, MAX_STDERR_BYTES)));
    let mut stdout = LineReader::new(child.stdout.take().ok_or(TextError::BadResponse)?);

    let stop = {
        let work = async {
            let mut exit: Option<Option<i32>> = None;
            loop {
                tokio::select! {
                    line = stdout.next_line() => match line? {
                        Some(line) => on_line(&line)?,
                        None => break,
                    },
                    status = child.wait(), if exit.is_none() => {
                        exit = Some(status.map_err(|_| TextError::BadResponse)?.code());
                        // Leftover descendants may hold stdout open; end them so the rest of
                        // the output can be drained to EOF.
                        job.terminate();
                    }
                }
            }
            match exit {
                Some(code) => Ok(code),
                None => Ok(child
                    .wait()
                    .await
                    .map_err(|_| TextError::BadResponse)?
                    .code()),
            }
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Stop::Cancelled,
            _ = tokio::time::sleep_until(deadline) => Stop::Timeout,
            result = work => Stop::Done(result),
        }
    };

    // Ends every process still in the job (the CLI on cancel/timeout/error, descendants on
    // success). The job handle itself is closed (KILL_ON_JOB_CLOSE) when `job` drops.
    job.terminate();
    let _ = tokio::time::timeout(KILL_WAIT, child.wait()).await;
    if let Some(task) = stdin_task {
        task.abort();
    }
    let stderr = match stderr_task {
        Some(task) => tokio::time::timeout(KILL_WAIT, task)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default(),
        None => Vec::new(),
    };
    match stop {
        Stop::Cancelled => Err(TextError::Cancelled),
        Stop::Timeout => Err(TextError::Timeout),
        Stop::Done(Err(err)) => Err(err),
        Stop::Done(Ok(code)) => Ok(RunExit {
            code,
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }),
    }
}

/// Reads to EOF, keeping the first `limit` bytes.
async fn read_bounded<R: AsyncRead + Unpin>(mut reader: R, limit: usize) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut chunk = [0u8; 4096];
    while let Ok(read) = reader.read(&mut chunk).await {
        if read == 0 {
            break;
        }
        let room = limit.saturating_sub(kept.len());
        kept.extend_from_slice(&chunk[..read.min(room)]);
    }
    kept
}

/// Splits a byte stream into lines with per-line and total size caps. Cancel-safe: a dropped
/// `next_line` future loses no data.
struct LineReader<R> {
    inner: R,
    buffer: Vec<u8>,
    total: usize,
    eof: bool,
}

impl<R: AsyncRead + Unpin> LineReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            buffer: Vec::new(),
            total: 0,
            eof: false,
        }
    }

    async fn next_line(&mut self) -> Result<Option<String>, TextError> {
        loop {
            if let Some(end) = self.buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=end).collect();
                return Ok(Some(decode_line(&line)));
            }
            if self.eof {
                if self.buffer.is_empty() {
                    return Ok(None);
                }
                let line = std::mem::take(&mut self.buffer);
                return Ok(Some(decode_line(&line)));
            }
            if self.buffer.len() > MAX_LINE_BYTES {
                return Err(TextError::BadResponse);
            }
            let mut chunk = [0u8; 8192];
            let read = self
                .inner
                .read(&mut chunk)
                .await
                .map_err(|_| TextError::BadResponse)?;
            if read == 0 {
                self.eof = true;
                continue;
            }
            self.total += read;
            if self.total > MAX_STDOUT_BYTES {
                return Err(TextError::BadResponse);
            }
            self.buffer.extend_from_slice(&chunk[..read]);
        }
    }
}

fn decode_line(line: &[u8]) -> String {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    String::from_utf8_lossy(line).into_owned()
}

/// Classifies CLI failure text (an error message, a result text or stderr). The text itself
/// is never returned, logged or shown: it may echo input.
pub(crate) fn classify_failure(text: &str) -> TextError {
    let lower = text.to_lowercase();
    let any = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));
    if any(&[
        "not logged in",
        "not signed in",
        "please run /login",
        "codex login",
        "claude login",
        "log in again",
        "login required",
        "authentication",
        "unauthorized",
        "401",
        "oauth",
        "token expired",
        "invalid api key",
    ]) {
        TextError::CliNotSignedIn
    } else if any(&[
        "rate limit",
        "usage limit",
        "limit reached",
        "too many requests",
        "429",
        "quota",
    ]) {
        TextError::RateLimited
    } else {
        TextError::BadResponse
    }
}

/// Checks a model id before it is passed as a CLI argument: non-empty, no leading `-` (no
/// option injection) and only `[A-Za-z0-9._:/\[\]-]`.
pub(crate) fn valid_model_arg(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 128
        && !model.starts_with('-')
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:/[]-".contains(c))
}

/// Windows Job Object and process start helpers.
mod job {
    use std::{ffi::c_void, os::windows::io::RawHandle};

    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{CloseHandle, HANDLE},
            System::{
                Diagnostics::ToolHelp::{
                    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
                    THREADENTRY32,
                },
                JobObjects::{
                    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                    SetInformationJobObject, TerminateJobObject,
                    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                },
                Threading::{
                    OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED,
                    THREAD_SUSPEND_RESUME,
                },
            },
        },
    };

    /// No window, and suspended until the process is in the job.
    pub(super) const CREATION_FLAGS: u32 = CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0;

    /// A job that kills its processes when terminated or when the handle closes.
    pub(super) struct Job(HANDLE);

    // The handle is an owned kernel object handle; the job APIs are thread-safe.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub(super) fn new() -> windows::core::Result<Self> {
            // SAFETY: plain Win32 calls; the handle is owned by `Job` and closed on drop.
            unsafe {
                let job = Job(CreateJobObjectW(None, PCWSTR::null())?);
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    std::mem::size_of_val(&info) as u32,
                )?;
                Ok(job)
            }
        }

        pub(super) fn assign(&self, process: RawHandle) -> Result<(), ()> {
            // SAFETY: `process` is the live child's handle, owned by tokio's `Child`.
            unsafe { AssignProcessToJobObject(self.0, HANDLE(process)) }.map_err(|_| ())
        }

        /// Kills every process in the job. Safe to call repeatedly.
        pub(super) fn terminate(&self) {
            // SAFETY: valid job handle.
            unsafe {
                let _ = TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: closes the owned handle once; KILL_ON_JOB_CLOSE ends any survivor.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// Resumes the threads of a process created with `CREATE_SUSPENDED`.
    pub(super) fn resume_process(pid: u32) -> Result<(), ()> {
        if pid == 0 {
            return Err(());
        }
        // SAFETY: snapshot and thread handles are closed before returning.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0).map_err(|_| ())?;
            let mut entry = THREADENTRY32 {
                dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            let mut resumed = 0;
            let mut more = Thread32First(snapshot, &mut entry).is_ok();
            while more {
                if entry.th32OwnerProcessID == pid {
                    if let Ok(thread) = OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID)
                    {
                        if ResumeThread(thread) != u32::MAX {
                            resumed += 1;
                        }
                        let _ = CloseHandle(thread);
                    }
                }
                more = Thread32Next(snapshot, &mut entry).is_ok();
            }
            let _ = CloseHandle(snapshot);
            if resumed > 0 {
                Ok(())
            } else {
                Err(())
            }
        }
    }
}

/// Fake CLI scripts and process-tree helpers for the bridge tests. The fakes are `.cmd`
/// scripts in a temp directory; they never call a real CLI.
#[cfg(test)]
pub(crate) mod fake {
    use std::{
        path::{Path, PathBuf},
        time::Duration,
    };

    use windows::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
        },
    };

    /// A fake CLI: `cli.cmd` plus `canned.txt` (printed to stdout). Every run records its
    /// arguments, stdin, cwd and cwd listing next to the script.
    pub struct FakeCli {
        pub dir: tempfile::TempDir,
    }

    impl FakeCli {
        /// `body` is the script tail after the recording lines; `canned` is the stdout text.
        pub fn new(canned: &str, body: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("canned.txt"), canned).unwrap();
            let script = format!(
                "@echo off\r\n\
                 echo %* > \"%~dp0args.txt\"\r\n\
                 cd > \"%~dp0cwd.txt\"\r\n\
                 dir /b /a > \"%~dp0listing.txt\"\r\n\
                 findstr \"^\" > \"%~dp0stdin.txt\"\r\n\
                 {body}\r\n"
            );
            std::fs::write(dir.path().join("cli.cmd"), script).unwrap();
            Self { dir }
        }

        /// Prints `canned` and exits with `code`.
        pub fn printing(canned: &str, code: i32) -> Self {
            Self::new(
                canned,
                &format!("type \"%~dp0canned.txt\"\r\nexit /b {code}"),
            )
        }

        /// Prints `canned`, starts a grandchild that sleeps, then sleeps itself.
        pub fn hanging(canned: &str) -> Self {
            Self::new(
                canned,
                "type \"%~dp0canned.txt\"\r\n\
                 start \"\" /b cmd /c ping -n 120 127.0.0.1 ^>nul\r\n\
                 ping -n 120 127.0.0.1 >nul",
            )
        }

        pub fn path(&self) -> PathBuf {
            self.dir.path().join("cli.cmd")
        }

        pub fn path_string(&self) -> String {
            self.path().display().to_string()
        }

        pub fn recorded(&self, name: &str) -> String {
            String::from_utf8_lossy(&std::fs::read(self.dir.path().join(name)).unwrap_or_default())
                .into_owned()
        }
    }

    /// All descendants of `pid` (by parent process id) with their executable names.
    pub fn descendants(pid: u32) -> Vec<(u32, String)> {
        let mut entries = Vec::new();
        // SAFETY: snapshot handle closed below.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).unwrap();
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
            while more {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                entries.push((entry.th32ProcessID, entry.th32ParentProcessID, name));
                more = Process32NextW(snapshot, &mut entry).is_ok();
            }
            let _ = CloseHandle(snapshot);
        }
        let mut parents = vec![pid];
        let mut found = Vec::new();
        let mut index = 0;
        while index < parents.len() {
            let parent = parents[index];
            for (child, child_parent, name) in &entries {
                if *child_parent == parent && *child != 0 && !parents.contains(child) {
                    parents.push(*child);
                    found.push((*child, name.clone()));
                }
            }
            index += 1;
        }
        found
    }

    /// Waits until `pid` has exited (or never existed). Returns false if it is still running
    /// after `timeout`.
    pub fn exited_within(pid: u32, timeout: Duration) -> bool {
        // SAFETY: the handle is closed before returning.
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else {
                return true;
            };
            let done = WaitForSingleObject(handle, timeout.as_millis() as u32) == WAIT_OBJECT_0;
            let _ = CloseHandle(handle);
            done
        }
    }

    pub fn listing_is_empty(listing: &str) -> bool {
        listing.trim().is_empty()
    }

    pub fn same_dir(a: &str, b: &Path) -> bool {
        let a = PathBuf::from(a.trim());
        match (a.canonicalize(), b.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::Mutex,
        time::{Duration, Instant},
    };

    use super::{fake::*, *};

    fn spec(fake: &FakeCli, stdin: Option<&str>, timeout: Duration) -> (RunDir, RunSpec) {
        let dir = RunDir::new().unwrap();
        let spec = RunSpec {
            program: fake.path(),
            args: vec!["--flag".into(), "value with space".into()],
            cwd: dir.work().to_path_buf(),
            stdin: stdin.map(|s| s.as_bytes().to_vec()),
            env_remove: CliKind::Claude.env_remove(),
            timeout,
            pid_probe: Some(Arc::default()),
        };
        (dir, spec)
    }

    async fn collect(
        spec: RunSpec,
        cancel: &CancelToken,
    ) -> (Result<RunExit, TextError>, Vec<String>) {
        let lines = Mutex::new(Vec::new());
        let result = run(spec, cancel, &mut |line| {
            lines.lock().unwrap().push(line.to_string());
            Ok(())
        })
        .await;
        (result, lines.into_inner().unwrap())
    }

    #[tokio::test]
    async fn runs_in_an_empty_temp_dir_with_the_prompt_on_stdin_only() {
        let fake = FakeCli::printing("one\r\ntwo ü 👋\r\n", 0);
        let (dir, spec) = spec(&fake, Some("SECRET-SELECTED-TEXT-42"), CLI_TIMEOUT);
        let (result, lines) = collect(spec, &CancelToken::new()).await;
        let exit = result.unwrap();
        assert_eq!(exit.code, Some(0));
        assert_eq!(lines, ["one", "two ü 👋"]);
        assert!(fake
            .recorded("stdin.txt")
            .contains("SECRET-SELECTED-TEXT-42"));
        let args = fake.recorded("args.txt");
        assert!(args.contains("--flag"), "{args}");
        assert!(!args.contains("SECRET"), "{args}");
        assert!(same_dir(&fake.recorded("cwd.txt"), dir.work()));
        assert!(listing_is_empty(&fake.recorded("listing.txt")));
    }

    #[tokio::test]
    async fn reports_the_exit_code_and_keeps_stderr_bounded() {
        let fake = FakeCli::new(
            "",
            "for /l %%i in (1,1,3000) do @echo private stderr line %%i 1>&2\r\nexit /b 3",
        );
        let (_dir, spec) = spec(&fake, None, CLI_TIMEOUT);
        let (result, _) = collect(spec, &CancelToken::new()).await;
        let exit = result.unwrap();
        assert_eq!(exit.code, Some(3));
        assert!(exit.stderr.len() <= MAX_STDERR_BYTES);
        assert!(exit.stderr.contains("private stderr line 1"));
    }

    #[tokio::test]
    async fn missing_binary_is_not_installed() {
        let dir = RunDir::new().unwrap();
        let spec = RunSpec {
            program: dir.work().join("no-such-cli.exe"),
            args: Vec::new(),
            cwd: dir.work().to_path_buf(),
            stdin: None,
            env_remove: &[],
            timeout: CLI_TIMEOUT,
            pid_probe: None,
        };
        let (result, _) = collect(spec, &CancelToken::new()).await;
        assert_eq!(result.unwrap_err(), TextError::CliNotInstalled);
    }

    /// Waits until the fake's process tree includes the sleeping grandchild.
    async fn wait_for_tree(probe: &AtomicU32) -> Vec<u32> {
        let started = Instant::now();
        loop {
            let pid = probe.load(Ordering::SeqCst);
            if pid != 0 {
                let tree = descendants(pid);
                // cmd -> (start) cmd -> ping (a grandchild), and cmd -> ping.
                let pings = tree.iter().filter(|(_, name)| name == "ping.exe").count();
                if pings >= 2 {
                    let mut all: Vec<u32> = tree.into_iter().map(|(pid, _)| pid).collect();
                    all.push(pid);
                    return all;
                }
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "tree never started"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    #[tokio::test]
    async fn timeout_kills_the_whole_process_tree() {
        let fake = FakeCli::hanging("partial\r\n");
        // Long enough for the tree to start even when many tests spawn processes at once.
        let (_dir, spec) = spec(&fake, None, Duration::from_secs(6));
        let probe = spec.pid_probe.clone().unwrap();
        let started = Instant::now();
        let runner = tokio::spawn(async move { collect(spec, &CancelToken::new()).await });
        let tree = wait_for_tree(&probe).await;
        let (result, lines) = runner.await.unwrap();
        assert_eq!(result.unwrap_err(), TextError::Timeout);
        assert_eq!(lines, ["partial"]);
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_millis(5900), "{elapsed:?}");
        assert!(elapsed < Duration::from_secs(15), "{elapsed:?}");
        for pid in tree {
            assert!(
                exited_within(pid, Duration::from_secs(3)),
                "pid {pid} survived"
            );
        }
    }

    #[tokio::test]
    async fn cancel_returns_within_a_second_and_kills_the_tree() {
        let fake = FakeCli::hanging("");
        let (_dir, spec) = spec(&fake, Some("text"), CLI_TIMEOUT);
        let probe = spec.pid_probe.clone().unwrap();
        let cancel = CancelToken::new();
        let runner = {
            let cancel = cancel.clone();
            tokio::spawn(async move { collect(spec, &cancel).await })
        };
        let tree = wait_for_tree(&probe).await;
        let cancelled_at = Instant::now();
        cancel.cancel();
        let (result, _) = runner.await.unwrap();
        assert!(
            cancelled_at.elapsed() < Duration::from_secs(1),
            "{:?}",
            cancelled_at.elapsed()
        );
        assert_eq!(result.unwrap_err(), TextError::Cancelled);
        for pid in tree {
            assert!(
                exited_within(pid, Duration::from_secs(3)),
                "pid {pid} survived"
            );
        }
    }

    #[tokio::test]
    async fn handler_errors_stop_the_run() {
        let fake = FakeCli::hanging("stop here\r\n");
        let (_dir, spec) = spec(&fake, None, CLI_TIMEOUT);
        let started = Instant::now();
        let result = run(spec, &CancelToken::new(), &mut |_| {
            Err(TextError::CliNotSignedIn)
        })
        .await;
        assert_eq!(result.unwrap_err(), TextError::CliNotSignedIn);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[tokio::test]
    async fn oversized_lines_are_rejected() {
        let fake = FakeCli::printing(&"x".repeat(MAX_LINE_BYTES + 10), 0);
        let (_dir, spec) = spec(&fake, None, CLI_TIMEOUT);
        let (result, _) = collect(spec, &CancelToken::new()).await;
        assert_eq!(result.unwrap_err(), TextError::BadResponse);
    }

    #[test]
    fn locate_prefers_the_explicit_path_then_path_then_known_locations() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(bin.join("claude.cmd"), "@echo off").unwrap();
        let path_var = Some(std::env::join_paths([dir.path().join("empty"), bin.clone()]).unwrap());
        assert_eq!(
            locate_in(CliKind::Claude, None, path_var.clone(), &[]),
            Some(bin.join("claude.cmd"))
        );
        // `.exe` wins over `.cmd` in the same directory.
        std::fs::write(bin.join("claude.exe"), "").unwrap();
        assert_eq!(
            locate_in(CliKind::Claude, None, path_var.clone(), &[]),
            Some(bin.join("claude.exe"))
        );
        // Not on PATH: no codex.
        assert_eq!(locate_in(CliKind::Codex, None, path_var.clone(), &[]), None);
        let known = dir.path().join("known-codex.exe");
        std::fs::write(&known, "").unwrap();
        assert_eq!(
            locate_in(
                CliKind::Codex,
                None,
                path_var.clone(),
                std::slice::from_ref(&known)
            ),
            Some(known.clone())
        );
        // An explicit path wins and never falls back.
        let explicit = known.display().to_string();
        assert_eq!(
            locate_in(
                CliKind::Claude,
                Some(&format!("\"{explicit}\"")),
                path_var.clone(),
                &[]
            ),
            Some(known.clone())
        );
        assert_eq!(
            locate_in(
                CliKind::Claude,
                Some(r"C:\nowhere\claude.exe"),
                path_var.clone(),
                &[]
            ),
            None
        );
        // Blank settings path means "search".
        assert_eq!(
            locate_in(CliKind::Claude, Some("  "), path_var, &[]),
            Some(bin.join("claude.exe"))
        );
    }

    #[test]
    fn npm_shims_resolve_to_the_native_binary() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("claude.cmd");
        std::fs::write(&shim, "@echo off").unwrap();
        std::fs::write(dir.path().join("claude.ps1"), "").unwrap();
        // Without the package the shim itself is used.
        assert_eq!(resolve_shim(CliKind::Claude, shim.clone()), shim);
        let native = dir
            .path()
            .join(r"node_modules\@anthropic-ai\claude-code\bin\claude.exe");
        std::fs::create_dir_all(native.parent().unwrap()).unwrap();
        std::fs::write(&native, "").unwrap();
        assert_eq!(resolve_shim(CliKind::Claude, shim.clone()), native);
        // A settings path naming the PowerShell shim finds the runnable sibling.
        let ps1 = dir.path().join("claude.ps1").display().to_string();
        assert_eq!(
            locate_in(CliKind::Claude, Some(&ps1), None, &[]),
            Some(native)
        );

        let codex_shim = dir.path().join("codex.cmd");
        std::fs::write(&codex_shim, "@echo off").unwrap();
        let codex = dir.path().join(
            r"node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe",
        );
        std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
        std::fs::write(&codex, "").unwrap();
        assert_eq!(resolve_shim(CliKind::Codex, codex_shim), codex);
    }

    fn status_fake(version_line: &str, auth_code: i32, auth_output: &str) -> FakeCli {
        FakeCli::new(
            auth_output,
            &format!(
                "if \"%~1\"==\"--version\" goto version\r\n\
                 type \"%~dp0canned.txt\"\r\n\
                 exit /b {auth_code}\r\n\
                 :version\r\n\
                 echo {version_line}\r\n\
                 exit /b 0"
            ),
        )
    }

    #[tokio::test]
    async fn status_reports_version_sign_in_and_plan_login() {
        let fake = status_fake(
            "2.1.325 (Claude Code)",
            0,
            "{\"loggedIn\": true, \"authMethod\": \"claude.ai\", \"email\": \"someone@example.com\"}",
        );
        let status = cli_status_with(CliKind::Claude, Some(fake.path()), STATUS_TIMEOUT).await;
        assert_eq!(
            status,
            CliStatus {
                installed: true,
                version: Some("2.1.325".into()),
                signed_in: Some(true),
                plan_login: Some(true),
                path: Some(fake.path_string()),
            }
        );
        // The status never carries account details.
        assert!(!serde_json::to_string(&status)
            .unwrap()
            .contains("example.com"));

        let api_key = status_fake(
            "2.1.0",
            0,
            "{\"loggedIn\": true, \"authMethod\": \"api_key\"}",
        );
        let status = cli_status_with(CliKind::Claude, Some(api_key.path()), STATUS_TIMEOUT).await;
        assert_eq!(status.plan_login, Some(false));

        let codex = status_fake("codex-cli 0.169.0", 0, "Logged in using ChatGPT");
        let status = cli_status_with(CliKind::Codex, Some(codex.path()), STATUS_TIMEOUT).await;
        assert_eq!(status.version.as_deref(), Some("0.169.0"));
        assert_eq!(
            (status.signed_in, status.plan_login),
            (Some(true), Some(true))
        );
    }

    #[tokio::test]
    async fn status_detects_signed_out_and_missing_clis() {
        let fake = status_fake("codex-cli 0.169.0", 1, "Not logged in");
        let status = cli_status_with(CliKind::Codex, Some(fake.path()), STATUS_TIMEOUT).await;
        assert!(status.installed);
        assert_eq!((status.signed_in, status.plan_login), (Some(false), None));

        let status = cli_status_with(CliKind::Codex, None, STATUS_TIMEOUT).await;
        assert_eq!(status, CliStatus::default());
        let missing = tempfile::tempdir().unwrap().path().join("codex.exe");
        let status = cli_status_with(CliKind::Codex, Some(missing), STATUS_TIMEOUT).await;
        assert!(!status.installed);
    }

    #[tokio::test]
    async fn status_commands_time_out() {
        let fake = FakeCli::hanging("");
        let started = Instant::now();
        let status = cli_status_with(
            CliKind::Claude,
            Some(fake.path()),
            Duration::from_millis(1500),
        )
        .await;
        assert!(started.elapsed() < Duration::from_secs(8));
        assert!(status.installed);
        assert_eq!((status.version, status.signed_in), (None, None));
    }

    /// Probes the CLIs installed on this machine with their version and status commands only
    /// (no model request, no plan usage). Prints no account details: `CliStatus` has none.
    #[tokio::test]
    #[ignore = "uses the locally installed CLIs"]
    async fn live_status_probe() {
        for kind in [CliKind::Codex, CliKind::Claude] {
            let started = Instant::now();
            let status = cli_status(kind, None).await;
            println!("{kind:?}: {status:?} in {:?}", started.elapsed());
        }
    }

    #[test]
    fn parses_versions() {
        assert_eq!(
            parse_version("codex-cli 0.169.0\n").as_deref(),
            Some("0.169.0")
        );
        assert_eq!(
            parse_version("2.1.325 (Claude Code)").as_deref(),
            Some("2.1.325")
        );
        assert_eq!(parse_version("\n  weird \n").as_deref(), Some("weird"));
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn classifies_failures_without_echoing_them() {
        assert_eq!(
            classify_failure("Not logged in · Please run /login"),
            TextError::CliNotSignedIn
        );
        assert_eq!(
            classify_failure("You've hit your usage limit"),
            TextError::RateLimited
        );
        assert_eq!(classify_failure("something odd"), TextError::BadResponse);
    }

    #[test]
    fn model_arguments_are_checked() {
        for ok in ["haiku", "gpt-6.1-sol", "claude-opus-5-5[1m]", "sonnet"] {
            assert!(valid_model_arg(ok), "{ok}");
        }
        for bad in ["", "--dangerously-skip-permissions", "a b", "x\"y", "a&b"] {
            assert!(!valid_model_arg(bad), "{bad}");
        }
    }
}
