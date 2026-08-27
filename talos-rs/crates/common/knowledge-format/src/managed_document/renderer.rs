use super::KnowledgeDocument;
use crate::{KnowledgeDocumentProfile, OkfDocument};

pub(super) fn empty_template(profile: &KnowledgeDocumentProfile) -> String {
    let mut body = format!(
        "# {}\n\n",
        profile
            .frontmatter
            .title()
            .or_else(|| profile.frontmatter.concept_type())
            .unwrap_or_else(|| "Knowledge Document".to_string())
    );
    if let Some(guidance) = &profile.guidance {
        body.push_str("<!--\n");
        body.push_str(guidance.trim());
        body.push_str("\n-->\n\n");
    }
    body.push_str(&profile.start_marker());
    body.push_str("\n\n");
    for section in &profile.sections {
        body.push_str(&format!("## {}\n\n", section.title));
        if let Some(description) = &section.description {
            body.push_str(&format!("<!-- {} -->\n\n", description.trim()));
        }
    }
    body.push_str(&profile.end_marker());
    body.push('\n');
    OkfDocument::builder()
        .frontmatter(profile.frontmatter.clone())
        .body(body)
        .build()
        .expect("validated profile must produce a conformant template")
        .render()
}

pub(super) fn render_body(document: &KnowledgeDocument) -> String {
    let mut body = document.prefix.trim_end().to_string();
    if !body.is_empty() {
        body.push_str("\n\n");
    }
    body.push_str(&document.profile.start_marker());
    body.push_str("\n\n");
    for section in &document.profile.sections {
        body.push_str(&format!("## {}\n\n", section.title));
        if let Some(description) = &section.description {
            body.push_str(&format!("<!-- {} -->\n\n", description.trim()));
        }
        for entry in document
            .entries
            .iter()
            .filter(|entry| entry.section == section.key)
        {
            body.push_str(&format!(
                "<!-- {}:entry key=\"{}\" -->\n### {}\n\n{}\n\n",
                document.profile.region_namespace,
                entry.key.as_str(),
                entry.title,
                entry.body.as_str().trim()
            ));
        }
    }
    body.push_str(&document.profile.end_marker());
    if !document.suffix.is_empty() {
        body.push_str("\n\n");
        body.push_str(document.suffix.trim());
    }
    body.push('\n');
    body
}

pub(super) fn remove_html_comments(content: &str) -> String {
    let mut output = String::new();
    let mut rest = content;
    while let Some(start) = rest.find("<!--") {
        output.push_str(&rest[..start]);
        let Some(end) = rest[start + 4..].find("-->") else {
            break;
        };
        rest = &rest[start + 4 + end + 3..];
    }
    output.push_str(rest);
    output
}
