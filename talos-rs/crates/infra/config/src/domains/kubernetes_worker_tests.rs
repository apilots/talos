use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn pasted_kubeconfig_is_private_and_readable_through_its_reference() {
    let temp = tempfile::tempdir().unwrap();
    let paths = TalosPaths::from_home(temp.path().to_path_buf());
    paths.ensure_base_dirs().unwrap();
    let reference = Uuid::new_v4();
    let config = KubernetesConfig {
        kubeconfig_source: Some(KubeconfigSource::Pasted),
        kubeconfig_context: Some("tenant".into()),
        pasted_kubeconfig: Some(reference),
        ..Default::default()
    };
    let contents = "token: fixture-secret";
    write_pasted_kubeconfig(&paths, reference, contents).unwrap();
    let KubernetesCredential::Pasted { path, context } = config.credential(&paths).unwrap() else {
        panic!("expected a private kubeconfig path");
    };
    assert_eq!(context.as_deref(), Some("tenant"));
    assert_eq!(read_pasted_kubeconfig(&path).unwrap(), contents);
    assert_eq!(
        path.parent()
            .unwrap()
            .metadata()
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        read_pasted_kubeconfig(&path).unwrap_err().error_code(),
        ErrorCode::ServiceUnavailable
    );
}
