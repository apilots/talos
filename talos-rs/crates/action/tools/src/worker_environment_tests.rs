use super::*;

#[cfg(unix)]
#[test]
fn merges_in_precedence_order_and_deduplicates_exact_entries() {
    let environment = WorkerToolEnvironment::assemble(
        &["/configured".into(), "/shared".into(), "/bad:entry".into()],
        Some(OsStr::new("/shared::relative:/discovered:/shared/")),
        Some(OsStr::new("/inherited:/discovered:.:/shared")),
    );
    assert_eq!(
        environment.search_path(&[]).as_str(),
        "/configured:/shared:/discovered:/shared/:/inherited"
    );
}

#[cfg(unix)]
#[test]
fn skips_broken_and_non_executable_candidates_and_disallows_relative_programs() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let root = tempfile::tempdir().unwrap();
    let directories: Vec<_> = ["first", "second", "third"]
        .map(|name| {
            let path = root.path().join(name);
            fs::create_dir(&path).unwrap();
            path
        })
        .to_vec();
    symlink("missing", directories[0].join("tool")).unwrap();
    fs::write(directories[1].join("tool"), "not executable").unwrap();
    let executable = directories[2].join("tool");
    fs::write(&executable, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let environment = WorkerToolEnvironment::assemble(&directories, None, None).search_path(&[]);
    assert_eq!(environment.resolve("tool"), Some(executable.clone()));
    assert_eq!(
        environment.resolve(executable.to_str().unwrap()),
        Some(executable)
    );
    assert_eq!(environment.resolve("./tool"), None);
    assert_eq!(environment.resolve("bin/tool"), None);
    // The candidate is owned by this account. The other execute bit cannot
    // authorize it; root is the exception.
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        fs::set_permissions(
            directories[1].join("tool"),
            fs::Permissions::from_mode(0o001),
        )
        .unwrap();
        assert_eq!(
            environment.resolve("tool"),
            Some(directories[2].join("tool"))
        );
    }
    fs::set_permissions(
        directories[1].join("tool"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert_eq!(
        environment.resolve(directories[1].join("tool").to_str().unwrap()),
        None
    );
}

#[cfg(unix)]
#[test]
fn no_usable_paths_never_searches_the_working_directory_or_default_paths() {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("worker-fixture");
    use std::os::unix::fs::PermissionsExt;
    fs::write(&executable, "#!/bin/sh\nexit 42\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let environment = WorkerToolEnvironment::assemble(&[], None, Some(OsStr::new(":.:relative")))
        .search_path(&[]);
    assert_eq!(environment.resolve("sh"), None);
    let status = std::process::Command::new("/bin/sh")
        .current_dir(root.path())
        .env_clear()
        .env("PATH", environment.as_str())
        .args(["-c", "command -v worker-fixture >/dev/null && exit 42; command -v ls >/dev/null && exit 43; exit 0"])
        .status()
        .unwrap();
    assert!(status.success());
}
