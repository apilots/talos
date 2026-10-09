use std::fs;

use super::*;

#[test]
fn registration_credential_must_be_a_private_file_outside_workspace() {
    let root = tempfile::tempdir().unwrap();
    let conf = root.path().join("conf");
    let workspace = root.path().join("workspace-talos");
    fs::create_dir_all(&conf).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    let credential = conf.join("node.credential");
    fs::write(&credential, "registration-secret\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&credential, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_credential(&credential, &conf).is_err());
        fs::set_permissions(&credential, fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert_eq!(
        read_credential(&credential, &conf).unwrap(),
        "registration-secret"
    );

    let misplaced = workspace.join("node.credential");
    fs::write(&misplaced, "registration-secret").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&misplaced, fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert!(read_credential(&misplaced, &conf).is_err());
}
