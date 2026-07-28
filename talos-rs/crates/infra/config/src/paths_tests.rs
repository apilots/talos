use super::*;

#[test]
fn ensure_agent_workspace_does_not_migrate_prefix_files() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(temp_dir.path().join(".talos"));
    let workspace_dir = paths.workspace_dir_for_agent("talos");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should be created");
    let memory_file = workspace_dir.join("MEMORY.md");
    std::fs::write(&memory_file, "# Prefix template").expect("prefix file should be written");

    paths
        .ensure_agent_workspace("talos")
        .expect("workspace dirs should be ensured");

    assert!(memory_file.exists());
    assert!(workspace_dir.join("users").join("anonymous").is_dir());
    assert!(!workspace_dir
        .join("users")
        .join("anonymous")
        .join("MEMORY.md")
        .exists());
}

#[test]
fn database_migration_path_uses_the_typed_backend() {
    let paths = TalosPaths::from_home(PathBuf::from("/tmp/talos-path-test"));

    assert_eq!(
        (
            paths.database_migrations_dir(crate::DatabaseType::Sqlite),
            paths.database_migrations_dir(crate::DatabaseType::Postgres),
        ),
        (
            PathBuf::from("/tmp/talos-path-test/conf/migrations/sqlite"),
            PathBuf::from("/tmp/talos-path-test/conf/migrations/postgres"),
        )
    );
}
