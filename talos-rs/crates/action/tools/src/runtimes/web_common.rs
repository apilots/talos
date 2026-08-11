use reqwest::Url;
use talos_error::{TalosError, TalosResult};

pub(crate) const DEFAULT_WEB_USER_AGENT: &str = "Talos/0.1 (+https://github.com/xiliu/talos)";

pub(crate) fn parse_http_url(raw: &str) -> TalosResult<Url> {
    let url = Url::parse(raw).map_err(|err| {
        TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            format!("invalid web url: {err}"),
        )
    })?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        other => Err(TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            format!("unsupported web url scheme: {other}"),
        )),
    }
}

pub(crate) fn html_to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut inside_tag = false;
    let mut previous_was_space = false;
    let mut tag_name = String::new();
    let mut tracking_tag = false;
    let mut skip_depth = 0usize;

    for ch in html.chars() {
        match ch {
            '<' => {
                inside_tag = true;
                tracking_tag = true;
                tag_name.clear();
            }
            '>' => {
                inside_tag = false;
                let normalized = tag_name.trim().to_ascii_lowercase();
                if normalized.starts_with('/') {
                    let closing = normalized.trim_start_matches('/');
                    if matches!(closing, "script" | "style") && skip_depth > 0 {
                        skip_depth -= 1;
                    }
                    if matches!(
                        closing,
                        "p" | "div"
                            | "section"
                            | "article"
                            | "li"
                            | "br"
                            | "h1"
                            | "h2"
                            | "h3"
                            | "h4"
                            | "h5"
                            | "h6"
                    ) {
                        push_space(&mut text, &mut previous_was_space);
                    }
                } else {
                    if matches!(normalized.as_str(), "script" | "style") {
                        skip_depth += 1;
                    }
                    if matches!(
                        normalized.as_str(),
                        "p" | "div"
                            | "section"
                            | "article"
                            | "li"
                            | "br"
                            | "h1"
                            | "h2"
                            | "h3"
                            | "h4"
                            | "h5"
                            | "h6"
                    ) {
                        push_space(&mut text, &mut previous_was_space);
                    }
                }
                tracking_tag = false;
            }
            _ if inside_tag => {
                if tracking_tag {
                    if ch.is_whitespace() || (ch == '/' && !tag_name.is_empty()) {
                        tracking_tag = false;
                    } else {
                        tag_name.push(ch);
                    }
                }
            }
            _ if skip_depth > 0 => {}
            _ => {
                let decoded = decode_html_entity(ch);
                if decoded.is_whitespace() {
                    push_space(&mut text, &mut previous_was_space);
                } else {
                    text.push(decoded);
                    previous_was_space = false;
                }
            }
        }
    }

    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn extract_html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title>")?;
    let end = lower[start + 7..].find("</title>")?;
    let raw = &html[start + 7..start + 7 + end];
    let title = html_to_text(raw);
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }

    let mut output = String::new();
    for ch in text.chars().take(max_chars) {
        output.push(ch);
    }
    output.push_str("...[truncated]");
    (output, true)
}

pub(crate) fn normalize_domain(domain: &str) -> String {
    domain
        .trim()
        .trim_start_matches("www.")
        .to_ascii_lowercase()
}

pub(crate) fn url_domain(url: &Url) -> Option<String> {
    url.host_str().map(normalize_domain)
}

fn push_space(buffer: &mut String, previous_was_space: &mut bool) {
    if !*previous_was_space && !buffer.is_empty() {
        buffer.push(' ');
        *previous_was_space = true;
    }
}

fn decode_html_entity(ch: char) -> char {
    match ch {
        '\u{00a0}' => ' ',
        _ => ch,
    }
}

#[cfg(test)]
#[path = "web_common_tests.rs"]
mod tests;
