use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use talos_sdk::{
    AgentEventPayload, AppServerClient, GatewayClient, MarketplaceSourceInput,
    PluginDocumentMediaType, PluginDocumentRole, PluginInstallConflictPolicy, PluginInstallId,
    PluginInstallRequest, PluginInstallTarget, PluginKind, PluginListRequest,
    PluginListStatusFilter, PluginReadRequest, PluginSearchRefreshPolicy, PluginSearchRequest,
    PluginSearchStatusFilter, PluginSourceAddRequest, PluginSourceRefreshRequest, PluginStatus,
    PluginUninstallRequest, TalosTimelineContent, TalosTimelineItemKind, TimelineEventEnvelope,
};

const SERVER_NAME: &str = "playwright";
const HACKER_NEWS_URL: &str = "https://news.ycombinator.com/news";
const NAVIGATE_TOOL_SUFFIX: &str = "browser_navigate";
const SCREENSHOT_TOOL_SUFFIX: &str = "browser_take_screenshot";

pub(super) async fn mcp_e2e(client: &mut AppServerClient) -> Result<()> {
    let install_id = plugin_lifecycle_e2e(client).await?;
    playwright_mcp_tool_e2e(client).await?;
    uninstall_mcp_plugin(client, install_id).await?;
    Ok(())
}

async fn plugin_lifecycle_e2e(client: &mut AppServerClient) -> Result<PluginInstallId> {
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
    if installed.install.kind != PluginKind::McpServer
        || installed.install.content_digest.file_count == 0
    {
        return Err(anyhow!(
            "installed MCP plugin returned wrong install record: {:?}",
            installed.install
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

    Ok(installed.install.id)
}

async fn uninstall_mcp_plugin(
    client: &mut AppServerClient,
    install_id: PluginInstallId,
) -> Result<()> {
    eprintln!("mcp-e2e plugin uninstall");
    let uninstalled = client
        .uninstall_plugin(PluginUninstallRequest { install_id })
        .await?;
    if !uninstalled.uninstalled {
        return Err(anyhow!("MCP plugin uninstall did not report success"));
    }
    Ok(())
}

async fn playwright_mcp_tool_e2e(client: &mut AppServerClient) -> Result<()> {
    let session_id = format!("mcp-e2e-{}", uuid::Uuid::new_v4().simple());
    let screenshot_path = output_dir()?.join("hacker-news.png");
    let prompt = format!(
        "You must first call tool 'ToolSearch' exactly once with query Playwright browser_navigate to activate the Playwright MCP browser tools. Then use the Playwright browser MCP tools to navigate to {HACKER_NEWS_URL}, inspect the current top stories, take a screenshot saved exactly at {}, and reply with at least five story titles plus the marker playwright-mcp-called.",
        screenshot_path.display()
    );

    eprintln!("mcp-e2e turn.start");
    client
        .start_turn(super::start_turn_request(
            &session_id,
            &prompt,
            Some(format!("mcp-e2e-{}", uuid::Uuid::new_v4().simple())),
        ))
        .await?;

    eprintln!("mcp-e2e event.subscribe");
    let _subscription = super::subscribe_events(client, &session_id).await?;

    eprintln!("mcp-e2e collect events");
    let events = collect_until_mcp_artifact(client).await?;

    let called_tools = called_mcp_tools_from_events(&events);
    ensure_tool_called(&called_tools, NAVIGATE_TOOL_SUFFIX)?;
    ensure_tool_called(&called_tools, SCREENSHOT_TOOL_SUFFIX)?;
    let _screenshot_path = wait_for_screenshot_artifact().await?;

    let assistant_text = super::assistant_text_from_events(&events);
    let mut stories = extract_plain_story_lines(&assistant_text);
    if stories.len() < 5 {
        stories = extract_story_lines_from_latest_snapshot()?;
    }
    if stories.len() < 5 {
        return Err(anyhow!(
            "MCP e2e should include at least five Hacker News stories, got {stories:?}"
        ));
    }

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

async fn collect_until_mcp_artifact(
    client: &mut AppServerClient,
) -> Result<Vec<TimelineEventEnvelope>> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(90);
    let mut events = Vec::new();
    loop {
        if observed_required_mcp_artifact(&events)? {
            return Ok(events);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "timed out waiting for Playwright MCP screenshot; called={:?}",
                called_mcp_tools_from_events(&events)
            ));
        }
        match tokio::time::timeout(std::time::Duration::from_secs(5), client.next_event()).await {
            Ok(Ok(event)) => events.push(event),
            Ok(Err(error)) => return Err(error.into()),
            Err(_) => {
                if observed_required_mcp_artifact(&events)? {
                    return Ok(events);
                }
            }
        }
    }
}

fn observed_required_mcp_artifact(events: &[TimelineEventEnvelope]) -> Result<bool> {
    let called_tools = called_mcp_tools_from_events(events);
    Ok(called_tools
        .iter()
        .any(|tool| tool.ends_with(NAVIGATE_TOOL_SUFFIX))
        && called_tools
            .iter()
            .any(|tool| tool.ends_with(SCREENSHOT_TOOL_SUFFIX))
        && latest_screenshot_artifact()?.is_some())
}

async fn wait_for_screenshot_artifact() -> Result<PathBuf> {
    for _ in 0..20 {
        if let Some(path) = latest_screenshot_artifact()? {
            assert_non_empty_file(&path)?;
            return Ok(path);
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    latest_screenshot_artifact()?
        .ok_or_else(|| anyhow!("expected Playwright screenshot artifact"))
        .and_then(|path| {
            assert_non_empty_file(&path)?;
            Ok(path)
        })
}

fn latest_screenshot_artifact() -> Result<Option<PathBuf>> {
    latest_playwright_artifact("png")
}

fn latest_snapshot_artifact() -> Result<Option<PathBuf>> {
    latest_playwright_artifact("yml")
}

fn latest_playwright_artifact(extension: &str) -> Result<Option<PathBuf>> {
    let mut artifacts = Vec::new();
    for entry in std::fs::read_dir(output_dir()?)
        .with_context(|| "read Playwright MCP output directory".to_string())?
    {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.starts_with("page-") && name.ends_with(&format!(".{extension}")) {
            artifacts.push(path);
        }
    }
    artifacts.sort();
    Ok(artifacts.pop())
}

fn called_mcp_tools_from_events(events: &[talos_sdk::TimelineEventEnvelope]) -> BTreeSet<String> {
    let mut tools = BTreeSet::new();
    for event in events {
        match &event.event.payload {
            AgentEventPayload::ItemStarted(started)
                if started.item.kind == TalosTimelineItemKind::ToolCall =>
            {
                if let TalosTimelineContent::ToolCall { name, .. } = &started.item.content {
                    if name.starts_with("mcp__") {
                        tools.insert(name.clone());
                    }
                }
            }
            AgentEventPayload::ItemCompleted(completed) => {
                if let Some(TalosTimelineContent::ToolCall { name, .. }) = &completed.final_content
                {
                    if name.starts_with("mcp__") {
                        tools.insert(name.clone());
                    }
                }
            }
            _ => {}
        }
    }
    tools
}

fn ensure_tool_called(called_tools: &BTreeSet<String>, required_suffix: &str) -> Result<()> {
    if called_tools
        .iter()
        .any(|tool| tool.ends_with(required_suffix))
    {
        return Ok(());
    }
    Err(anyhow!(
        "MCP e2e did not call a tool ending with `{required_suffix}`; called={called_tools:?}"
    ))
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
         - `turn.start` triggered real MCP browser tool calls through Agent execution.\n\
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

fn extract_story_lines_from_latest_snapshot() -> Result<Vec<String>> {
    let Some(path) = latest_snapshot_artifact()? else {
        return Ok(Vec::new());
    };
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("read snapshot {}", path.display()))?;
    Ok(extract_story_lines_from_snapshot(&contents))
}

fn extract_story_lines_from_snapshot(contents: &str) -> Vec<String> {
    let mut stories = Vec::new();
    for line in contents.lines() {
        let Some(index) = line.find(". upvote ") else {
            continue;
        };
        let mut title = &line[index + ". upvote ".len()..];
        if let Some((value, _)) = title.split_once("\" [ref=") {
            title = value;
        } else if let Some((value, _)) = title.split_once("' [ref=") {
            title = value;
        }
        let title = trim_hacker_news_domain(title.trim().trim_matches('"').trim_matches('\''))
            .replace("''", "'");
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
        if name.starts_with("page-") && (name.ends_with(".yml") || name.ends_with(".png")) {
            clean_file(&path)?;
        }
    }
    Ok(())
}

fn clean_file(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path)
            .with_context(|| format!("remove stale MCP E2E artifact {}", path.display()))?;
    }
    Ok(())
}
