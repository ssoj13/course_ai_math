//! String list parsing shared by the config, the environment overrides and the look parser
//! (OCIO `ParseUtils.cpp`).

/// Where the name that starts at `start` ends: the next `sep` outside double quotes, or the end
/// of `s` (OCIO `FindEndOfName`, `ParseUtils.cpp:702-747`).
fn find_end_of_name(s: &str, start: usize, sep: u8) -> Result<usize, String> {
    let b = s.as_bytes();
    let mut pos = start;
    loop {
        let Some(off) = b[pos..].iter().position(|&c| c == b'"' || c == sep) else {
            return Ok(b.len());
        };
        let at = pos + off;
        if b[at] == sep {
            return Ok(at);
        }
        // An opening quote: the name runs to the closing one, then on to the next separator.
        let Some(close) = b[at + 1..].iter().position(|&c| c == b'"') else {
            return Err(format!(
                "The string '{s}' is not correctly formatted. It is missing a closing quote."
            ));
        };
        pos = at + 1 + close + 1;
    }
}

/// Splits a list the way OCIO reads `OCIO_ACTIVE_DISPLAYS`, `OCIO_ACTIVE_VIEWS`,
/// `OCIO_INACTIVE_COLORSPACES` and each `|` option of a look string (OCIO
/// `SplitStringEnvStyle`, `ParseUtils.cpp:749-807`).
///
/// The separator is `,` when the string has one anywhere, else `:`. A name may be quoted to
/// contain the separator; the quotes are removed and every name is trimmed. An empty string
/// yields one empty name (look parsing wants a result). A missing closing quote is an error.
pub(crate) fn split_string_env_style(s: &str) -> Result<Vec<String>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(vec![String::new()]);
    }
    let sep = if s.contains(',') {
        b','
    } else if s.contains(':') {
        b':'
    } else {
        return Ok(vec![unquote(s)]);
    };
    let mut out = Vec::new();
    let mut pos = 0;
    while pos <= s.len() {
        let end = find_end_of_name(s, pos, sep)?;
        if end > pos {
            out.push(unquote(&s[pos..end]));
            pos = end + 1;
        } else {
            out.push(String::new());
            pos += 1;
        }
    }
    Ok(out)
}

fn unquote(v: &str) -> String {
    let t = v.trim();
    if t.len() > 1 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::split_string_env_style as split;

    /// OCIO `ParseUtils_tests.cpp:390-469`, `split_string_env_style`, row for row.
    #[test]
    fn split_string_env_style_matches_ocio() {
        assert_eq!(split("").unwrap(), [""]);
        assert_eq!(
            split("This:is:a:test").unwrap(),
            ["This", "is", "a", "test"]
        );
        assert_eq!(
            split("   \"This\"  : is   :   a:   test  ").unwrap(),
            ["This", "is", "a", "test"]
        );
        assert_eq!(
            split("   This  , is   ,   a,   test  ").unwrap(),
            ["This", "is", "a", "test"]
        );
        assert_eq!(
            split("This:is   ,   a:test  ").unwrap(),
            ["This:is", "a:test"]
        );
        assert_eq!(split(",,").unwrap(), ["", "", ""]);
        assert_eq!(
            split("   \"This  : is   \":   a:   test  ").unwrap(),
            ["This  : is   ", "a", "test"]
        );
        assert_eq!(
            split("   This  : is   \":   a:   test  ").unwrap_err(),
            "The string 'This  : is   \":   a:   test' is not correctly formatted. \
             It is missing a closing quote."
        );
        assert_eq!(
            split("   This  : is   :   a:   test  \"").unwrap_err(),
            "The string 'This  : is   :   a:   test  \"' is not correctly formatted. \
             It is missing a closing quote."
        );
        assert_eq!(
            split("   This  : is   \":   a:   test  \"").unwrap(),
            ["This", "is   \":   a:   test  \""]
        );
        assert_eq!(
            split("   \"This  : is   \",   a,   test  ").unwrap(),
            ["This  : is   ", "a", "test"]
        );
        // A comma anywhere makes it the separator, even inside quotes.
        assert_eq!(
            split("   \"This  , is   \":   a:   test  ").unwrap(),
            ["\"This  , is   \":   a:   test"]
        );
        assert_eq!(
            split("   \"This  , is   \":   a,   test  ").unwrap(),
            ["\"This  , is   \":   a", "test"]
        );
    }
}
