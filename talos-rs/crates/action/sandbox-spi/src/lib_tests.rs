use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;

use super::*;

fn absolute(path: &str) -> AbsolutePathBuf {
    let base = env::current_dir().expect("current directory must resolve");
    AbsolutePathBuf::new(base.join(path.trim_start_matches('/')))
        .expect("test path must be absolute")
}

fn command() -> SandboxExecutionCommand {
    SandboxExecutionCommand::new(
        absolute("/bin/echo"),
        vec!["hello".to_string()],
        BTreeMap::from([("LANG".to_string(), "C".to_string())]),
        absolute("/tmp"),
        SandboxStdin::Closed,
    )
    .expect("test command must be valid")
}

#[test]
fn absolute_path_rejects_relative_input_and_normalizes_components() {
    assert_eq!(
        AbsolutePathBuf::new(PathBuf::from("workspace/file")),
        Err(SandboxProviderError::InvalidIsolationSpec {
            reason: "path must be absolute: workspace/file".to_string(),
        })
    );
    assert_eq!(
        absolute("workspace/./src/../Cargo.toml"),
        absolute("/workspace/Cargo.toml")
    );
}

#[test]
fn filesystem_spec_applies_precedence_and_compacts_same_access_roots() {
    let spec = FileSystemIsolationSpec::new(
        vec![absolute("/workspace"), absolute("/workspace/src")],
        vec![
            absolute("/workspace/target"),
            absolute("/workspace/target/debug"),
        ],
        vec![absolute("/workspace/target/private")],
    )
    .expect("filesystem spec must be valid");

    assert_eq!(
        spec,
        FileSystemIsolationSpec::new(
            vec![absolute("/workspace")],
            vec![absolute("/workspace/target")],
            vec![absolute("/workspace/target/private")],
        )
        .expect("expected filesystem spec must be valid")
    );
    assert_eq!(
        spec.access_for(&absolute("/workspace/target/private/key")),
        SandboxPathAccess::Denied
    );
    assert_eq!(
        spec.access_for(&absolute("/workspace/target/output")),
        SandboxPathAccess::Writable
    );
    assert_eq!(
        spec.access_for(&absolute("/workspace/src/lib.rs")),
        SandboxPathAccess::Readable
    );
    assert_eq!(
        spec.access_for(&absolute("/outside")),
        SandboxPathAccess::Denied
    );
}

#[test]
fn denied_parent_removes_ineffective_allowed_roots() {
    let spec = FileSystemIsolationSpec::new(
        vec![absolute("/workspace/secret")],
        vec![absolute("/workspace/secret/output")],
        vec![absolute("/workspace/secret")],
    )
    .expect("filesystem spec must be valid");

    assert_eq!(
        spec,
        FileSystemIsolationSpec::new(Vec::new(), Vec::new(), vec![absolute("/workspace/secret")],)
            .expect("expected filesystem spec must be valid")
    );
}

#[test]
fn request_rejects_oversized_command_without_truncating() {
    let error = SandboxExecutionCommand::new(
        absolute("/bin/echo"),
        vec!["argument".to_string(); MAX_ARGUMENT_COUNT + 1],
        BTreeMap::new(),
        absolute("/tmp"),
        SandboxStdin::Closed,
    )
    .expect_err("oversized args must fail");

    assert_eq!(
        error,
        SandboxProviderError::InvalidIsolationSpec {
            reason: format!("argument count exceeds {MAX_ARGUMENT_COUNT}"),
        }
    );
}

#[test]
fn command_rejects_environment_and_stdin_over_limits() {
    let oversized_environment = BTreeMap::from([(
        "KEY".to_string(),
        "v".repeat(MAX_ENVIRONMENT_VALUE_BYTES + 1),
    )]);
    assert_eq!(
        SandboxExecutionCommand::new(
            absolute("/bin/echo"),
            Vec::new(),
            oversized_environment,
            absolute("/tmp"),
            SandboxStdin::Closed,
        ),
        Err(SandboxProviderError::InvalidIsolationSpec {
            reason: "invalid environment value for key: KEY".to_string(),
        })
    );
    assert_eq!(
        SandboxExecutionCommand::new(
            absolute("/bin/echo"),
            Vec::new(),
            BTreeMap::new(),
            absolute("/tmp"),
            SandboxStdin::Bytes(vec![0; MAX_INLINE_STDIN_BYTES + 1]),
        ),
        Err(SandboxProviderError::InvalidIsolationSpec {
            reason: format!("stdin bytes exceed {MAX_INLINE_STDIN_BYTES}"),
        })
    );
}

#[test]
fn filesystem_spec_rejects_root_count_over_limit() {
    let roots = (0..=MAX_ROOT_COUNT)
        .map(|index| absolute(&format!("/workspace/{index}")))
        .collect();

    assert_eq!(
        FileSystemIsolationSpec::new(roots, Vec::new(), Vec::new()),
        Err(SandboxProviderError::InvalidIsolationSpec {
            reason: format!("readable root count exceeds {MAX_ROOT_COUNT}"),
        })
    );
}

#[test]
fn request_preserves_validated_objects() {
    let file_system = FileSystemIsolationSpec::new(
        vec![absolute("/workspace")],
        vec![absolute("/workspace")],
        vec![absolute("/workspace/.git")],
    )
    .expect("filesystem spec must be valid");
    let request = SandboxExecutionRequest::new(
        SandboxProviderPlatform::MacosSeatbelt,
        command(),
        file_system.clone(),
        NetworkIsolationSpec::Restricted,
        SandboxRuntimeRequirements { private_tmp: true },
    )
    .expect("request must be valid");

    assert_eq!(request.platform(), SandboxProviderPlatform::MacosSeatbelt);
    assert_eq!(request.command(), &command());
    assert_eq!(request.file_system(), &file_system);
    assert_eq!(request.network(), NetworkIsolationSpec::Restricted);
    assert_eq!(
        request.runtime(),
        &SandboxRuntimeRequirements { private_tmp: true }
    );
}

#[derive(Debug)]
struct FakeProvider;

impl SandboxProvider for FakeProvider {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn supports_platform(&self, platform: SandboxProviderPlatform) -> bool {
        platform == SandboxProviderPlatform::LinuxBubblewrap
    }

    fn probe(
        &self,
        platform: SandboxProviderPlatform,
    ) -> Result<SandboxProviderCapabilities, SandboxProviderError> {
        if !self.supports_platform(platform) {
            return Err(SandboxProviderError::ProbeFailed {
                reason: "unexpected platform".to_string(),
            });
        }
        Ok(SandboxProviderCapabilities::all(platform, false))
    }

    fn support(
        &self,
        request: &SandboxExecutionRequest,
        snapshot: &SandboxProviderSnapshot,
    ) -> SandboxRequestSupport {
        default_request_support(request, snapshot)
    }

    fn prepare(
        &self,
        request: SandboxExecutionRequest,
    ) -> Result<PreparedSandboxCommand, SandboxProviderError> {
        Ok(PreparedSandboxCommand::from_command(request.into_command()))
    }
}

#[test]
fn fake_provider_satisfies_shared_contract() {
    provider_contract::assert_provider_contract(
        &FakeProvider,
        SandboxProviderPlatform::LinuxBubblewrap,
        ProviderContractExpectation::Supported,
    );
}

#[test]
fn failed_snapshot_is_never_supported() {
    let request = SandboxExecutionRequest::new(
        SandboxProviderPlatform::LinuxBubblewrap,
        command(),
        FileSystemIsolationSpec::new(Vec::new(), Vec::new(), Vec::new())
            .expect("empty roots are valid"),
        NetworkIsolationSpec::Enabled,
        SandboxRuntimeRequirements { private_tmp: false },
    )
    .expect("request must be valid");
    let snapshot = SandboxProviderSnapshot::Failed {
        reason: "probe failed".to_string(),
    };

    assert_eq!(
        default_request_support(&request, &snapshot),
        SandboxRequestSupport::Unavailable {
            reason: "probe failed".to_string(),
        }
    );
}

#[test]
fn support_reports_the_first_missing_required_capability() {
    let request = SandboxExecutionRequest::new(
        SandboxProviderPlatform::LinuxBubblewrap,
        command(),
        FileSystemIsolationSpec::new(Vec::new(), Vec::new(), Vec::new())
            .expect("empty roots are valid"),
        NetworkIsolationSpec::Restricted,
        SandboxRuntimeRequirements { private_tmp: false },
    )
    .expect("request must be valid");
    let capabilities = SandboxProviderCapabilities {
        platform: SandboxProviderPlatform::LinuxBubblewrap,
        file_system: FileSystemIsolationCapabilities {
            read: true,
            write: true,
            deny_read: true,
        },
        network: NetworkIsolationCapabilities { restricted: false },
        private_tmp: true,
        helper_required: false,
    };

    assert_eq!(
        default_request_support(&request, &capabilities.into()),
        SandboxRequestSupport::Unsupported {
            capability: SandboxIsolationCapability::NetworkRestricted,
            reason: "provider does not support NetworkRestricted".to_string(),
        }
    );
}
