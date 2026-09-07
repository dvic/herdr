pub(crate) const MAX_URL_BYTES: usize = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValidationError {
    Empty,
    TooLong,
    RawControl,
    RawWhitespace,
    MalformedPercentEncoding,
    Malformed,
    UnsupportedScheme,
    MissingHost,
}

impl ValidationError {
    pub(crate) fn class(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::TooLong => "too_long",
            Self::RawControl => "raw_control",
            Self::RawWhitespace => "raw_whitespace",
            Self::MalformedPercentEncoding => "malformed_percent_encoding",
            Self::Malformed => "malformed",
            Self::UnsupportedScheme => "unsupported_scheme",
            Self::MissingHost => "missing_host",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ValidatedUrl<'a>(&'a str);

impl<'a> ValidatedUrl<'a> {
    pub(crate) fn as_str(self) -> &'a str {
        self.0
    }
}

pub(crate) fn validate(url: &str) -> Result<ValidatedUrl<'_>, ValidationError> {
    if url.is_empty() {
        return Err(ValidationError::Empty);
    }
    if url.len() > MAX_URL_BYTES {
        return Err(ValidationError::TooLong);
    }
    if url.chars().any(|ch| ch <= '\u{001f}' || ch == '\u{007f}') {
        return Err(ValidationError::RawControl);
    }
    if url.chars().any(char::is_whitespace) {
        return Err(ValidationError::RawWhitespace);
    }
    if !has_valid_percent_encoding(url.as_bytes()) {
        return Err(ValidationError::MalformedPercentEncoding);
    }

    let parsed = url::Url::parse(url).map_err(|error| match error {
        url::ParseError::EmptyHost => ValidationError::MissingHost,
        _ => ValidationError::Malformed,
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ValidationError::UnsupportedScheme);
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(ValidationError::MissingHost);
    }

    Ok(ValidatedUrl(url))
}

fn has_valid_percent_encoding(bytes: &[u8]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

#[derive(Debug)]
pub(crate) enum LocalDispatchError {
    Invalid,
    Open(std::io::Error),
}

pub(crate) fn dispatch_locally(
    url: &str,
) -> Result<Option<std::process::Child>, LocalDispatchError> {
    dispatch_locally_with(url, crate::platform::open_url)
}

pub(crate) fn dispatch_locally_with(
    url: &str,
    opener: impl FnOnce(&str) -> std::io::Result<Option<std::process::Child>>,
) -> Result<Option<std::process::Child>, LocalDispatchError> {
    let validated = validate(url).map_err(|error| {
        log_validation_failure("local_dispatch", url, error);
        LocalDispatchError::Invalid
    })?;
    opener(validated.as_str()).map_err(|error| {
        tracing::warn!(
            context = "local_dispatch",
            scheme = scheme_class(url),
            utf8_bytes = url.len(),
            error_kind = ?error.kind(),
            raw_os_error = error.raw_os_error(),
            "web URL platform opener failed"
        );
        LocalDispatchError::Open(error)
    })
}

pub(crate) fn log_validation_failure(context: &'static str, url: &str, error: ValidationError) {
    tracing::warn!(
        context,
        scheme = scheme_class(url),
        utf8_bytes = url.len(),
        failure_class = error.class(),
        "web URL validation failed"
    );
}

fn scheme_class(url: &str) -> &'static str {
    let Some((scheme, _)) = url.split_once(':') else {
        return "missing";
    };
    if scheme.eq_ignore_ascii_case("http") {
        "http"
    } else if scheme.eq_ignore_ascii_case("https") {
        "https"
    } else {
        "unsupported"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_supported_urls_without_changing_them() {
        for input in [
            "HTTP://example.com",
            "https://user:password@example.com:8443/path?q=1#fragment",
            "https://localhost/private",
            "https://例え.テスト/道",
            "https://example.com/%20/%00?q=%0A#%7F",
        ] {
            assert_eq!(validate(input).map(ValidatedUrl::as_str), Ok(input));
        }
    }

    #[test]
    fn rejects_unsupported_or_unsafe_urls() {
        let oversized = format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES));
        for (input, expected) in [
            ("", ValidationError::Empty),
            ("relative/path", ValidationError::Malformed),
            ("https://?q=1", ValidationError::MissingHost),
            ("file:///tmp/data", ValidationError::UnsupportedScheme),
            (
                "https://example.com/raw space",
                ValidationError::RawWhitespace,
            ),
            (
                "https://example.com/%GG",
                ValidationError::MalformedPercentEncoding,
            ),
            (&oversized, ValidationError::TooLong),
        ] {
            assert_eq!(validate(input), Err(expected));
        }
    }

    #[test]
    fn local_dispatch_passes_the_exact_url_as_one_value() {
        let input = "https://例え.テスト/path?one=1&two='quoted'#fragment";
        let mut opened = Vec::new();

        dispatch_locally_with(input, |url| {
            opened.push(url.to_owned());
            Ok(None)
        })
        .unwrap();

        assert_eq!(opened, [input]);
    }
}
