use super::*;

#[tokio::test]
async fn grep_uses_rg_and_returns_search_matches() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    let file = dir.path().join("demo.rs");
    tokio::fs::write(&file, "fn alpha() {}\nfn beta() {}\n")
        .await
        .expect("write should succeed");
    let context = crate::test_support::full_access_process_context(
        crate::ToolContext::new("session", "user").with_project_root(dir.path()),
    );

    let result = run(
        &GrepFilesRequest {
            pattern: "alpha".to_string(),
            include: None,
            glob: Some("*.rs".to_string()),
            path: None,
            r#type: None,
            limit: 10,
            head_limit: None,
            offset: 0,
            output_mode: GrepOutputMode::Content,
            context: 0,
            before_context: None,
            after_context: None,
            multiline: false,
            case_insensitive: false,
            timeout_ms: 10_000,
        },
        &context,
    )
    .await
    .expect("grep should succeed");

    assert_eq!(result.total_matches, 1);
    assert_eq!(result.matches[0].line, 1);
    assert!(result.matches[0].text.contains("alpha"));
}

#[tokio::test]
async fn grep_excludes_dotenv_files_in_full_access_mode() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    let visible = dir.path().join("visible.txt");
    tokio::fs::write(&visible, "canary\n")
        .await
        .expect("visible fixture should write");
    tokio::fs::write(dir.path().join(".env"), "SECRET=canary\n")
        .await
        .expect("env fixture should write");
    tokio::fs::write(dir.path().join(".env.local"), "TOKEN=canary\n")
        .await
        .expect("env variant fixture should write");
    let context = crate::test_support::full_access_process_context(
        crate::ToolContext::new("session", "user").with_project_root(dir.path()),
    );

    let result = run(
        &GrepFilesRequest {
            pattern: "canary".to_string(),
            include: None,
            glob: None,
            path: None,
            r#type: None,
            limit: 10,
            head_limit: None,
            offset: 0,
            output_mode: GrepOutputMode::Content,
            context: 0,
            before_context: None,
            after_context: None,
            multiline: false,
            case_insensitive: false,
            timeout_ms: 10_000,
        },
        &context,
    )
    .await
    .expect("grep should succeed");

    assert_eq!(
        result
            .matches
            .iter()
            .map(|item| item.path.clone())
            .collect::<Vec<_>>(),
        vec![visible.to_string_lossy().into_owned()]
    );
}

#[tokio::test]
async fn grep_rejects_explicit_dotenv_files_in_full_access_mode() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    let context = crate::test_support::full_access_process_context(
        crate::ToolContext::new("session", "user").with_project_root(dir.path()),
    );
    let env_file = dir.path().join(".env");
    let env_variant = dir.path().join(".env.local");
    tokio::fs::write(&env_file, "SECRET=canary\n")
        .await
        .expect("env fixture should write");
    tokio::fs::write(&env_variant, "SECRET=canary\n")
        .await
        .expect("env variant fixture should write");
    let mut files = vec![env_file, env_variant];
    #[cfg(unix)]
    {
        let link = dir.path().join("visible-link");
        std::os::unix::fs::symlink(&files[0], &link).expect("env symlink should create");
        files.push(link);
    }

    for file in files {
        let error = run(
            &GrepFilesRequest {
                pattern: "canary".to_string(),
                include: None,
                glob: None,
                path: Some(file.display().to_string()),
                r#type: None,
                limit: 10,
                head_limit: None,
                offset: 0,
                output_mode: GrepOutputMode::Content,
                context: 0,
                before_context: None,
                after_context: None,
                multiline: false,
                case_insensitive: false,
                timeout_ms: 10_000,
            },
            &context,
        )
        .await
        .expect_err("explicit dotenv search should fail");

        assert_eq!(
            error.diagnostic_message(),
            "searching dotenv files is not allowed"
        );
    }
}

#[tokio::test]
async fn grep_uses_the_parent_directory_as_cwd_for_a_single_file() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    let file = dir.path().join("single.rs");
    tokio::fs::write(&file, "fn needle() {}\n")
        .await
        .expect("write should succeed");
    let context = crate::test_support::full_access_process_context(
        crate::ToolContext::new("session", "user").with_project_root(dir.path()),
    );

    let result = run(
        &GrepFilesRequest {
            pattern: "needle".to_string(),
            include: None,
            glob: None,
            path: Some(file.display().to_string()),
            r#type: None,
            limit: 10,
            head_limit: None,
            offset: 0,
            output_mode: GrepOutputMode::Content,
            context: 0,
            before_context: None,
            after_context: None,
            multiline: false,
            case_insensitive: false,
            timeout_ms: 10_000,
        },
        &context,
    )
    .await
    .expect("single-file grep should succeed");

    assert_eq!(result.total_matches, 1);
    assert!(result.matches[0].text.contains("needle"));
}
