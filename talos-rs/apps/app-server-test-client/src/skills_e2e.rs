use anyhow::{anyhow, Context, Result};
use talos_sdk::{
    AppServerClient, GatewayClient, MarketplaceSourceInput, ModelSelection,
    PluginDocumentMediaType, PluginDocumentRole, PluginInstallConflictPolicy, PluginInstallRequest,
    PluginInstallTarget, PluginKind, PluginReadRequest, PluginSearchRefreshPolicy,
    PluginSearchRequest, PluginSearchStatusFilter, PluginSourceAddRequest,
    PluginSourceRefreshRequest, SkillScope, SkillSelector, SkillSummary, SkillsListRequest,
    SkillsReadRequest, SkillsResolveRequest, SkillsSuggestRequest, StartTurnRequest,
    TalosInputItem,
};

pub(super) async fn skills_e2e(client: &mut AppServerClient) -> Result<()> {
    configured_root_skill_e2e(client).await?;
    plugin_installed_skill_e2e(client).await?;
    Ok(())
}

async fn configured_root_skill_e2e(client: &mut AppServerClient) -> Result<()> {
    eprintln!("skills-e2e configured suggest");
    let suggestions = client
        .suggest_skills(SkillsSuggestRequest {
            query: "localizer".to_string(),
            limit: Some(8),
        })
        .await?;
    let suggestion = suggestions
        .data
        .into_iter()
        .find(|skill| skill.name == "e2e-localizer")
        .ok_or_else(|| anyhow!("e2e-localizer skill was not suggested"))?;
    let listed = client
        .list_skills(SkillsListRequest {
            include_disabled: false,
        })
        .await?;
    let skill = listed
        .data
        .into_iter()
        .find(|skill| skill.path == suggestion.path)
        .ok_or_else(|| anyhow!("e2e-localizer skill was not listed"))?;

    ensure_skill_resolves(client, &skill).await?;
    let output = run_selected_skill_turn(
        client,
        &skill,
        concat!(
            "Use $e2e-localizer. Localize this source text: ",
            "\"The Skill chip opens the Plugin marketplace before the permission prompt appears.\" ",
            "Follow the selected skill output contract."
        ),
        "app-server-skills",
    )
    .await?;
    eprintln!("skills-e2e configured assistant output:\n{output}");
    assert_skill_localizer_output(&output, "e2e-localizer", "localized-by-skill")?;
    Ok(())
}

async fn plugin_installed_skill_e2e(client: &mut AppServerClient) -> Result<()> {
    eprintln!("skills-e2e plugin source add");
    let source_repo = plugin_marketplace_fixture_path()?;
    let added = client
        .add_plugin_source(PluginSourceAddRequest {
            source: MarketplaceSourceInput::LocalFolder {
                path: source_repo,
                sparse_paths: vec!["skills".to_string()],
            },
            refresh: false,
        })
        .await?;

    eprintln!("skills-e2e plugin source refresh");
    let refreshed = client
        .refresh_plugin_source(PluginSourceRefreshRequest {
            source_id: added.source.id.clone(),
        })
        .await?;
    if !refreshed.warnings.is_empty() {
        return Err(anyhow!(
            "plugin.source.refresh returned warnings: {:?}",
            refreshed.warnings
        ));
    }

    eprintln!("skills-e2e plugin search");
    let search = client
        .search_plugins(PluginSearchRequest {
            query: Some("localizer".to_string()),
            source_id: Some(added.source.id),
            kind: Some(PluginKind::Skill),
            status: Some(PluginSearchStatusFilter::NotInstalled),
            refresh: PluginSearchRefreshPolicy::CacheOnly,
            limit: Some(8),
        })
        .await?;
    if !search.warnings.is_empty() {
        return Err(anyhow!(
            "plugin.search returned warnings: {:?}",
            search.warnings
        ));
    }
    let plugin = search
        .plugins
        .into_iter()
        .find(|plugin| plugin.metadata.name == "e2e-plugin-localizer")
        .ok_or_else(|| anyhow!("e2e-plugin-localizer plugin was not found by search"))?;
    if plugin.metadata.description
        != "Localize Talos plugin manager release notes into Simplified Chinese with the E2E glossary."
    {
        return Err(anyhow!(
            "plugin skill metadata mismatch: {:?}",
            plugin.metadata
        ));
    }
    if plugin.documents.len() != 1
        || plugin.documents[0].role != PluginDocumentRole::Primary
        || plugin.documents[0].path != "skills/e2e-plugin-localizer/SKILL.md"
        || plugin.documents[0].media_type != PluginDocumentMediaType::Markdown
    {
        return Err(anyhow!(
            "plugin skill document refs mismatch: {:?}",
            plugin.documents
        ));
    }

    eprintln!("skills-e2e plugin read");
    let plugin_document = client
        .read_plugin(PluginReadRequest {
            plugin_id: plugin.id.clone(),
        })
        .await?;
    if plugin_document.plugin.metadata.name != "e2e-plugin-localizer"
        || plugin_document.documents.len() != 1
        || plugin_document.documents[0].role != PluginDocumentRole::Primary
        || plugin_document.documents[0].media_type != PluginDocumentMediaType::Markdown
        || !plugin_document.documents[0]
            .contents
            .contains("Check: localized-by-plugin-skill")
    {
        return Err(anyhow!(
            "plugin.read returned wrong skill document structure: {:?}",
            plugin_document.documents
        ));
    }

    eprintln!("skills-e2e plugin install");
    let installed = client
        .install_plugin(PluginInstallRequest {
            target: PluginInstallTarget::Plugin {
                plugin_id: plugin.id.clone(),
                expected_source_commit: plugin.source_commit.clone(),
            },
            enable: Some(true),
            conflict_policy: PluginInstallConflictPolicy::Reject,
        })
        .await?;

    eprintln!("skills-e2e plugin skills.list");
    let listed = client
        .list_skills(SkillsListRequest {
            include_disabled: false,
        })
        .await?;
    if !listed.errors.is_empty() {
        return Err(anyhow!("skills.list returned errors: {:?}", listed.errors));
    }
    let skill = listed
        .data
        .into_iter()
        .find(|skill| skill.name == "e2e-plugin-localizer")
        .ok_or_else(|| anyhow!("installed plugin skill was not listed"))?;
    if skill.scope != SkillScope::Plugin {
        return Err(anyhow!(
            "installed plugin skill had wrong scope: {:?}",
            skill.scope
        ));
    }
    if skill.source_plugin.as_deref() != Some(plugin.id.0.as_str())
        || skill.source_install.as_deref() != Some(installed.install.id.0.as_str())
    {
        return Err(anyhow!(
            "installed plugin skill source metadata mismatch: {:?}",
            skill
        ));
    }

    eprintln!("skills-e2e plugin skills.read");
    let document = client
        .read_skill(SkillsReadRequest {
            selector: SkillSelector {
                name: Some(skill.name.clone()),
                path: Some(skill.path.clone()),
            },
        })
        .await?;
    if !document
        .document
        .contents
        .contains("Check: localized-by-plugin-skill")
    {
        return Err(anyhow!(
            "installed plugin skill document did not contain expected contract"
        ));
    }

    ensure_skill_resolves(client, &skill).await?;
    let output = run_selected_skill_turn(
        client,
        &skill,
        concat!(
            "Use $e2e-plugin-localizer. Localize this source text: ",
            "\"Install the Skill chip from the Plugin marketplace before checking the permission prompt.\" ",
            "Follow the selected skill output contract."
        ),
        "app-server-plugin-skill",
    )
    .await?;
    eprintln!("skills-e2e plugin assistant output:\n{output}");
    assert_skill_localizer_output(&output, "e2e-plugin-localizer", "localized-by-plugin-skill")?;
    Ok(())
}

async fn ensure_skill_resolves(client: &mut AppServerClient, skill: &SkillSummary) -> Result<()> {
    eprintln!("skills-e2e resolve {}", skill.name);
    let resolved = client
        .resolve_skills(SkillsResolveRequest {
            text: format!("Use ${}.", skill.name),
            selected: vec![SkillSelector {
                name: Some(skill.name.clone()),
                path: Some(skill.path.clone()),
            }],
        })
        .await?;
    if !resolved.warnings.is_empty() {
        return Err(anyhow!(
            "skills.resolve returned warnings: {:?}",
            resolved.warnings
        ));
    }
    if !resolved
        .data
        .iter()
        .any(|candidate| candidate.path == skill.path)
    {
        return Err(anyhow!("skills.resolve did not include {}", skill.name));
    }
    Ok(())
}

async fn run_selected_skill_turn(
    client: &mut AppServerClient,
    skill: &SkillSummary,
    prompt: &str,
    operation_prefix: &str,
) -> Result<String> {
    eprintln!("skills-e2e turn.start {}", skill.name);
    let session_id = super::new_session_id(client).await?;
    client
        .start_turn(StartTurnRequest {
            session_id: session_id.clone(),
            input: vec![
                TalosInputItem::Text {
                    text: prompt.to_string(),
                },
                TalosInputItem::SkillSelection {
                    name: skill.name.clone(),
                    path: skill.path.to_string_lossy().to_string(),
                },
            ],
            workspace_id: None,
            context: Some(super::test_talos_context()),
            model_selection: super::test_model_ref().map(ModelSelection::model_default),
            project_id: None,
            current_directory: None,
            client_request_id: Some(format!(
                "{}-{}",
                operation_prefix,
                uuid::Uuid::new_v4().simple()
            )),
        })
        .await?;
    eprintln!("skills-e2e event.subscribe {}", skill.name);
    let subscription = super::subscribe_events(client, &session_id).await?;
    eprintln!("skills-e2e collect events {}", skill.name);
    let events = super::collect_until_terminal(client, &subscription.subscription_id).await?;
    super::ensure_turn_event_flow(&events)?;
    Ok(super::assistant_text_from_events(&events))
}

fn assert_skill_localizer_output(output: &str, skill_name: &str, check: &str) -> Result<()> {
    for expected in [
        format!("Skill: {skill_name}"),
        "Translation:".to_string(),
        "技能标签".to_string(),
        "插件市场".to_string(),
        "权限提示".to_string(),
        "Terms:".to_string(),
        "Skill chip=技能标签".to_string(),
        "Plugin marketplace=插件市场".to_string(),
        "permission prompt=权限提示".to_string(),
        format!("Check: {check}"),
    ] {
        if !output.contains(&expected) {
            return Err(anyhow!(
                "skill localizer output missing `{expected}`; output={output:?}"
            ));
        }
    }
    Ok(())
}

pub(super) fn prepare_skills_fixture() -> Result<()> {
    prepare_configured_skill_fixture()?;
    prepare_plugin_marketplace_fixture()?;
    Ok(())
}

fn prepare_configured_skill_fixture() -> Result<()> {
    let talos_home = std::env::var("TALOS_HOME").context("TALOS_HOME is required")?;
    let skill_root = std::path::Path::new(&talos_home)
        .join("agents-skills")
        .join("e2e-localizer");
    std::fs::create_dir_all(&skill_root)
        .with_context(|| format!("create skill fixture {}", skill_root.display()))?;
    write_skill(
        &skill_root,
        r#"---
name: e2e-localizer
description: Localize Talos UI release notes into Simplified Chinese with the E2E glossary.
metadata:
  author: talos-e2e
  version: 1
  argument-hint: source text
---

When this skill is selected, localize the user's source text into Simplified Chinese.
Use this glossary exactly:

- Skill chip = 技能标签
- Plugin marketplace = 插件市场
- permission prompt = 权限提示

Always answer with exactly these four lines:

Skill: e2e-localizer
Translation: <localized Chinese sentence using the glossary terms>
Terms: Skill chip=技能标签; Plugin marketplace=插件市场; permission prompt=权限提示
Check: localized-by-skill

Do not answer with a generic acknowledgement.
"#,
    )
}

fn prepare_plugin_marketplace_fixture() -> Result<()> {
    let source_skill = plugin_marketplace_fixture_path()?
        .join("skills")
        .join("e2e-plugin-localizer");
    std::fs::create_dir_all(&source_skill)
        .with_context(|| format!("create plugin skill fixture {}", source_skill.display()))?;
    write_skill(
        &source_skill,
        r#"---
name: e2e-plugin-localizer
description: Localize Talos plugin manager release notes into Simplified Chinese with the E2E glossary.
metadata:
  author: talos-e2e
  version: 1
  argument-hint: source text
---

When this skill is selected, localize the user's source text into Simplified Chinese.
Use this glossary exactly:

- Skill chip = 技能标签
- Plugin marketplace = 插件市场
- permission prompt = 权限提示

Always answer with exactly these four lines:

Skill: e2e-plugin-localizer
Translation: <localized Chinese sentence using the glossary terms>
Terms: Skill chip=技能标签; Plugin marketplace=插件市场; permission prompt=权限提示
Check: localized-by-plugin-skill

Do not answer with a generic acknowledgement.
"#,
    )
}

fn plugin_marketplace_fixture_path() -> Result<std::path::PathBuf> {
    let talos_home = std::env::var("TALOS_HOME").context("TALOS_HOME is required")?;
    Ok(std::path::Path::new(&talos_home).join("plugin-marketplace-fixture"))
}

fn write_skill(skill_root: &std::path::Path, contents: &str) -> Result<()> {
    std::fs::create_dir_all(skill_root)
        .with_context(|| format!("create skill fixture {}", skill_root.display()))?;
    std::fs::write(skill_root.join("SKILL.md"), contents)
        .with_context(|| format!("write skill fixture {}", skill_root.display()))?;
    Ok(())
}
