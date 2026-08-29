use std::fs;

use super::*;

#[tokio::test]
async fn sqlx_ledger_skips_an_already_applied_migration() {
    let temp = tempfile::tempdir().expect("temp migration directory");
    fs::write(
        temp.path().join("202608290001_create_upgrade_probe.sql"),
        "CREATE TABLE upgrade_probe (id INTEGER PRIMARY KEY);",
    )
    .unwrap();
    let pool = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("connect sqlite");

    let first = DatabaseMigrationRunner::new(&pool, temp.path())
        .run()
        .await
        .expect("apply migration");
    let second = DatabaseMigrationRunner::new(&pool, temp.path())
        .run()
        .await
        .expect("skip applied migration");

    assert_eq!(
        (
            first.applied_versions,
            first.previously_applied_count,
            first.total_applied_count,
            second.applied_versions,
            second.previously_applied_count,
            second.total_applied_count,
        ),
        (vec![202608290001], 0, 1, Vec::<i64>::new(), 1, 1)
    );
}
