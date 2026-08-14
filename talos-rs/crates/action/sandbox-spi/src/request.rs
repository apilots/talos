use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{AbsolutePathBuf, SandboxProviderError, SandboxProviderPlatform};

pub const MAX_ROOT_COUNT: usize = 256;
pub const MAX_ARGUMENT_COUNT: usize = 4096;
pub const MAX_ARGUMENT_BYTES: usize = 1024 * 1024;
pub const MAX_ENVIRONMENT_ENTRIES: usize = 256;
pub const MAX_ENVIRONMENT_KEY_BYTES: usize = 256;
pub const MAX_ENVIRONMENT_VALUE_BYTES: usize = 32 * 1024;
pub const MAX_ENVIRONMENT_BYTES: usize = 256 * 1024;
pub const MAX_INLINE_STDIN_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxStdin {
    Closed,
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxExecutionCommand {
    program: AbsolutePathBuf,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: AbsolutePathBuf,
    stdin: SandboxStdin,
}

impl SandboxExecutionCommand {
    pub fn new(
        program: AbsolutePathBuf,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        cwd: AbsolutePathBuf,
        stdin: SandboxStdin,
    ) -> Result<Self, SandboxProviderError> {
        validate_args(&args)?;
        validate_env(&env)?;
        validate_stdin(&stdin)?;
        Ok(Self {
            program,
            args,
            env,
            cwd,
            stdin,
        })
    }

    pub fn program(&self) -> &AbsolutePathBuf {
        &self.program
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    pub fn cwd(&self) -> &AbsolutePathBuf {
        &self.cwd
    }

    pub fn stdin(&self) -> &SandboxStdin {
        &self.stdin
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSystemIsolationSpec {
    readable_roots: Vec<AbsolutePathBuf>,
    writable_roots: Vec<AbsolutePathBuf>,
    denied_roots: Vec<AbsolutePathBuf>,
}

impl FileSystemIsolationSpec {
    pub fn new(
        readable_roots: Vec<AbsolutePathBuf>,
        writable_roots: Vec<AbsolutePathBuf>,
        denied_roots: Vec<AbsolutePathBuf>,
    ) -> Result<Self, SandboxProviderError> {
        validate_root_count("readable", &readable_roots)?;
        validate_root_count("writable", &writable_roots)?;
        validate_root_count("denied", &denied_roots)?;

        let denied_roots = compact_roots(denied_roots);
        let writable_roots = compact_roots(writable_roots)
            .into_iter()
            .filter(|root| !is_contained_by_any(root, &denied_roots))
            .collect::<Vec<_>>();
        let mut readable_roots = readable_roots;
        readable_roots.extend(writable_roots.iter().cloned());
        let readable_roots = compact_roots(readable_roots)
            .into_iter()
            .filter(|root| !is_contained_by_any(root, &denied_roots))
            .collect();

        Ok(Self {
            readable_roots,
            writable_roots,
            denied_roots,
        })
    }

    pub fn readable_roots(&self) -> &[AbsolutePathBuf] {
        &self.readable_roots
    }

    pub fn writable_roots(&self) -> &[AbsolutePathBuf] {
        &self.writable_roots
    }

    pub fn denied_roots(&self) -> &[AbsolutePathBuf] {
        &self.denied_roots
    }

    pub fn access_for(&self, path: &AbsolutePathBuf) -> SandboxPathAccess {
        if is_contained_by_any(path, &self.denied_roots) {
            return SandboxPathAccess::Denied;
        }
        if is_contained_by_any(path, &self.writable_roots) {
            return SandboxPathAccess::Writable;
        }
        if is_contained_by_any(path, &self.readable_roots) {
            return SandboxPathAccess::Readable;
        }
        SandboxPathAccess::Denied
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxPathAccess {
    Denied,
    Readable,
    Writable,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkIsolationSpec {
    Restricted,
    Enabled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxRuntimeRequirements {
    pub private_tmp: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxExecutionRequest {
    platform: SandboxProviderPlatform,
    command: SandboxExecutionCommand,
    file_system: FileSystemIsolationSpec,
    network: NetworkIsolationSpec,
    runtime: SandboxRuntimeRequirements,
}

impl SandboxExecutionRequest {
    pub fn new(
        platform: SandboxProviderPlatform,
        command: SandboxExecutionCommand,
        file_system: FileSystemIsolationSpec,
        network: NetworkIsolationSpec,
        runtime: SandboxRuntimeRequirements,
    ) -> Result<Self, SandboxProviderError> {
        validate_args(command.args())?;
        validate_env(command.env())?;
        validate_stdin(command.stdin())?;
        Ok(Self {
            platform,
            command,
            file_system,
            network,
            runtime,
        })
    }

    pub fn platform(&self) -> SandboxProviderPlatform {
        self.platform
    }

    pub fn command(&self) -> &SandboxExecutionCommand {
        &self.command
    }

    pub fn file_system(&self) -> &FileSystemIsolationSpec {
        &self.file_system
    }

    pub fn network(&self) -> NetworkIsolationSpec {
        self.network
    }

    pub fn runtime(&self) -> &SandboxRuntimeRequirements {
        &self.runtime
    }

    pub fn into_command(self) -> SandboxExecutionCommand {
        self.command
    }
}

fn compact_roots(roots: Vec<AbsolutePathBuf>) -> Vec<AbsolutePathBuf> {
    let roots = roots.into_iter().collect::<BTreeSet<_>>();
    roots
        .iter()
        .filter(|candidate| {
            !roots
                .iter()
                .any(|other| other != *candidate && other.contains(candidate))
        })
        .cloned()
        .collect()
}

fn is_contained_by_any(path: &AbsolutePathBuf, roots: &[AbsolutePathBuf]) -> bool {
    roots.iter().any(|root| root.contains(path))
}

fn validate_root_count(kind: &str, roots: &[AbsolutePathBuf]) -> Result<(), SandboxProviderError> {
    if roots.len() > MAX_ROOT_COUNT {
        return invalid(format!("{kind} root count exceeds {MAX_ROOT_COUNT}"));
    }
    Ok(())
}

fn validate_args(args: &[String]) -> Result<(), SandboxProviderError> {
    if args.len() > MAX_ARGUMENT_COUNT {
        return invalid(format!("argument count exceeds {MAX_ARGUMENT_COUNT}"));
    }
    let bytes = args.iter().map(String::len).sum::<usize>();
    if bytes > MAX_ARGUMENT_BYTES {
        return invalid(format!("argument bytes exceed {MAX_ARGUMENT_BYTES}"));
    }
    if args.iter().any(|arg| arg.contains('\0')) {
        return invalid("arguments must not contain NUL bytes".to_string());
    }
    Ok(())
}

fn validate_env(env: &BTreeMap<String, String>) -> Result<(), SandboxProviderError> {
    if env.len() > MAX_ENVIRONMENT_ENTRIES {
        return invalid(format!(
            "environment entry count exceeds {MAX_ENVIRONMENT_ENTRIES}"
        ));
    }
    let mut total_bytes = 0;
    for (key, value) in env {
        if key.is_empty() || key.len() > MAX_ENVIRONMENT_KEY_BYTES || key.contains(['=', '\0']) {
            return invalid(format!("invalid environment key: {key}"));
        }
        if value.len() > MAX_ENVIRONMENT_VALUE_BYTES || value.contains('\0') {
            return invalid(format!("invalid environment value for key: {key}"));
        }
        total_bytes += key.len() + value.len();
    }
    if total_bytes > MAX_ENVIRONMENT_BYTES {
        return invalid(format!("environment bytes exceed {MAX_ENVIRONMENT_BYTES}"));
    }
    Ok(())
}

fn validate_stdin(stdin: &SandboxStdin) -> Result<(), SandboxProviderError> {
    if let SandboxStdin::Bytes(bytes) = stdin {
        if bytes.len() > MAX_INLINE_STDIN_BYTES {
            return invalid(format!("stdin bytes exceed {MAX_INLINE_STDIN_BYTES}"));
        }
    }
    Ok(())
}

fn invalid<T>(reason: String) -> Result<T, SandboxProviderError> {
    Err(SandboxProviderError::InvalidIsolationSpec { reason })
}
