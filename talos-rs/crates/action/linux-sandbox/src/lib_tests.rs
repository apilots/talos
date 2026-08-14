use std::collections::BTreeMap;
use std::path::PathBuf;

use talos_sandbox_spi::{
    AbsolutePathBuf, FileSystemIsolationSpec, NetworkIsolationSpec, SandboxExecutionCommand,
    SandboxExecutionRequest, SandboxProvider, SandboxProviderPlatform, SandboxRuntimeRequirements,
    SandboxStdin,
};

use super::*;

fn request(stdin: SandboxStdin) -> SandboxExecutionRequest {
    let root = AbsolutePathBuf::new(PathBuf::from("/")).expect("root must be absolute");
    let executable = std::env::current_exe().expect("test executable must resolve");
    let command = SandboxExecutionCommand::new(
        AbsolutePathBuf::new(executable).expect("program must be absolute"),
        Vec::new(),
        BTreeMap::new(),
        root.clone(),
        stdin,
    )
    .expect("command must be valid");
    SandboxExecutionRequest::new(
        SandboxProviderPlatform::LinuxBubblewrap,
        command,
        FileSystemIsolationSpec::new(vec![root], Vec::new(), Vec::new())
            .expect("filesystem must be valid"),
        NetworkIsolationSpec::Restricted,
        SandboxRuntimeRequirements { private_tmp: false },
    )
    .expect("request must be valid")
}

fn provider() -> LinuxSandboxProvider {
    let executable = std::env::current_exe().expect("test executable must resolve");
    LinuxSandboxProvider::new(
        LinuxSandboxProviderConfig::with_bubblewrap(executable.clone(), executable)
            .expect("provider config must be valid"),
    )
}

#[test]
fn prepares_bounded_private_launcher_request() {
    let command = provider()
        .prepare(request(SandboxStdin::Closed))
        .expect("request must prepare")
        .into_command();
    assert_eq!(
        command.program().as_path(),
        std::env::current_exe().expect("test executable must resolve")
    );
    assert_eq!(command.args(), ["--outer"]);
    let SandboxStdin::Bytes(payload) = command.stdin() else {
        panic!("launcher payload must use stdin");
    };
    assert!(payload.len() <= protocol::MAX_LAUNCHER_PAYLOAD_BYTES);
    let decoded: LauncherRequest =
        serde_json::from_slice(payload).expect("launcher payload must decode");
    assert_eq!(decoded.protocol_version, PROTOCOL_VERSION);
    assert_eq!(
        decoded.command.program,
        std::env::current_exe().expect("test executable must resolve")
    );
}

#[test]
fn carries_target_stdin_in_the_private_launcher_request() {
    let command = provider()
        .prepare(request(SandboxStdin::Bytes(b"input".to_vec())))
        .expect("request must prepare")
        .into_command();
    let SandboxStdin::Bytes(payload) = command.stdin() else {
        panic!("launcher payload must use stdin");
    };
    let decoded: LauncherRequest =
        serde_json::from_slice(payload).expect("launcher payload must decode");
    assert_eq!(
        decoded.command.stdin,
        SandboxStdin::Bytes(b"input".to_vec())
    );
}
