use std::ffi::{CStr, OsStr, OsString};
use std::mem::MaybeUninit;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use talos_tools_sandbox::ToolProcessTerminationTarget;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::{is_executable, valid_entry, WorkerToolEnvironment};

const TIMEOUT: Duration = Duration::from_secs(3);
const MAX_OUTPUT: u64 = 64 * 1024;
const FRAME: &[u8] = b"\0talos-worker-path\0";
const CAPTURE: &str = r#"\command printf '\000talos-worker-path\000%s\000' "${PATH-}""#;
const ZSH_CAPTURE: &str = r#"\builtin printf '\000talos-worker-path\000%s\000' "${PATH-}""#;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Unavailable {
    Account,
    UnsupportedShell,
    Launch,
    Capture,
    OutputLimit,
    Timeout,
    ShellExit,
    InvalidPath,
}

struct Account {
    home: PathBuf,
    shell: PathBuf,
    name: OsString,
}

pub(super) async fn discover_path(inherited: Option<&OsStr>) -> Result<OsString, Unavailable> {
    let account = tokio::time::timeout(TIMEOUT, tokio::task::spawn_blocking(worker_account))
        .await
        .map_err(|_| Unavailable::Timeout)?
        .map_err(|_| Unavailable::Account)??;
    let (mode, script) = shell_probe(&account.shell).ok_or(Unavailable::UnsupportedShell)?;
    if !account.shell.is_absolute() || !is_executable(&account.shell) || !account.home.is_absolute()
    {
        return Err(Unavailable::Account);
    }
    let inherited = WorkerToolEnvironment::assemble(&[], None, inherited);
    let mut command = Command::new(&account.shell);
    command
        .args([mode, script])
        .current_dir("/")
        .env_clear()
        .env("HOME", account.home)
        .env("SHELL", &account.shell)
        .env("USER", &account.name)
        .env("LOGNAME", &account.name)
        .env("PATH", inherited.path)
        .env("TERM", "dumb");
    capture(command, TIMEOUT).await
}

fn shell_probe(shell: &Path) -> Option<(&'static str, &'static str)> {
    match shell.file_name()?.as_encoded_bytes() {
        // zsh profiles commonly set PATH in .zshrc. Bash enables job control
        // for interactive shells even without a TTY; keep its probe a login
        // shell without interactive job control so jobs stay in our group.
        b"zsh" => Some(("-ilc", ZSH_CAPTURE)),
        b"bash" | b"sh" | b"dash" => Some(("-lc", CAPTURE)),
        _ => None,
    }
}

fn worker_account() -> Result<Account, Unavailable> {
    let mut pwd = MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buf = vec![0_u8; 64 * 1024];
    // SAFETY: The result and buffer remain live while getpwuid_r writes them
    // and while the returned account strings are copied below.
    let status = unsafe {
        libc::getpwuid_r(
            libc::geteuid(),
            pwd.as_mut_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return Err(Unavailable::Account);
    }
    // SAFETY: A successful non-null result points to the initialized record;
    // its non-null string fields point to NUL-terminated data in `buf`.
    unsafe {
        let pwd = pwd.assume_init();
        if pwd.pw_dir.is_null() || pwd.pw_shell.is_null() || pwd.pw_name.is_null() {
            return Err(Unavailable::Account);
        }
        let copy = |ptr| OsString::from_vec(CStr::from_ptr(ptr).to_bytes().to_vec());
        Ok(Account {
            home: copy(pwd.pw_dir).into(),
            shell: copy(pwd.pw_shell).into(),
            name: copy(pwd.pw_name),
        })
    }
}

struct ProbeProcessGroup(ToolProcessTerminationTarget);

impl Drop for ProbeProcessGroup {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

async fn capture(mut command: Command, timeout: Duration) -> Result<OsString, Unavailable> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // SAFETY: before exec the child calls only async-signal-safe setsid and,
    // on failure, reads errno. The new session has no controlling terminal;
    // its initial process group ID equals the child PID.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|_| Unavailable::Launch)?;
    let pid = child.id().ok_or(Unavailable::Launch)?;
    let group = ProbeProcessGroup(ToolProcessTerminationTarget::ProcessGroup(pid));
    let stdout = child.stdout.take().ok_or(Unavailable::Capture)?;
    let result = tokio::time::timeout(timeout, async {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_OUTPUT + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| Unavailable::Capture)?;
        if bytes.len() as u64 > MAX_OUTPUT {
            return Err(Unavailable::OutputLimit);
        }
        if !child
            .wait()
            .await
            .map_err(|_| Unavailable::Capture)?
            .success()
        {
            return Err(Unavailable::ShellExit);
        }
        parse_path(&bytes)
    })
    .await;
    drop(group);
    if !matches!(result, Ok(Ok(_))) {
        // Reap promptly when possible; Tokio retains responsibility for orphaned
        // children if the kernel does not return from wait in this interval.
        let _ = tokio::time::timeout(Duration::from_millis(250), child.wait()).await;
    }
    result.map_err(|_| Unavailable::Timeout)?
}

fn parse_path(bytes: &[u8]) -> Result<OsString, Unavailable> {
    let mut frames = bytes
        .windows(FRAME.len())
        .enumerate()
        .filter_map(|(i, window)| (window == FRAME).then_some(i + FRAME.len()));
    let start = frames.next().ok_or(Unavailable::InvalidPath)?;
    if frames.next().is_some() {
        return Err(Unavailable::InvalidPath);
    }
    let rest = &bytes[start..];
    let end = rest
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(Unavailable::InvalidPath)?;
    let path = OsStr::from_bytes(&rest[..end]);
    if !std::env::split_paths(path).any(|entry| valid_entry(entry.as_os_str())) {
        return Err(Unavailable::InvalidPath);
    }
    Ok(path.to_owned())
}

#[cfg(test)]
#[path = "probe_tests.rs"]
mod tests;
