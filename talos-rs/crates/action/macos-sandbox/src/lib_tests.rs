use std::collections::BTreeMap;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use talos_sandbox_spi::{
    provider_contract, AbsolutePathBuf, FileSystemIsolationSpec, NetworkIsolationSpec,
    ProviderContractExpectation, SandboxExecutionCommand, SandboxExecutionRequest,
    SandboxProviderPlatform, SandboxProviderSnapshot, SandboxRuntimeRequirements, SandboxStdin,
};

use super::*;

fn absolute(path: &Path) -> AbsolutePathBuf {
    let mut current = path;
    let mut suffix = Vec::new();
    while !current.exists() {
        suffix.push(
            current
                .file_name()
                .expect("test path must have an existing ancestor")
                .to_owned(),
        );
        current = current
            .parent()
            .expect("test path must have an existing ancestor");
    }
    let mut normalized = std::fs::canonicalize(current).expect("test path must canonicalize");
    for component in suffix.into_iter().rev() {
        normalized.push(component);
    }
    AbsolutePathBuf::new(normalized).expect("test path must be absolute")
}

fn request(
    program: &str,
    args: Vec<String>,
    cwd: &Path,
    readable_roots: Vec<AbsolutePathBuf>,
    writable_roots: Vec<AbsolutePathBuf>,
    denied_roots: Vec<AbsolutePathBuf>,
    network: NetworkIsolationSpec,
) -> SandboxExecutionRequest {
    let command = SandboxExecutionCommand::new(
        absolute(Path::new(program)),
        args,
        BTreeMap::new(),
        absolute(cwd),
        SandboxStdin::Closed,
    )
    .expect("test command must be valid");
    let file_system = FileSystemIsolationSpec::new(readable_roots, writable_roots, denied_roots)
        .expect("test filesystem policy must be valid");
    SandboxExecutionRequest::new(
        SandboxProviderPlatform::MacosSeatbelt,
        command,
        file_system,
        network,
        SandboxRuntimeRequirements { private_tmp: false },
    )
    .expect("test request must be valid")
}

fn run(provider: &MacosSandboxProvider, request: SandboxExecutionRequest) -> Output {
    let prepared = provider.prepare(request).expect("request must prepare");
    let command = prepared.command();
    Command::new(command.program().as_path())
        .args(command.args())
        .env_clear()
        .envs(command.env())
        .current_dir(command.cwd().as_path())
        .stdin(Stdio::null())
        .output()
        .expect("prepared command must execute")
}

#[test]
fn profile_uses_parameters_and_keeps_restricted_network_closed() {
    let root = tempfile::tempdir().expect("temporary workspace must be created");
    let denied = root.path().join("quoted-path");
    let request = request(
        "/bin/echo",
        vec!["hello".to_string()],
        root.path(),
        vec![absolute(root.path())],
        vec![absolute(root.path())],
        vec![absolute(&denied)],
        NetworkIsolationSpec::Restricted,
    );

    let (policy, definitions) = super::profile::build_seatbelt_profile(&request, Some(root.path()));
    assert!(policy.starts_with("; Adapted from OpenAI Codex"));
    assert!(policy.contains("(deny default)"));
    assert!(!policy.contains("(allow network-outbound)\n(allow network-inbound)"));
    assert!(definitions
        .iter()
        .any(|(name, path)| name == "WRITABLE_ROOT_0" && path == absolute(root.path()).as_path()));
    assert!(definitions
        .iter()
        .any(|(name, path)| name == "DENIED_ROOT_0" && path == absolute(&denied).as_path()));
    assert!(policy.contains("/xcrun_db(-[^/]+)?$"));
    let (without_host_tmp, _) = super::profile::build_seatbelt_profile(&request, None);
    assert!(!without_host_tmp.contains("/xcrun_db(-[^/]+)?$"));
    assert!(policy.contains("(subpath \"/Library/Developer/CommandLineTools\")"));
    assert!(policy.contains("(subpath \"/opt/homebrew/opt\")"));
    assert!(policy.contains("(subpath \"/opt/homebrew/Cellar\")"));
    assert!(policy.contains("(subpath \"/usr/local/opt\")"));
    assert!(policy.contains("(subpath \"/usr/local/Cellar\")"));
    assert!(policy.contains("(sysctl-name \"hw.pagesize_compat\")"));
}

#[cfg(target_os = "macos")]
#[test]
fn provider_satisfies_shared_contract() {
    provider_contract::assert_provider_contract(
        &MacosSandboxProvider::new(),
        SandboxProviderPlatform::MacosSeatbelt,
        ProviderContractExpectation::Supported,
    );
}

#[cfg(target_os = "macos")]
#[test]
fn read_only_denies_write_and_workspace_write_stays_within_root() {
    let provider = MacosSandboxProvider::new();
    let workspace = tempfile::tempdir().expect("temporary workspace must be created");
    let outside = tempfile::tempdir().expect("outside directory must be created");
    let inside_path = workspace.path().join("inside.txt");
    let outside_path = outside.path().join("outside.txt");

    let read_only = request(
        "/bin/sh",
        vec!["-c".to_string(), format!("touch {}", inside_path.display())],
        workspace.path(),
        vec![absolute(workspace.path())],
        Vec::new(),
        Vec::new(),
        NetworkIsolationSpec::Restricted,
    );
    assert!(!run(&provider, read_only).status.success());
    assert!(!inside_path.exists());

    let workspace_write = request(
        "/bin/sh",
        vec!["-c".to_string(), format!("touch {}", inside_path.display())],
        workspace.path(),
        vec![absolute(workspace.path())],
        vec![absolute(workspace.path())],
        Vec::new(),
        NetworkIsolationSpec::Restricted,
    );
    let output = run(&provider, workspace_write);
    assert!(
        output.status.success(),
        "workspace write failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    assert!(inside_path.exists());

    let outside_write = request(
        "/bin/sh",
        vec![
            "-c".to_string(),
            format!("touch {}", outside_path.display()),
        ],
        workspace.path(),
        vec![absolute(workspace.path())],
        vec![absolute(workspace.path())],
        Vec::new(),
        NetworkIsolationSpec::Restricted,
    );
    assert!(!run(&provider, outside_write).status.success());
    assert!(!outside_path.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn xcrun_python_cache_is_limited_to_the_named_runtime_files() {
    let provider = MacosSandboxProvider::new();
    let workspace = tempfile::tempdir().expect("temporary workspace must be created");
    let request = request(
        "/bin/sh",
        vec![
            "-c".to_string(),
            "xcrun_nocache=1 /usr/bin/python3 --version".to_string(),
        ],
        workspace.path(),
        vec![absolute(workspace.path())],
        vec![absolute(workspace.path())],
        Vec::new(),
        NetworkIsolationSpec::Restricted,
    );

    let output = run(&provider, request);

    assert!(
        output.status.success(),
        "xcrun Python failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("Operation not permitted"),
        "xcrun cache should use only the named runtime files: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_os = "macos")]
#[test]
fn denied_root_blocks_read() {
    let provider = MacosSandboxProvider::new();
    let workspace = tempfile::tempdir().expect("temporary workspace must be created");
    let denied = workspace.path().join("denied");
    std::fs::create_dir(&denied).expect("denied directory must be created");
    let secret = denied.join("secret.txt");
    std::fs::write(&secret, "secret").expect("secret must be written");
    let request = request(
        "/bin/cat",
        vec![secret.display().to_string()],
        workspace.path(),
        vec![absolute(workspace.path())],
        Vec::new(),
        vec![absolute(&denied)],
        NetworkIsolationSpec::Restricted,
    );

    assert!(!run(&provider, request).status.success());
}

#[cfg(target_os = "macos")]
#[test]
fn restricted_network_denies_local_tcp_and_enabled_network_allows_it() {
    let provider = MacosSandboxProvider::new();
    let workspace = tempfile::tempdir().expect("temporary workspace must be created");
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener must bind");
    let port = listener
        .local_addr()
        .expect("listener address must resolve")
        .port();
    let roots = vec![absolute(workspace.path())];

    let restricted = request(
        "/usr/bin/nc",
        vec!["-z".to_string(), "127.0.0.1".to_string(), port.to_string()],
        workspace.path(),
        roots.clone(),
        Vec::new(),
        Vec::new(),
        NetworkIsolationSpec::Restricted,
    );
    assert!(!run(&provider, restricted).status.success());

    let enabled = request(
        "/usr/bin/nc",
        vec!["-z".to_string(), "127.0.0.1".to_string(), port.to_string()],
        workspace.path(),
        roots,
        Vec::new(),
        Vec::new(),
        NetworkIsolationSpec::Enabled,
    );
    assert!(run(&provider, enabled).status.success());
}

#[cfg(target_os = "macos")]
#[test]
fn probe_snapshot_supports_required_capabilities() {
    let provider = MacosSandboxProvider::new();
    let capabilities = provider
        .probe(SandboxProviderPlatform::MacosSeatbelt)
        .expect("Seatbelt must probe");
    assert!(capabilities.private_tmp);
    assert!(matches!(
        SandboxProviderSnapshot::from(capabilities),
        SandboxProviderSnapshot::Available { .. }
    ));
}
