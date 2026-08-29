use super::*;

#[test]
fn workspace_prefix_is_independent_from_agent_identity() {
    let paths = TalosPaths::from_home(PathBuf::from("/tmp/talos-path-test"));

    assert_eq!(
        paths.workspace_prefix(),
        PathBuf::from("/tmp/talos-path-test/workspace-talos")
    );
    assert_eq!(
        paths.legacy_workspace_dir_for_agent("Research Agent"),
        PathBuf::from("/tmp/talos-path-test/workspace-research-agent")
    );
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
