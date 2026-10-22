//! HTML / URL 解析 —— 悬浮预览的网页部分。
//!
//! 从 `hover_preview.rs` 拆出:这些函数自成一体(纯字符串解析 +
//! 一个网络抓取),且都处理**远程来的 HTML** —— 那里出过一次
//! 字节切片 panic(见 [`favicon_url_for`]),值得单独成文件、单独覆盖。

use std::time::Duration;

use super::PreviewContent;

pub fn parse_html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let after_tag = &html[start..];
    let gt = after_tag.find('>')?;
    let inner = &after_tag[gt + 1..];
    let end = inner.to_ascii_lowercase().find("</title>")?;
    let title = inner[..end].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

pub fn favicon_url_for(page_url: &str, html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    for needle in ["rel=\"icon\"", "rel='icon'", "rel=\"shortcut icon\"", "rel='shortcut icon'"] {
        if let Some(rel_pos) = lower.find(needle) {
            // `rel_pos + 240` 是**字节**下标，可能落在多字节字符中间
            // （远程页面里一个中文标题或 emoji 就足够）—— 直接切片会
            // panic（"byte index is not a char boundary"）。先向下取整
            // 到字符边界。
            let end = floor_char_boundary(html, rel_pos.saturating_add(240).min(html.len()));
            let chunk = &html[rel_pos..end];
            if let Some(href) = extract_href(chunk) {
                return Some(resolve_url(page_url, &href));
            }
        }
    }
    origin_of(page_url).map(|o| format!("{o}/favicon.ico"))
}

/// 把字节下标向下取整到最近的字符边界（`idx` 超出长度时钳到末尾）。
fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn extract_href(chunk: &str) -> Option<String> {
    let lower = chunk.to_ascii_lowercase();
    let href_pos = lower.find("href=")?;
    let rest = &chunk[href_pos + 5..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value = &rest[1..];
    let end = value.find(quote)?;
    Some(value[..end].to_string())
}

fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split('/').next()?.split('?').next()?;
    if host.is_empty() {
        None
    } else {
        Some(format!("{scheme}://{host}"))
    }
}

fn resolve_url(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else if href.starts_with("//") {
        if let Some(scheme) = base.split("://").next() {
            format!("{scheme}:{href}")
        } else {
            href.to_string()
        }
    } else if let Some(origin) = origin_of(base) {
        if href.starts_with('/') {
            format!("{origin}{href}")
        } else {
            format!("{origin}/{href}")
        }
    } else {
        href.to_string()
    }
}

pub async fn fetch_url_preview(url: &str) -> PreviewContent {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .user_agent("PureTerm/0.1 hover-preview")
        .build()
    {
        Ok(c) => c,
        Err(e) => return PreviewContent::Error(e.to_string()),
    };
    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => return PreviewContent::Error(e.to_string()),
    };
    if !resp.status().is_success() {
        return PreviewContent::Error(format!("HTTP {}", resp.status()));
    }
    let html = match resp.text().await {
        Ok(t) => t,
        Err(e) => return PreviewContent::Error(e.to_string()),
    };
    let title = parse_html_title(&html);
    let favicon_url = favicon_url_for(url, &html);
    PreviewContent::Url {
        title,
        favicon_url,
    }
}
