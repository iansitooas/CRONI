use url::Url;

pub fn normalize_address(input: &str, search_template: &str) -> String {
    let value = input.trim();
    if value.is_empty() {
        return "about:blank".to_string();
    }

    if Url::parse(value).is_ok() {
        return value.to_string();
    }

    let looks_like_host = !value.contains(char::is_whitespace)
        && (value.contains('.') || value.eq_ignore_ascii_case("localhost"));
    if looks_like_host {
        let candidate = format!("https://{value}");
        if Url::parse(&candidate).is_ok() {
            return candidate;
        }
    }

    let encoded: String = url::form_urlencoded::byte_serialize(value.as_bytes()).collect();
    search_template.replace("{query}", &encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = "https://www.google.com/search?q={query}";

    #[test]
    fn preserves_complete_url() {
        assert_eq!(
            normalize_address("https://example.com/a", SEARCH),
            "https://example.com/a"
        );
    }

    #[test]
    fn adds_https_to_hosts() {
        assert_eq!(
            normalize_address("example.com", SEARCH),
            "https://example.com"
        );
    }

    #[test]
    fn searches_plain_text() {
        assert_eq!(
            normalize_address("memoria baja", SEARCH),
            "https://www.google.com/search?q=memoria+baja"
        );
    }
}
