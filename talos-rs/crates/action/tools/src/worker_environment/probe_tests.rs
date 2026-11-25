use super::*;

#[test]
fn parser_accepts_noise_but_rejects_missing_duplicate_and_invalid_frames() {
    assert_eq!(
        parse_path(b"startup\n\0talos-worker-path\0/one:/two\0logout\n"),
        Ok(OsString::from("/one:/two"))
    );
    for malformed in [
        &b"PATH=/one"[..],
        b"\0talos-worker-path\0/one",
        b"\0talos-worker-path\0:relative\0",
        b"\0talos-worker-path\0/one\0\0talos-worker-path\0/two\0",
    ] {
        assert_eq!(parse_path(malformed), Err(Unavailable::InvalidPath));
    }
}

#[test]
fn selects_only_supported_account_shells() {
    assert_eq!(
        shell_probe(Path::new("/bin/zsh")),
        Some(("-ilc", ZSH_CAPTURE))
    );
    assert_eq!(shell_probe(Path::new("/bin/bash")), Some(("-lc", CAPTURE)));
    assert_eq!(shell_probe(Path::new("/bin/sh")), Some(("-lc", CAPTURE)));
    assert_eq!(shell_probe(Path::new("/usr/sbin/nologin")), None);
}

fn shell(script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.env_clear().current_dir("/").args(["-c", script]);
    command
}

#[tokio::test]
async fn fixed_capture_works_without_an_executable_printf_on_path() {
    let mut command = shell(CAPTURE);
    command.env("PATH", "/fixture");
    assert_eq!(
        capture(command, TIMEOUT).await,
        Ok(OsString::from("/fixture"))
    );
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn zsh_login_probe_reads_disposable_profile_under_sparse_path() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join(".zprofile"),
        "export PATH='/discovered-fixture:/inherited-fixture'\n",
    )
    .unwrap();
    let mut command = Command::new("/bin/zsh");
    command
        .env_clear()
        .current_dir("/")
        .env("HOME", home.path())
        .env("ZDOTDIR", home.path())
        .env("TERM", "dumb")
        .env("PATH", "/inherited-fixture")
        .args(["-ilc", ZSH_CAPTURE]);
    assert_eq!(
        capture(command, TIMEOUT).await,
        Ok(OsString::from("/discovered-fixture:/inherited-fixture"))
    );
}

#[tokio::test]
async fn bounds_noisy_and_unsuccessful_probes() {
    assert_eq!(
        capture(
            shell("while :; do printf 'noise-noise-noise'; done"),
            TIMEOUT
        )
        .await,
        Err(Unavailable::OutputLimit)
    );
    assert_eq!(
        capture(shell("exit 1"), TIMEOUT).await,
        Err(Unavailable::ShellExit)
    );
}

#[tokio::test]
async fn times_out_and_stops_descendants_that_inherit_the_capture_pipe() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("descendant-survived");
    #[cfg(target_os = "macos")]
    let (binary, flags) = ("/bin/zsh", &["-f", "-ilc"][..]);
    #[cfg(target_os = "linux")]
    let (binary, flags) = ("/bin/bash", &["--noprofile", "--norc", "-lc"][..]);
    let mut command = Command::new(binary);
    command.env_clear().current_dir("/").args(flags).arg(
        "if ( : </dev/tty ) 2>/dev/null; then exit 21; fi; (/bin/sleep 0.7; printf survived > \"$MARKER\") & wait",
    );
    command.env("MARKER", &marker);
    assert_eq!(
        capture(command, Duration::from_millis(150)).await,
        Err(Unavailable::Timeout)
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!marker.exists());
}
