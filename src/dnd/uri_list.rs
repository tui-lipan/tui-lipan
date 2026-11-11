//! `text/uri-list` (RFC 2483), and the local paths its `file:` URIs name.

use std::path::PathBuf;

/// The URIs of a `text/uri-list`, comments and blank lines dropped.
pub fn parse_uri_list(data: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(data)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// A `text/uri-list` naming `uris`, lines ended with CRLF as RFC 2483 asks.
pub fn format_uri_list<S: AsRef<str>>(uris: &[S]) -> Vec<u8> {
    let mut out = Vec::new();
    for uri in uris {
        out.extend_from_slice(uri.as_ref().as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// The local path a `file:` URI names, or `None` for another scheme or another host.
///
/// `file:/path`, `file:///path`, and `file://localhost/path` are local. A URI naming any other
/// host is not, even when that host is this machine's name: a drop only carries such a URI when
/// the file is elsewhere.
pub fn file_uri_path(uri: &str) -> Option<PathBuf> {
    let rest = strip_prefix_ignore_case(uri, "file:")?;
    let path = if let Some(authority_and_path) = rest.strip_prefix("//") {
        let slash = authority_and_path.find('/')?;
        let host = &authority_and_path[..slash];
        if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
            return None;
        }
        &authority_and_path[slash..]
    } else {
        rest
    };
    let path = path.split(['?', '#']).next()?;
    let bytes = percent_decode(path.as_bytes())?;
    if bytes.is_empty() || bytes.contains(&0) {
        return None;
    }
    path_from_bytes(bytes)
}

/// The `file://` URI for an absolute local path, with reserved bytes percent-encoded.
pub fn file_uri(path: &std::path::Path) -> Option<String> {
    let bytes = path_bytes(path)?;
    let mut out = String::from("file://");
    #[cfg(windows)]
    out.push('/');
    for &byte in &bytes {
        if byte.is_ascii_alphanumeric() || b"/-._~!$&'()*+,;=:@".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    Some(out)
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

fn percent_decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let hex = input.get(index + 1..index + 3)?;
            let value = u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
            out.push(value);
            index += 3;
        } else {
            out.push(input[index]);
            index += 1;
        }
    }
    Some(out)
}

#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    path.is_absolute().then_some(path)
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: Vec<u8>) -> Option<PathBuf> {
    let text = String::from_utf8(bytes).ok()?;
    // `/C:/Users/x` names `C:/Users/x`.
    let text = match text.as_bytes() {
        [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => text[1..].to_owned(),
        _ => text,
    };
    let path = PathBuf::from(text);
    path.is_absolute().then_some(path)
}

#[cfg(unix)]
fn path_bytes(path: &std::path::Path) -> Option<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    path.is_absolute()
        .then(|| path.as_os_str().as_bytes().to_vec())
}

#[cfg(not(unix))]
fn path_bytes(path: &std::path::Path) -> Option<Vec<u8>> {
    if !path.is_absolute() {
        return None;
    }
    Some(path.to_str()?.replace('\\', "/").into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lists_with_comments_and_either_line_ending() {
        let list =
            parse_uri_list(b"# from a file manager\r\nfile:///tmp/a%20b\r\n\nhttps://x.org/\n");
        assert_eq!(list, ["file:///tmp/a%20b", "https://x.org/"]);
        assert_eq!(
            format_uri_list(&list),
            b"file:///tmp/a%20b\r\nhttps://x.org/\r\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn local_file_uris_become_paths_and_back() {
        assert_eq!(
            file_uri_path("file:///tmp/a%20b").as_deref(),
            Some(std::path::Path::new("/tmp/a b"))
        );
        assert_eq!(
            file_uri_path("FILE://localhost/tmp/x").as_deref(),
            Some(std::path::Path::new("/tmp/x"))
        );
        assert_eq!(
            file_uri_path("file:/tmp/x").as_deref(),
            Some(std::path::Path::new("/tmp/x"))
        );
        assert_eq!(file_uri_path("file://otherhost/tmp/x"), None);
        assert_eq!(file_uri_path("https://example.org/x"), None);
        assert_eq!(file_uri_path("file:///tmp/%00"), None);
        assert_eq!(file_uri_path("file:///tmp/%zz"), None);
        let path = std::path::Path::new("/tmp/a b/ü#?");
        let uri = file_uri(path).unwrap();
        assert_eq!(uri, "file:///tmp/a%20b/%C3%BC%23%3F");
        assert_eq!(file_uri_path(&uri).as_deref(), Some(path));
    }
}
