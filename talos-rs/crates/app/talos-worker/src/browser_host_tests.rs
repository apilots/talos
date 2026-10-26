use super::*;

#[test]
fn packaged_browser_selects_both_embedded_and_registered_hosts() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("browser")).unwrap();
    std::fs::write(root.path().join("browser/runtime.json"), "{}").unwrap();
    for fallback in [BrowserHostKind::Server, BrowserHostKind::Native] {
        assert_eq!(
            select_host(fallback, Some(root.path())),
            BrowserHostKind::Docker
        );
    }
    assert_eq!(
        select_host(BrowserHostKind::Desktop, Some(root.path())),
        BrowserHostKind::Desktop
    );
}

#[test]
fn unpackaged_hosts_retain_their_browser_policy() {
    let root = tempfile::tempdir().unwrap();
    for fallback in [
        BrowserHostKind::Server,
        BrowserHostKind::Native,
        BrowserHostKind::Desktop,
    ] {
        assert_eq!(select_host(fallback, None), fallback);
        assert_eq!(select_host(fallback, Some(root.path())), fallback);
    }
}
