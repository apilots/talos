use std::ffi::CString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use talos_sandbox_spi::{NetworkIsolationSpec, SandboxStdin};

use crate::protocol::{LauncherRequest, MAX_LAUNCHER_PAYLOAD_BYTES};

const LINUX_RUNTIME_READ_ROOTS: &[&str] = &[
    "/bin",
    "/sbin",
    "/usr",
    "/etc",
    "/lib",
    "/lib64",
    "/nix/store",
    "/run/current-system/sw",
];

pub(crate) fn run() -> Result<(), String> {
    match std::env::args().nth(1).as_deref() {
        Some("--outer") => run_outer(),
        Some("--inner-restricted") => run_inner(/* restricted */ true),
        Some("--inner-enabled") => run_inner(/* restricted */ false),
        Some("--probe-seccomp") => crate::seccomp::apply_restricted_network_filter(),
        _ => Err("expected an internal launcher mode".to_string()),
    }
}

fn run_outer() -> Result<(), String> {
    let request = read_request()?;
    request.validate()?;
    let argv = build_bubblewrap_argv(&request)?;
    restore_target_stdin(&request.command.stdin)?;
    exec(&request.bubblewrap_executable, &argv)
}

fn run_inner(restricted: bool) -> Result<(), String> {
    let args = std::env::args().skip(2).collect::<Vec<_>>();
    let Some((program, command_args)) = args.split_first() else {
        return Err("inner launcher requires a target command".to_string());
    };
    if restricted {
        crate::seccomp::apply_restricted_network_filter()?;
    }
    exec(Path::new(program), command_args)
}

fn read_request() -> Result<LauncherRequest, String> {
    let mut payload = Vec::new();
    std::io::stdin()
        .take((MAX_LAUNCHER_PAYLOAD_BYTES + 1) as u64)
        .read_to_end(&mut payload)
        .map_err(|error| format!("failed to read launcher request: {error}"))?;
    if payload.len() > MAX_LAUNCHER_PAYLOAD_BYTES {
        return Err("launcher request exceeds the payload limit".to_string());
    }
    serde_json::from_slice(&payload)
        .map_err(|error| format!("invalid launcher request payload: {error}"))
}

fn build_bubblewrap_argv(request: &LauncherRequest) -> Result<Vec<String>, String> {
    let mut args = vec![
        "--new-session".to_string(),
        "--die-with-parent".to_string(),
        "--tmpfs".to_string(),
        "/".to_string(),
        "--unshare-user".to_string(),
        "--unshare-pid".to_string(),
    ];
    if request.network == NetworkIsolationSpec::Restricted {
        args.push("--unshare-net".to_string());
    }
    for root in LINUX_RUNTIME_READ_ROOTS
        .iter()
        .map(Path::new)
        .filter(|root| root.exists())
    {
        push_mount(&mut args, "--ro-bind", root);
    }
    push_mount(&mut args, "--ro-bind", request.helper_executable.as_path());
    push_mount(&mut args, "--ro-bind", request.command.program.as_path());
    for root in &request.file_system.readable_roots {
        push_mount(&mut args, "--ro-bind", root);
    }
    // A readable host root must not shadow the private pseudo-filesystems.
    args.extend([
        "--dev".to_string(),
        "/dev".to_string(),
        "--proc".to_string(),
        "/proc".to_string(),
    ]);
    for root in &request.file_system.writable_roots {
        push_mount(&mut args, "--bind", root);
    }
    for root in &request.file_system.denied_roots {
        args.push("--tmpfs".to_string());
        args.push(path_arg(root)?);
    }

    args.push("--clearenv".to_string());
    for (key, value) in &request.command.env {
        args.extend(["--setenv".to_string(), key.clone(), value.clone()]);
    }
    args.extend([
        "--chdir".to_string(),
        path_arg(&request.command.cwd)?,
        "--".to_string(),
        path_arg(&request.helper_executable)?,
        match request.network {
            NetworkIsolationSpec::Restricted => "--inner-restricted".to_string(),
            NetworkIsolationSpec::Enabled => "--inner-enabled".to_string(),
        },
        path_arg(&request.command.program)?,
    ]);
    args.extend(request.command.args.iter().cloned());
    Ok(args)
}

fn restore_target_stdin(stdin: &SandboxStdin) -> Result<(), String> {
    let source = match stdin {
        SandboxStdin::Closed => File::open("/dev/null")
            .map_err(|error| format!("failed to open /dev/null for target stdin: {error}"))?,
        SandboxStdin::Bytes(bytes) => {
            let name = CString::new("talos-sandbox-stdin")
                .map_err(|error| format!("invalid target stdin memfd name: {error}"))?;
            // SAFETY: memfd_create receives a valid C string and documented flags.
            let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
            if fd < 0 {
                return Err(format!(
                    "failed to create target stdin memfd: {}",
                    std::io::Error::last_os_error()
                ));
            }
            // SAFETY: fd was returned by memfd_create and ownership moves to File.
            let mut file = unsafe { File::from_raw_fd(fd) };
            file.write_all(bytes)
                .map_err(|error| format!("failed to stage target stdin: {error}"))?;
            file.seek(SeekFrom::Start(0))
                .map_err(|error| format!("failed to rewind target stdin: {error}"))?;
            file
        }
    };
    // SAFETY: source is a valid open fd and STDIN_FILENO is the intended target.
    if unsafe { libc::dup2(source.as_raw_fd(), libc::STDIN_FILENO) } < 0 {
        return Err(format!(
            "failed to restore target stdin: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn push_mount(args: &mut Vec<String>, operation: &str, root: &Path) {
    let root = root.as_os_str().to_string_lossy().into_owned();
    args.extend([operation.to_string(), root.clone(), root]);
}

fn path_arg(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("launcher path is not UTF-8: {}", path.display()))
}

fn exec(program: &Path, args: &[String]) -> Result<(), String> {
    let program_c = CString::new(program.as_os_str().as_bytes())
        .map_err(|_| format!("program contains a NUL byte: {}", program.display()))?;
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push(program_c.clone());
    for arg in args {
        argv.push(
            CString::new(arg.as_bytes())
                .map_err(|_| "launcher argument contains a NUL byte".to_string())?,
        );
    }
    let mut pointers = argv.iter().map(|arg| arg.as_ptr()).collect::<Vec<_>>();
    pointers.push(std::ptr::null());

    // SAFETY: program and argv are valid C strings and remain alive for execv.
    unsafe { libc::execv(program_c.as_ptr(), pointers.as_ptr()) };
    Err(format!(
        "failed to exec {}: {}",
        program.display(),
        std::io::Error::last_os_error()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_protocol_version() {
        let protocol_version = crate::protocol::PROTOCOL_VERSION;
        let request = LauncherRequest {
            protocol_version: protocol_version + 1,
            bubblewrap_executable: "/bin/true".into(),
            helper_executable: "/opt/talos/launcher".into(),
            command: crate::protocol::LauncherCommand {
                program: "/opt/talos/program".into(),
                args: Vec::new(),
                env: Default::default(),
                cwd: "/".into(),
                stdin: SandboxStdin::Closed,
            },
            file_system: crate::protocol::LauncherFileSystem {
                readable_roots: vec!["/".into()],
                writable_roots: Vec::new(),
                denied_roots: Vec::new(),
            },
            network: NetworkIsolationSpec::Restricted,
        };

        assert_eq!(
            request.validate(),
            Err(format!(
                "unsupported launcher protocol version: {}",
                protocol_version + 1
            ))
        );
    }

    #[test]
    fn restricted_mount_plan_uses_declared_roots_without_host_root() {
        let request = LauncherRequest {
            protocol_version: crate::protocol::PROTOCOL_VERSION,
            bubblewrap_executable: "/bin/true".into(),
            helper_executable: "/opt/talos/launcher".into(),
            command: crate::protocol::LauncherCommand {
                program: "/opt/talos/program".into(),
                args: Vec::new(),
                env: Default::default(),
                cwd: "/workspace".into(),
                stdin: SandboxStdin::Closed,
            },
            file_system: crate::protocol::LauncherFileSystem {
                readable_roots: vec!["/workspace".into(), "/readonly".into()],
                writable_roots: vec!["/workspace".into()],
                denied_roots: vec!["/workspace/secret".into()],
            },
            network: NetworkIsolationSpec::Restricted,
        };

        let args = build_bubblewrap_argv(&request).expect("mount plan should be built");

        assert!(!args
            .windows(3)
            .any(|window| window == ["--ro-bind", "/", "/"]));
        assert!(args
            .windows(3)
            .any(|window| window == ["--ro-bind", "/readonly", "/readonly"]));
        assert!(args.windows(3).any(|window| {
            window == ["--ro-bind", "/opt/talos/launcher", "/opt/talos/launcher"]
        }));
        assert!(args
            .windows(3)
            .any(|window| { window == ["--ro-bind", "/opt/talos/program", "/opt/talos/program"] }));
        assert!(args
            .windows(3)
            .any(|window| window == ["--bind", "/workspace", "/workspace"]));
        assert!(args
            .windows(2)
            .any(|window| window == ["--tmpfs", "/workspace/secret"]));
    }

    #[test]
    fn pseudo_filesystems_override_readable_host_root() {
        let request = LauncherRequest {
            protocol_version: crate::protocol::PROTOCOL_VERSION,
            bubblewrap_executable: "/bin/true".into(),
            helper_executable: "/opt/talos/launcher".into(),
            command: crate::protocol::LauncherCommand {
                program: "/opt/talos/program".into(),
                args: Vec::new(),
                env: Default::default(),
                cwd: "/workspace".into(),
                stdin: SandboxStdin::Closed,
            },
            file_system: crate::protocol::LauncherFileSystem {
                readable_roots: vec!["/".into()],
                writable_roots: vec!["/workspace".into()],
                denied_roots: vec!["/workspace/secret".into()],
            },
            network: NetworkIsolationSpec::Restricted,
        };

        let args = build_bubblewrap_argv(&request).expect("mount plan should be built");
        let root_bind = args
            .windows(3)
            .position(|window| window == ["--ro-bind", "/", "/"])
            .expect("readable host root must be mounted");
        let dev_mount = args
            .windows(2)
            .position(|window| window == ["--dev", "/dev"])
            .expect("private dev must be mounted");
        let proc_mount = args
            .windows(2)
            .position(|window| window == ["--proc", "/proc"])
            .expect("private proc must be mounted");
        let denied_mount = args
            .windows(2)
            .position(|window| window == ["--tmpfs", "/workspace/secret"])
            .expect("denied root must be masked");

        assert!(root_bind < dev_mount);
        assert!(root_bind < proc_mount);
        assert!(dev_mount < denied_mount);
        assert!(proc_mount < denied_mount);
    }
}
