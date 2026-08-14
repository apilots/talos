use std::collections::BTreeMap;

use seccompiler::{
    apply_filter, BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition,
    SeccompFilter, SeccompRule, TargetArch,
};

pub(crate) fn apply_restricted_network_filter() -> Result<(), String> {
    set_no_new_privs()?;
    let program = restricted_network_filter()?;
    apply_filter(&program).map_err(|error| format!("failed to apply seccomp filter: {error}"))
}

fn set_no_new_privs() -> Result<(), String> {
    // SAFETY: prctl is called with the documented PR_SET_NO_NEW_PRIVS arguments.
    let result = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(format!(
            "failed to set no_new_privs: {}",
            std::io::Error::last_os_error()
        ))
    }
}

fn restricted_network_filter() -> Result<BpfProgram, String> {
    let mut rules = BTreeMap::<i64, Vec<SeccompRule>>::new();
    for syscall in [
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
        libc::SYS_connect,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_getpeername,
        libc::SYS_getsockname,
        libc::SYS_shutdown,
        libc::SYS_sendto,
        libc::SYS_sendmmsg,
        libc::SYS_recvmmsg,
        libc::SYS_getsockopt,
        libc::SYS_setsockopt,
    ] {
        rules.insert(syscall, Vec::new());
    }

    let deny_non_unix = SeccompRule::new(vec![SeccompCondition::new(
        0,
        SeccompCmpArgLen::Dword,
        SeccompCmpOp::Ne,
        libc::AF_UNIX as u64,
    )
    .map_err(|error| format!("failed to construct seccomp condition: {error}"))?])
    .map_err(|error| format!("failed to construct seccomp rule: {error}"))?;
    rules.insert(libc::SYS_socket, vec![deny_non_unix.clone()]);
    rules.insert(libc::SYS_socketpair, vec![deny_non_unix]);

    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        target_architecture()?,
    )
    .map_err(|error| format!("failed to construct seccomp filter: {error}"))?;
    filter
        .try_into()
        .map_err(|error| format!("failed to compile seccomp filter: {error}"))
}

#[cfg(target_arch = "x86_64")]
fn target_architecture() -> Result<TargetArch, String> {
    Ok(TargetArch::x86_64)
}

#[cfg(target_arch = "aarch64")]
fn target_architecture() -> Result<TargetArch, String> {
    Ok(TargetArch::aarch64)
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn target_architecture() -> Result<TargetArch, String> {
    Err(format!(
        "unsupported seccomp target architecture: {}",
        std::env::consts::ARCH
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_restricted_filter_for_supported_architecture() {
        assert!(restricted_network_filter().is_ok());
    }
}
