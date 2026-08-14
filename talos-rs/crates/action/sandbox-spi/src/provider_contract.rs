use std::collections::BTreeMap;
use std::env;

use crate::{
    AbsolutePathBuf, FileSystemIsolationSpec, NetworkIsolationSpec, SandboxExecutionCommand,
    SandboxExecutionRequest, SandboxIsolationCapability, SandboxProvider, SandboxProviderError,
    SandboxProviderPlatform, SandboxProviderSnapshot, SandboxRequestSupport,
    SandboxRuntimeRequirements, SandboxStdin,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderContractExpectation {
    Supported,
    Unsupported {
        capability: SandboxIsolationCapability,
    },
}

pub fn assert_provider_contract(
    provider: &dyn SandboxProvider,
    platform: SandboxProviderPlatform,
    expectation: ProviderContractExpectation,
) {
    assert!(provider.supports_platform(platform));
    assert!(!provider.name().is_empty());

    let capabilities = provider
        .probe(platform)
        .expect("provider contract probe must succeed");
    assert_eq!(capabilities.platform, platform);
    let snapshot = SandboxProviderSnapshot::from(capabilities);
    let request = canonical_request(platform);
    let first = provider.support(&request, &snapshot);
    let second = provider.support(&request, &snapshot);
    assert_eq!(first, second, "support must be deterministic");

    match expectation {
        ProviderContractExpectation::Supported => {
            assert_eq!(first, SandboxRequestSupport::Supported);
            provider
                .prepare(request)
                .expect("supported request must prepare");
        }
        ProviderContractExpectation::Unsupported { capability } => {
            let SandboxRequestSupport::Unsupported {
                capability: actual, ..
            } = first
            else {
                panic!("provider must return unsupported capability");
            };
            assert_eq!(actual, capability);
            assert!(matches!(
                provider.prepare(request),
                Err(SandboxProviderError::UnsupportedCapability {
                    capability: actual,
                    ..
                }) if actual == capability
            ));
        }
    }
}

fn canonical_request(platform: SandboxProviderPlatform) -> SandboxExecutionRequest {
    let program = AbsolutePathBuf::new(env::current_exe().expect("current exe must resolve"))
        .expect("current exe must be absolute");
    let cwd = AbsolutePathBuf::new(env::current_dir().expect("current dir must resolve"))
        .expect("current dir must be absolute");
    let file_system =
        FileSystemIsolationSpec::new(vec![cwd.clone()], vec![cwd.clone()], Vec::new())
            .expect("canonical filesystem spec must be valid");
    let command = SandboxExecutionCommand::new(
        program,
        Vec::new(),
        BTreeMap::new(),
        cwd,
        SandboxStdin::Closed,
    )
    .expect("canonical command must be valid");

    SandboxExecutionRequest::new(
        platform,
        command,
        file_system,
        NetworkIsolationSpec::Restricted,
        SandboxRuntimeRequirements { private_tmp: false },
    )
    .expect("canonical request must be valid")
}
