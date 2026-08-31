//! Optional base64 outer wrapper decode.

use std::borrow::Cow;

use crate::error::SubscriptionError;

/// Case-insensitive ASCII substring scan without allocating a lowercased copy
/// (subscription bodies can be up to 8 MiB).
pub(crate) fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// If `raw` looks like base64 (no `{`/`proxies:` at start), try decode to UTF-8 text.
/// Plain bodies are returned borrowed (no full-body copy).
pub fn maybe_decode_base64(raw: &str) -> Result<Cow<'_, str>, SubscriptionError> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') || contains_ascii_case_insensitive(trimmed, "proxies:") {
        return Ok(Cow::Borrowed(trimmed));
    }

    // Strip whitespace/newlines common in subscription bodies.
    let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() < 16 || !looks_like_base64(&compact) {
        return Ok(Cow::Borrowed(trimmed));
    }

    use base64::Engine;
    match base64::engine::general_purpose::STANDARD
        .decode(&compact)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&compact))
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&compact))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&compact))
    {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) if !text.trim().is_empty() => Ok(Cow::Owned(text)),
            Ok(_) | Err(_) => Ok(Cow::Borrowed(trimmed)),
        },
        Err(_) => Ok(Cow::Borrowed(trimmed)),
    }
}

fn looks_like_base64(s: &str) -> bool {
    s.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '-' || c == '_'
    })
}
