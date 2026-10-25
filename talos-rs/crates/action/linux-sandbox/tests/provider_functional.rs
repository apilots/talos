#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;

use talos_linux_sandbox::{LinuxSandboxProvider, LinuxSandboxProviderConfig};
use talos_sandbox_spi::provider_contract::{assert_provider_contract, ProviderContractExpectation};
use talos_sandbox_spi::{
    AbsolutePathBuf, FileSystemIsolationSpec, NetworkIsolationSpec, SandboxExecutionCommand,
    SandboxExecutionRequest, SandboxProvider, SandboxProviderPlatform, SandboxRuntimeRequirements,
    SandboxStdin,
};

const BWRAP: &str = "/usr/bin/bwrap";

fn provider() -> LinuxSandboxProvider {
    LinuxSandboxProvider::new(
        LinuxSandboxProviderConfig::with_bubblewrap(
            PathBuf::from(
                std::env::var_os("CARGO_BIN_EXE_talos-linux-sandbox-launcher")
                    .expect("Cargo must provide the sandbox launcher path"),
            ),
            PathBuf::from(BWRAP),
        )
        .expect("Linux provider config must be valid"),
    )
}

fn absolute(path: &Path) -> AbsolutePathBuf {
    AbsolutePathBuf::new(path.to_path_buf()).expect("test path must be absolute")
}

fn request(
    program: &Path,
    args: Vec<String>,
    cwd: &Path,
    writable_roots: Vec<AbsolutePathBuf>,
    denied_roots: Vec<AbsolutePathBuf>,
    network: NetworkIsolationSpec,
    stdin: SandboxStdin,
) -> SandboxExecutionRequest {
    let cwd = absolute(cwd);
    SandboxExecutionRequest::new(
        SandboxProviderPlatform::LinuxBubblewrap,
        SandboxExecutionCommand::new(absolute(program), args, BTreeMap::new(), cwd.clone(), stdin)
            .expect("test command must be valid"),
        FileSystemIsolationSpec::new(vec![cwd], writable_roots, denied_roots)
            .expect("test filesystem must be valid"),
        network,
        SandboxRuntimeRequirements { private_tmp: false },
    )
    .expect("test request must be valid")
}

fn run(provider: &LinuxSandboxProvider, request: SandboxExecutionRequest) -> Output {
    let prepared = provider
        .prepare(request)
        .expect("sandbox request must prepare")
        .into_command();
    let mut child = Command::new(prepared.program().as_path())
        .args(prepared.args())
        .env_clear()
        .envs(prepared.env())
        .current_dir(prepared.cwd().as_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sandbox launcher must spawn");
    let SandboxStdin::Bytes(payload) = prepared.stdin() else {
        panic!("launcher request must be provided on stdin");
    };
    child
        .stdin
        .take()
        .expect("launcher stdin must exist")
        .write_all(payload)
        .expect("launcher payload must write");
    child.wait_with_output().expect("launcher must exit")
}

#[test]
fn satisfies_shared_provider_contract() {
    assert_provider_contract(
        &provider(),
        SandboxProviderPlatform::LinuxBubblewrap,
        ProviderContractExpectation::Supported,
    );
}

#[test]
fn enforces_workspace_write_denied_root_and_nested_child_policy() {
    let provider = provider();
    provider
        .probe(SandboxProviderPlatform::LinuxBubblewrap)
        .expect("Linux sandbox preflight must pass");
    let workspace = tempfile::tempdir().expect("workspace must be created");
    let outside = tempfile::tempdir().expect("outside root must be created");
    let denied = workspace.path().join("denied");
    std::fs::create_dir(&denied).expect("denied root must be created");
    std::fs::write(denied.join("secret"), "secret").expect("secret must be written");

    let output = run(
        &provider,
        request(
            Path::new("/bin/sh"),
            vec![
                "-c".to_string(),
                "read input; test \"$input\" = payload; echo allowed > inside; test ! -e denied/secret; /bin/sh -c 'echo blocked > \"$1/nested\"' child \"$1\"; test $? -ne 0".to_string(),
                "talos".to_string(),
                outside.path().to_string_lossy().into_owned(),
            ],
            workspace.path(),
            vec![absolute(workspace.path())],
            vec![absolute(&denied)],
            NetworkIsolationSpec::Restricted,
            SandboxStdin::Bytes(b"payload\n".to_vec()),
        ),
    );
    assert!(
        output.status.success(),
        "sandbox failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("inside")).expect("inside file must exist"),
        "allowed\n"
    );
    assert!(!outside.path().join("nested").exists());
}

#[test]
fn restricted_network_fails_after_enabled_positive_control_succeeds() {
    let provider = provider();
    let workspace = tempfile::tempdir().expect("workspace must be created");
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener must bind");
    let address = listener
        .local_addr()
        .expect("listener address must resolve");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("enabled request must connect");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
            .expect("response must write");
    });
    let url = format!("http://{address}/");

    let enabled = run(
        &provider,
        request(
            Path::new("/usr/bin/curl"),
            vec![
                "--fail".to_string(),
                "--max-time".to_string(),
                "2".to_string(),
                url.clone(),
            ],
            workspace.path(),
            Vec::new(),
            Vec::new(),
            NetworkIsolationSpec::Enabled,
            SandboxStdin::Closed,
        ),
    );
    assert!(
        enabled.status.success(),
        "enabled positive control failed: {}",
        String::from_utf8_lossy(&enabled.stderr)
    );
    server.join().expect("server must finish");

    let restricted = run(
        &provider,
        request(
            Path::new("/usr/bin/curl"),
            vec![
                "--fail".to_string(),
                "--max-time".to_string(),
                "2".to_string(),
                url,
            ],
            workspace.path(),
            Vec::new(),
            Vec::new(),
            NetworkIsolationSpec::Restricted,
            SandboxStdin::Closed,
        ),
    );
    assert!(!restricted.status.success());
}
