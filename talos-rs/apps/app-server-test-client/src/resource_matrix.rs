use super::*;

pub(super) async fn resource_matrix(client: &mut AppServerClient) -> Result<()> {
    eprintln!("resource-matrix config/model list");
    list_models(client).await?;
    get_config(client).await?;

    eprintln!("resource-matrix memory roundtrip");
    let memory_date = "2099-12-31";
    let memory_content = memory_e2e::canonical_dream_memory_document(memory_date)?;
    let memory_expected_version = match read_memory_file(client, memory_date).await {
        Ok(existing) => Some(existing.file.version_token),
        Err(error) if error.is_code(talos_sdk::ErrorCode::ResourceNotFound) => None,
        Err(error) => return Err(error.into()),
    };
    let written_memory =
        write_memory_file(client, memory_date, memory_content, memory_expected_version)
            .await?
            .file;
    if !written_memory
        .content
        .contains("app-server resource parity")
    {
        return Err(anyhow!("memory write did not persist expected content"));
    }
    let read_memory = read_memory_file(client, memory_date).await?.file;
    if read_memory.version_token != written_memory.version_token {
        return Err(anyhow!("memory read returned a different version token"));
    }
    expect_conflict(
        write_memory_file(
            client,
            memory_date,
            "stale write".to_string(),
            Some("stale-version-token".to_string()),
        )
        .await,
        "memory stale write should conflict",
    )?;
    expect_validation(
        read_memory_file(client, "not-a-date.md").await,
        "invalid memory date should validate",
    )?;
    list_memory_files(client).await?;

    eprintln!("resource-matrix workspace memory canonical create/read/conflict");
    let mut read_workspace = read_workspace_doc(client, WorkspaceDocKind::Memory)
        .await?
        .doc;
    if !read_workspace.exists {
        let template = memory_e2e::canonical_memory_template()?;
        read_workspace = write_workspace_doc(
            client,
            WorkspaceDocKind::Memory,
            template,
            ExpectedFileVersion::MustNotExist,
        )
        .await?
        .doc;
    }
    memory_e2e::validate_memory_markdown(&read_workspace.content)?;
    expect_conflict(
        write_workspace_doc(
            client,
            WorkspaceDocKind::Memory,
            "blind overwrite".to_string(),
            ExpectedFileVersion::MustNotExist,
        )
        .await,
        "workspace blind overwrite should conflict",
    )?;
    list_workspace_docs(client).await?;

    eprintln!("resource-matrix Computer SDK metadata parity");
    let computers = client.list_computers(Default::default()).await?;
    let embedded = computers
        .data
        .iter()
        .find(|node| node.node_id == "embedded")
        .ok_or_else(|| anyhow!("computer.list omitted embedded Computer"))?;
    let computer = client
        .read_computer(talos_sdk::GetNodeRequest {
            node_id: embedded.node_id.clone(),
        })
        .await?;
    if computer != *embedded {
        return Err(anyhow!(
            "computer.read differed from the canonical list metadata"
        ));
    }

    eprintln!("resource-matrix project roundtrip");
    let project_dir = std::env::temp_dir().join(format!(
        "talos-app-server-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&project_dir)
        .with_context(|| format!("create project test directory {}", project_dir.display()))?;
    let project_dir = project_dir.canonicalize()?;
    let project_name = format!("App Server {}", uuid::Uuid::new_v4().simple());
    let project = client
        .create_project(talos_sdk::ProjectCreateRequest {
            node_id: None,
            name: project_name.clone(),
            directory: project_dir.clone(),
        })
        .await?
        .project;
    let read_project = get_project_by_id(client, &project.project_id)
        .await?
        .project;
    if read_project.directory != project_dir {
        return Err(anyhow!("project.read returned a different directory"));
    }
    let listed_projects = list_all_projects(client).await?.projects;
    if !listed_projects
        .iter()
        .any(|candidate| candidate.project_id == project.project_id)
    {
        return Err(anyhow!("project.list did not include created project"));
    }
    let patched_name = format!("{project_name} patched");
    let patched_project = client
        .patch_project(talos_sdk::ProjectPatchRequest {
            project_id: project.project_id.clone(),
            name: Some(patched_name.clone()),
        })
        .await?
        .project;
    if patched_project.name != patched_name {
        return Err(anyhow!("project.patch did not update name"));
    }
    delete_project_by_id(client, &project.project_id).await?;
    expect_not_found(
        get_project_by_id(client, &project.project_id).await,
        "deleted project should map to not found",
    )?;

    eprintln!("resource-matrix session error mapping");
    let session_id = new_session_id(client).await?;
    start_turn(client, &session_id).await?;
    let listed_sessions = client.list_sessions(Default::default()).await?;
    if !listed_sessions
        .sessions
        .iter()
        .any(|candidate| candidate.id == session_id)
    {
        return Err(anyhow!("session.list did not include created session"));
    }
    let _snapshot = recover_timeline_after(client, &session_id, None).await?;
    expect_not_found(
        get_session_by_id(client, "missing-session").await,
        "missing session should map to not found",
    )?;
    expect_not_found(
        recover_timeline_after(client, "missing-session", None).await,
        "missing timeline recovery should map to not found",
    )?;
    expect_not_found(
        submit_dynamic_tool_result_for_missing_session(
            client,
            "missing-session",
            "result".to_string(),
            true,
        )
        .await,
        "missing dynamic tool result session should map to not found",
    )?;

    Ok(())
}
