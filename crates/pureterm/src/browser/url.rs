//! GAP-3.5 / GAP-7.8: 浏览器入口的 URL 规范化。
//!
//! [`loadable_url`] 是免依赖的「是否为可加载 URL」核心,由
//! 所有「原始字符串变成 WebView 导航」的位置共享:
//! - 地址栏 UX([`crate::iced_app::normalize_url_input`],它会对非 URL 输入
//!   做 Google 搜索回退),以及
//! - **agent** 路径(GAP-7.8 `browser.navigate` + GAP-3.5 `pane.split_web`),
//!   它们对非 URL 输入则直接*报错*——传垃圾数据的 agent 应当听到报错,
//!   而不是被悄悄带到搜索页。
//!
//! 把核心拆出来,使两种策略(搜索回退 vs. 严格)
//! 保持诚实:URL 判定分支只存在于一处。

/// 若 `input` 可被识别为一个可加载 URL 则原样返回,否则返回 `None`。
///
/// 可识别的形态(镜像 GAP-3.5 地址栏启发式):
/// - 已带 scheme(含 `"://"`)→ 原样返回;
/// - 裸 scheme 且无 `//`、右侧非数字(`about:blank`),
///   `mailto:x@y.io`)→ 原样返回;
/// - 无点主机名 + 端口(`localhost:8080`, `myhost:3000`)→
///   `http://…`(开发服务器);
/// - 有点主机且无空白(`example.com`, `127.0.0.1:3000`)→
///   `https://…`。
///
/// `None` 覆盖空输入与一切非 URL 输入(由调用方决定
/// 怎么处理——搜索回退或报错)。纯函数;可在测试环境中测试。
pub(crate) fn loadable_url(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if s.contains("://") {
        return Some(s.to_string());
    }
    // `rhs` 是端口号 → 无 scheme 的 `host:port`,而非裸 scheme
    // (如 `about:blank`)。用于把 `localhost:8080` 挡在裸 scheme
    // 直通逻辑之外(否则它会原样返回,而 WKWebView 无法
    // 加载无 scheme 的 `localhost:8080`)。
    let rhs_is_port = |rhs: &str| !rhs.is_empty() && rhs.chars().all(|c| c.is_ascii_digit());
    // 裸 scheme(如 "about:blank",无 "://")— 直通。排除
    // host:port 形态(全字母主机 + 数字端口),它在下面处理。
    if let Some((lhs, rhs)) = s.split_once(':') {
        if !lhs.is_empty()
            && lhs.chars().all(|c| c.is_ascii_alphabetic())
            && !rhs.starts_with("//")
            && !lhs.contains('.')
            && !rhs_is_port(rhs)
        {
            return Some(s.to_string());
        }
    }
    // 无 scheme、无点主机名的裸 host:port(如
    // `localhost:8080`, `myhost:3000`)→ 前缀 `http://`(开发服务器)。
    // 有点主机(`127.0.0.1:3000`, `example.com:8443`)继续落入
    // 下方域名分支的 `https://`。
    if !s.chars().any(char::is_whitespace) {
        if let Some((lhs, rhs)) = s.split_once(':') {
            if !lhs.is_empty()
                && lhs.chars().all(|c| c.is_ascii_alphabetic())
                && !lhs.contains('.')
                && rhs_is_port(rhs)
            {
                return Some(format!("http://{s}"));
            }
        }
    }
    // 看起来像域名:含点、无空白。
    if !s.chars().any(char::is_whitespace) && s.contains('.') {
        return Some(format!("https://{s}"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::loadable_url;

    #[test]
    fn scheme_passthrough() {
        assert_eq!(loadable_url("https://example.com/a?b=c").as_deref(), Some("https://example.com/a?b=c"));
        assert_eq!(loadable_url("http://localhost:8080").as_deref(), Some("http://localhost:8080"));
    }

    #[test]
    fn bare_host_port_gets_http() {
        // GAP-7.8 动机场景:`browser navigate localhost:8080` 必须能加载。
        assert_eq!(loadable_url("localhost:8080").as_deref(), Some("http://localhost:8080"));
        assert_eq!(loadable_url("myhost:3000").as_deref(), Some("http://myhost:3000"));
    }

    #[test]
    fn dotted_domain_gets_https() {
        assert_eq!(loadable_url("example.com").as_deref(), Some("https://example.com"));
        assert_eq!(loadable_url("www.google.com/search").as_deref(), Some("https://www.google.com/search"));
        // 有点 host:port 保持 https://(精确:仅裸 host+port → http://)。
        assert_eq!(loadable_url("127.0.0.1:3000").as_deref(), Some("https://127.0.0.1:3000"));
    }

    #[test]
    fn bare_scheme_passthrough() {
        assert_eq!(loadable_url("about:blank").as_deref(), Some("about:blank"));
        assert_eq!(loadable_url("mailto:x@y.io").as_deref(), Some("mailto:x@y.io"));
    }

    #[test]
    fn non_url_and_empty_are_none() {
        // 严格 agent 路径对这些报错(此处无静默搜索回退)。
        assert_eq!(loadable_url("hello world"), None);
        assert_eq!(loadable_url("how do i exit vim"), None);
        assert_eq!(loadable_url(""), None);
        assert_eq!(loadable_url("   "), None);
    }
}
