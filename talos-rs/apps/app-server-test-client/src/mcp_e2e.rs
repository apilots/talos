use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use talos_sdk::{
    AppServerClient, MarketplaceSourceInput, PluginDocumentMediaType, PluginDocumentRole,
    PluginInstallConflictPolicy, PluginInstallRequest, PluginInstallTarget, PluginKind,
    PluginListRequest, PluginListStatusFilter, PluginReadRequest, PluginSearchRefreshPolicy,
    PluginSearchRequest, PluginSearchStatusFilter, PluginSourceAddRequest,
    PluginSourceRefreshRequest, PluginStatus, PluginUninstallRequest, ToolExecuteParams,
};

const SERVER_NAME: &str = "playwright";
const HACKER_NEWS_URL: &str = "https://news.ycombinator.com/news";
const NAVIGATE_TOOL: &str = "mcp__playwright__browser_navigate";
const SNAPSHOT_TOOL: &str = "mcp__playwright__browser_snapshot";
const EVALUATE_TOOL: &str = "mcp__playwright__browser_evaluate";
const SCREENSHOT_TOOL: &str = "mcp__playwright__browser_take_screenshot";

pub(super) async fn mcp_e2e(client: &mut AppServerClient) -> Result<()> {
    plugin_lifecycle_e2e(client).await?;
    playwright_mcp_tool_e2e(client).await?;
    Ok(())
}

async fn plugin_lifecycle_e2e(client: &mut AppServerClient) -> Result<()> {
    eprintln!("mcp-e2e plugin source add");
    let source_repo = plugin_marketplace_fixture_path()?;
    let added = client
        .add_plugin_source(PluginSourceAddRequest {
            source: MarketplaceSourceInput::LocalFolder {
                path: source_repo,
                sparse_paths: vec!["mcp".to_string()],
            },
            refresh: false,
        })
        .await?;

    eprintln!("mcp-e2e plugin source refresh");
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

    eprintln!("mcp-e2e plugin search");
    let search = client
        .search_plugins(PluginSearchRequest {
            query: Some("playwright".to_string()),
            source_id: Some(added.source.id.clone()),
            kind: Some(PluginKind::McpServer),
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
        .find(|plugin| plugin.metadata.name == SERVER_NAME)
        .ok_or_else(|| anyhow!("playwright MCP plugin was not found by search"))?;
    if plugin.kind != PluginKind::McpServer {
        return Err(anyhow!(
            "playwright plugin had wrong kind: {:?}",
            plugin.kind
        ));
    }
    if plugin.metadata.display_name.as_deref() != Some("Microsoft Playwright MCP")
        || plugin.metadata.icon.as_deref() != Some("browser")
        || plugin.metadata.description != "Browser automation MCP server used by Talos E2E."
    {
        return Err(anyhow!(
            "playwright MCP plugin metadata mismatch: {:?}",
            plugin.metadata
        ));
    }
    if !plugin.documents.iter().any(|document| {
        document.role == PluginDocumentRole::Readme
            && document.path == "mcp/playwright/README.md"
            && document.media_type == PluginDocumentMediaType::Markdown
    }) || !plugin.documents.iter().any(|document| {
        document.role == PluginDocumentRole::Manifest
            && document.path == "mcp/playwright/talos-mcp.json"
            && document.media_type == PluginDocumentMediaType::Json
    }) {
        return Err(anyhow!(
            "playwright MCP plugin document refs mismatch: {:?}",
            plugin.documents
        ));
    }

    eprintln!("mcp-e2e plugin read");
    let read = client
        .read_plugin(PluginReadRequest {
            plugin_id: plugin.id.clone(),
        })
        .await?;
    if read.plugin.metadata.name != SERVER_NAME {
        return Err(anyhow!(
            "plugin.read returned wrong plugin metadata: {:?}",
            read.plugin.metadata
        ));
    }
    if read.documents.len() != 2
        || read.documents[0].role != PluginDocumentRole::Readme
        || read.documents[0].media_type != PluginDocumentMediaType::Markdown
        || !read.documents[0]
            .contents
            .contains("Microsoft Playwright MCP")
        || read.documents[1].role != PluginDocumentRole::Manifest
        || read.documents[1].media_type != PluginDocumentMediaType::Json
        || !read.documents[1]
            .contents
            .contains("\"name\": \"playwright\"")
    {
        return Err(anyhow!(
            "plugin.read returned wrong MCP documents: {:?}",
            read.documents
        ));
    }

    eprintln!("mcp-e2e plugin install");
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
    if installed.install.exposed_mcp_servers.len() != 1 {
        return Err(anyhow!(
            "installed MCP plugin exposed wrong servers: {:?}",
            installed.install.exposed_mcp_servers
        ));
    }

    eprintln!("mcp-e2e plugin list installed");
    let listed = client
        .list_plugins(PluginListRequest {
            kind: Some(PluginKind::McpServer),
            status: Some(PluginListStatusFilter::Installed),
            source_id: Some(added.source.id),
            include_disabled: true,
        })
        .await?;
    if !listed
        .plugins
        .iter()
        .any(|plugin| plugin.metadata.name == SERVER_NAME)
    {
        return Err(anyhow!("installed playwright MCP plugin was not listed"));
    }

    eprintln!("mcp-e2e plugin disable");
    let disabled = client
        .disable_plugin(talos_sdk::PluginDisableRequest {
            install_id: installed.install.id.clone(),
        })
        .await?;
    if !matches!(
        disabled.plugin.status,
        PluginStatus::Installed { enabled: false, .. }
    ) {
        return Err(anyhow!(
            "MCP plugin had wrong status after disable: {:?}",
            disabled.plugin.status
        ));
    }

    eprintln!("mcp-e2e plugin enable");
    let enabled = client
        .enable_plugin(talos_sdk::PluginEnableRequest {
            install_id: installed.install.id.clone(),
        })
        .await?;
    if !matches!(
        enabled.plugin.status,
        PluginStatus::Installed { enabled: true, .. }
    ) {
        return Err(anyhow!(
            "MCP plugin had wrong status after enable: {:?}",
            enabled.plugin.status
        ));
    }

    eprintln!("mcp-e2e plugin uninstall");
    let uninstalled = client
        .uninstall_plugin(PluginUninstallRequest {
            install_id: installed.install.id,
        })
        .await?;
    if !uninstalled.uninstalled {
        return Err(anyhow!("MCP plugin uninstall did not report success"));
    }
    Ok(())
}

async fn playwright_mcp_tool_e2e(client: &mut AppServerClient) -> Result<()> {
    eprintln!("mcp-e2e tool.list");
    let tools = client.list_tools().await?;
    ensure_tool_registered(&tools, NAVIGATE_TOOL)?;
    ensure_tool_registered(&tools, SNAPSHOT_TOOL)?;
    ensure_tool_registered(&tools, EVALUATE_TOOL)?;
    ensure_tool_registered(&tools, SCREENSHOT_TOOL)?;

    let session_id = format!("mcp-e2e-{}", uuid::Uuid::new_v4().simple());
    let mut called_tools = BTreeSet::new();

    eprintln!("mcp-e2e tool.execute navigate");
    execute_mcp_tool(
        client,
        &session_id,
        NAVIGATE_TOOL,
        serde_json::json!({ "url": HACKER_NEWS_URL }),
    )
    .await?;
    called_tools.insert(NAVIGATE_TOOL.to_string());

    eprintln!("mcp-e2e tool.execute snapshot");
    let snapshot = execute_mcp_tool(client, &session_id, SNAPSHOT_TOOL, serde_json::json!({}))
        .await
        .context("read Hacker News page snapshot through MCP")?;
    called_tools.insert(SNAPSHOT_TOOL.to_string());
    let snapshot_stories = extract_hacker_news_stories(&snapshot);
    if snapshot_stories.len() < 5 {
        return Err(anyhow!(
            "snapshot should expose at least five Hacker News stories, got {snapshot_stories:?}"
        ));
    }

    eprintln!("mcp-e2e tool.execute evaluate");
    let evaluated = execute_mcp_tool(
        client,
        &session_id,
        EVALUATE_TOOL,
        serde_json::json!({
            "function": "() => Array.from(document.querySelectorAll('.athing .titleline a')).slice(0, 10).map((a) => a.textContent || '').filter(Boolean).join('\\n')"
        }),
    )
    .await
    .context("evaluate Hacker News story titles through MCP")?;
    called_tools.insert(EVALUATE_TOOL.to_string());
    let stories = extract_plain_story_lines(&evaluated);
    if stories.len() < 5 {
        return Err(anyhow!(
            "evaluate should return at least five Hacker News stories, got {stories:?}"
        ));
    }

    eprintln!("mcp-e2e tool.execute screenshot");
    let screenshot_result = execute_mcp_tool_response(
        client,
        &session_id,
        SCREENSHOT_TOOL,
        serde_json::json!({
            "filename": "hacker-news.png",
            "fullPage": true
        }),
    )
    .await?;
    called_tools.insert(SCREENSHOT_TOOL.to_string());

    write_screenshot_from_result(&screenshot_result)?;
    wait_for_screenshot_artifact().await?;
    write_report_artifact(&stories, &called_tools)?;
    Ok(())
}

pub(super) fn prepare_mcp_fixture() -> Result<()> {
    let output_dir = output_dir()?;
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("create MCP E2E output dir {}", output_dir.display()))?;
    clean_output_file(&output_dir, "hacker-news.png")?;
    clean_output_file(&output_dir, "hacker-news-report.md")?;
    clean_playwright_snapshots(&output_dir)?;
    clean_file(&std::env::current_dir()?.join("hacker-news.png"))?;

    let browser_path = browser_path()?;
    if !browser_path.exists() {
        return Err(anyhow!(
            "set TALOS_PLAYWRIGHT_MCP_BROWSER to a Chrome/Chromium executable; missing {}",
            browser_path.display()
        ));
    }
    prepare_plugin_marketplace_fixture(&browser_path, &output_dir)?;
    Ok(())
}

fn ensure_tool_registered(tools: &[serde_json::Value], name: &str) -> Result<()> {
    if tools
        .iter()
        .any(|tool| tool.get("name").and_then(|value| value.as_str()) == Some(name))
    {
        return Ok(());
    }
    let names = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(|value| value.as_str()))
        .collect::<Vec<_>>();
    Err(anyhow!(
        "tool.list did not include `{name}`; tools={names:?}"
    ))
}

async fn execute_mcp_tool(
    client: &mut AppServerClient,
    session_id: &str,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<String> {
    let result = execute_mcp_tool_response(client, session_id, tool_name, arguments).await?;
    extract_success_text(tool_name, result)
}

async fn execute_mcp_tool_response(
    client: &mut AppServerClient,
    session_id: &str,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value> {
    let response = client
        .execute_tool(ToolExecuteParams {
            tool_name: tool_name.to_string(),
            arguments: arguments.into(),
            session_id: Some(session_id.to_string()),
        })
        .await
        .with_context(|| format!("execute {tool_name}"))?;
    let result: serde_json::Value = response.result.into();
    if response.status != "executed" {
        return Err(anyhow!(
            "{tool_name} returned non-executed status: {} {:?}",
            response.status,
            result
        ));
    }
    if result.get("status").and_then(|value| value.as_str()) != Some("success") {
        return Err(anyhow!(
            "{tool_name} returned non-success result: {:?}",
            result
        ));
    }
    Ok(result)
}

fn extract_success_text(_tool_name: &str, result: serde_json::Value) -> Result<String> {
    let text = result
        .get("content_items")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("text").and_then(|value| value.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    if text.trim().is_empty() {
        return Ok(result
            .get("compact_summary")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string());
    }
    Ok(text)
}

fn write_screenshot_from_result(result: &serde_json::Value) -> Result<()> {
    if let Some(image) = result
        .get("content_items")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .find_map(|item| {
            let value = item.get("value")?;
            if value.get("type").and_then(|item| item.as_str()) == Some("image") {
                value.get("data_base64").and_then(|item| item.as_str())
            } else {
                None
            }
        })
    {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(image)
            .context("decode screenshot image content")?;
        let screenshot_path = output_dir()?.join("hacker-news.png");
        return std::fs::write(&screenshot_path, bytes)
            .with_context(|| format!("write screenshot artifact {}", screenshot_path.display()));
    }

    let text = result_text(result);
    let relative_path = screenshot_relative_path(&text).ok_or_else(|| {
        anyhow!("screenshot result did not include image content or path: {result:?}")
    })?;
    let source_path = std::env::current_dir()?.join(relative_path.trim_start_matches("./"));
    let screenshot_path = output_dir()?.join("hacker-news.png");
    std::fs::copy(&source_path, &screenshot_path).with_context(|| {
        format!(
            "copy screenshot artifact from {} to {}",
            source_path.display(),
            screenshot_path.display()
        )
    })?;
    Ok(())
}

fn result_text(result: &serde_json::Value) -> String {
    result
        .get("content_items")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("text").and_then(|value| value.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn screenshot_relative_path(text: &str) -> Option<String> {
    let start = text.find("](")? + "](".len();
    let rest = &text[start..];
    let end = rest.find(')')?;
    Some(rest[..end].to_string())
}

async fn wait_for_screenshot_artifact() -> Result<()> {
    let screenshot_path = output_dir()?.join("hacker-news.png");
    for _ in 0..20 {
        if screenshot_path.exists() {
            assert_non_empty_file(&screenshot_path)?;
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    assert_non_empty_file(&screenshot_path)
}

fn assert_non_empty_file(path: &Path) -> Result<()> {
    let metadata =
        std::fs::metadata(path).with_context(|| format!("expected artifact {}", path.display()))?;
    if metadata.len() == 0 {
        return Err(anyhow!("artifact was empty: {}", path.display()));
    }
    Ok(())
}

fn write_report_artifact(stories: &[String], called_tools: &BTreeSet<String>) -> Result<()> {
    let report_path = output_dir()?.join("hacker-news-report.md");
    let generated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time should be after unix epoch")?
        .as_secs();
    let story_lines = stories
        .iter()
        .take(10)
        .enumerate()
        .map(|(index, story)| format!("{}. {story}", index + 1))
        .collect::<Vec<_>>()
        .join("\n");
    let topic_lines = topic_summary(stories)
        .into_iter()
        .map(|(topic, count)| format!("- {topic}: {count}"))
        .collect::<Vec<_>>()
        .join("\n");
    let tools = called_tools
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n- ");
    let report = format!(
        "# Playwright MCP Hacker News E2E Report\n\n\
         Generated at Unix timestamp: {generated_at}\n\n\
         Source: {HACKER_NEWS_URL}\n\n\
         ## Verified Path\n\n\
         - app-server loaded configured Playwright MCP during runtime startup.\n\
         - `tool.list` exposed browser MCP tools.\n\
         - `tool.execute` triggered real MCP browser tool calls through app-server.\n\
         - Playwright wrote `hacker-news.png` under the scenario artifact directory.\n\n\
         ## Tools Called\n\n\
         - {tools}\n\n\
         ## Assistant Report\n\n\
         MCP Demo: playwright\n\n\
         Source: Hacker News\n\n\
         Top Stories:\n\n\
         {story_lines}\n\n\
         Topic Analysis:\n\n\
         {topic_lines}\n\n\
         Check: playwright-mcp-called\n"
    );
    std::fs::write(&report_path, report)
        .with_context(|| format!("write MCP E2E report {}", report_path.display()))
}

fn extract_hacker_news_stories(text: &str) -> Vec<String> {
    let mut stories = Vec::new();
    for line in text.lines() {
        let Some(title) = ranked_story_title(line) else {
            continue;
        };
        if is_story_title(&title) && !stories.contains(&title) {
            stories.push(title);
        }
        if stories.len() >= 10 {
            break;
        }
    }
    stories
}

fn extract_plain_story_lines(text: &str) -> Vec<String> {
    let mut stories = Vec::new();
    for line in expanded_result_lines(text) {
        let title = trim_hacker_news_domain(line.trim()).to_string();
        if is_story_title(&title) && !is_domain_only(&title) && !stories.contains(&title) {
            stories.push(title);
        }
        if stories.len() >= 10 {
            break;
        }
    }
    stories
}

fn expanded_result_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("### ") {
            continue;
        }
        if let Ok(decoded) = serde_json::from_str::<String>(trimmed) {
            lines.extend(decoded.lines().map(str::to_string));
        } else {
            lines.push(trimmed.to_string());
        }
    }
    lines
}

fn ranked_story_title(line: &str) -> Option<String> {
    let trimmed = line.trim().trim_matches('"');
    if let Some(start) = trimmed.find(". upvote ") {
        return Some(trim_hacker_news_domain(&trimmed[start + ". upvote ".len()..]).to_string());
    }
    None
}

fn trim_hacker_news_domain(title: &str) -> &str {
    let Some((story_title, domain)) = title.rsplit_once(" (") else {
        return title.trim();
    };
    let domain = domain.trim_end_matches(')');
    if domain.contains('.') || domain == "github" {
        story_title.trim()
    } else {
        title.trim()
    }
}

fn is_domain_only(title: &str) -> bool {
    let trimmed = title.trim();
    trimmed.contains('.') && !trimmed.chars().any(char::is_whitespace)
}

fn is_story_title(title: &str) -> bool {
    let normalized = title.trim().to_ascii_lowercase();
    title.len() >= 8
        && !matches!(
            normalized.as_str(),
            "hacker news"
                | "new"
                | "past"
                | "comments"
                | "ask"
                | "show"
                | "jobs"
                | "submit"
                | "login"
                | "more"
                | "guidelines"
                | "faq"
                | "lists"
                | "api"
                | "security"
                | "legal"
                | "apply to yc"
        )
}

fn topic_summary(stories: &[String]) -> Vec<(&'static str, usize)> {
    let mut counts = std::collections::BTreeMap::<&'static str, usize>::new();
    for story in stories {
        *counts.entry(infer_topic(story)).or_default() += 1;
    }
    counts.into_iter().collect()
}

fn infer_topic(story: &str) -> &'static str {
    let lower = story.to_ascii_lowercase();
    if contains_any(&lower, &["ai", "llm", "model", "agent", "openai", "claude"]) {
        "AI and model tooling"
    } else if contains_any(
        &lower,
        &[
            "rust",
            "python",
            "javascript",
            "typescript",
            "compiler",
            "language",
        ],
    ) {
        "Programming languages and developer tooling"
    } else if contains_any(
        &lower,
        &["database", "postgres", "sqlite", "sql", "storage"],
    ) {
        "Data systems and infrastructure"
    } else if contains_any(
        &lower,
        &["linux", "kernel", "browser", "network", "cpu", "memory"],
    ) {
        "Systems and platform engineering"
    } else if contains_any(&lower, &["startup", "company", "funding", "yc", "business"]) {
        "Startups and business"
    } else if contains_any(&lower, &["security", "vulnerability", "attack", "cve"]) {
        "Security"
    } else {
        "General engineering and technology"
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn prepare_plugin_marketplace_fixture(browser_path: &Path, output_dir: &Path) -> Result<()> {
    let mcp_dir = plugin_marketplace_fixture_path()?
        .join("mcp")
        .join(SERVER_NAME);
    std::fs::create_dir_all(&mcp_dir)
        .with_context(|| format!("create MCP plugin fixture {}", mcp_dir.display()))?;
    std::fs::write(
        mcp_dir.join("talos-mcp.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "name": SERVER_NAME,
            "display_name": "Microsoft Playwright MCP",
            "description": "Browser automation MCP server used by Talos E2E.",
            "icon": "browser",
            "transport": {
                "type": "stdio",
                "command": "npx",
                "args": playwright_args(browser_path, output_dir),
            }
        }))?,
    )
    .with_context(|| format!("write MCP plugin fixture {}", mcp_dir.display()))?;
    std::fs::write(
        mcp_dir.join("README.md"),
        "# Microsoft Playwright MCP\n\nBrowser automation MCP server used by Talos E2E.\n",
    )
    .with_context(|| format!("write MCP plugin README {}", mcp_dir.display()))
}

fn playwright_args(browser_path: &Path, output_dir: &Path) -> Vec<String> {
    vec![
        "-y".to_string(),
        "@playwright/mcp@latest".to_string(),
        "--headless".to_string(),
        "--isolated".to_string(),
        "--viewport-size".to_string(),
        "1280x900".to_string(),
        "--executable-path".to_string(),
        browser_path.to_string_lossy().to_string(),
        "--output-dir".to_string(),
        output_dir.to_string_lossy().to_string(),
    ]
}

fn plugin_marketplace_fixture_path() -> Result<PathBuf> {
    Ok(talos_home()?.join("plugin-marketplace-fixture"))
}

fn output_dir() -> Result<PathBuf> {
    std::env::var_os("TALOS_PLAYWRIGHT_MCP_E2E_OUTPUT")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("TALOS_PLAYWRIGHT_MCP_E2E_OUTPUT is required"))
}

fn browser_path() -> Result<PathBuf> {
    Ok(std::env::var_os("TALOS_PLAYWRIGHT_MCP_BROWSER")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
        }))
}

fn talos_home() -> Result<PathBuf> {
    std::env::var_os("TALOS_HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("TALOS_HOME is required"))
}

fn clean_output_file(output_dir: &Path, name: &str) -> Result<()> {
    clean_file(&output_dir.join(name))
}

fn clean_playwright_snapshots(output_dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(output_dir)
        .with_context(|| format!("read MCP E2E output dir {}", output_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.starts_with("page-") && name.ends_with(".yml") {
            clean_file(&path)?;
        }
    }
    Ok(())
}

fn clean_file(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("remove stale MCP E2E artifact {}", path.display()))?;
    }
    Ok(())
}
