//! GAP-3.5 / GAP-7.8: URL normalization for browser entry points.
//!
//! [`loadable_url`] is the dependency-free "is this a loadable URL?" core shared by
//! every place a raw string becomes a WebView navigation:
//! - the address-bar UX ([`crate::iced_app::normalize_url_input`], which adds a
//!   Google-search fallback for non-URL input), and
//! - the **agent** paths (GAP-7.8 `browser.navigate` + GAP-3.5 `pane.split_web`),
//!   which instead *error* on non-URL input — an agent passing garbage should hear
//!   about it, not silently land on a search page.
//!
//! Splitting the core out keeps the two policies (search-fallback vs. strict)
//! honest: the URL-detection branches live in exactly one place.

/// Return `input` as a loadable URL if it is recognizable as one, else `None`.
///
/// Recognized shapes (mirroring the GAP-3.5 address-bar heuristic):
/// - already has a scheme (`contains "://"`) → as-is;
/// - a bare scheme with no `//` and a non-numeric rhs (`about:blank`,
///   `mailto:x@y.io`) → as-is;
/// - a bare dot-less hostname + port (`localhost:8080`, `myhost:3000`) →
///   `http://…` (dev servers);
/// - a dotted host with no whitespace (`example.com`, `127.0.0.1:3000`) →
///   `https://…`.
///
/// `None` covers empty input and anything that isn't a URL (the caller decides
/// what to do — search fallback or error). Pure function; harness-testable.
pub(crate) fn loadable_url(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if s.contains("://") {
        return Some(s.to_string());
    }
    // `rhs` is a port number → `host:port` with no scheme, not a bare scheme
    // like `about:blank`. Used to keep `localhost:8080` out of the bare-scheme
    // passthrough below (it'd otherwise be returned verbatim and WKWebView can't
    // load a scheme-less `localhost:8080`).
    let rhs_is_port = |rhs: &str| !rhs.is_empty() && rhs.chars().all(|c| c.is_ascii_digit());
    // Bare scheme like "about:blank" (no "://") — pass through. Exclude the
    // host:port shape (all-alpha host + numeric port), which is handled below.
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
    // Bare host:port with no scheme and a dot-less hostname (e.g.
    // `localhost:8080`, `myhost:3000`) → prefix `http://` (dev servers). Dotted
    // hosts (`127.0.0.1:3000`, `example.com:8443`) keep falling through to the
    // domain branch's `https://` below.
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
    // Looks like a domain: has a dot, no whitespace.
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
        // GAP-7.8 motivating case: `browser navigate localhost:8080` must load.
        assert_eq!(loadable_url("localhost:8080").as_deref(), Some("http://localhost:8080"));
        assert_eq!(loadable_url("myhost:3000").as_deref(), Some("http://myhost:3000"));
    }

    #[test]
    fn dotted_domain_gets_https() {
        assert_eq!(loadable_url("example.com").as_deref(), Some("https://example.com"));
        assert_eq!(loadable_url("www.google.com/search").as_deref(), Some("https://www.google.com/search"));
        // Dotted host:port stays https:// (surgical: only bare-host+port → http://).
        assert_eq!(loadable_url("127.0.0.1:3000").as_deref(), Some("https://127.0.0.1:3000"));
    }

    #[test]
    fn bare_scheme_passthrough() {
        assert_eq!(loadable_url("about:blank").as_deref(), Some("about:blank"));
        assert_eq!(loadable_url("mailto:x@y.io").as_deref(), Some("mailto:x@y.io"));
    }

    #[test]
    fn non_url_and_empty_are_none() {
        // The strict agent path errors on these (no silent search fallback here).
        assert_eq!(loadable_url("hello world"), None);
        assert_eq!(loadable_url("how do i exit vim"), None);
        assert_eq!(loadable_url(""), None);
        assert_eq!(loadable_url("   "), None);
    }
}
