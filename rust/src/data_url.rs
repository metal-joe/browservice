//! Port of `src/data_url.{hpp,cpp}`.
//!
//! Signed data URLs are data URLs containing arbitrary data as plain text,
//! with a message authentication code so that we can check whether a URL was
//! created by us. Base64 and URI encoding are done with the CEF functions,
//! exactly like the C++ version.

use cef::*;

const HASH_LENGTH: usize = 32;
const HEX_HASH_LENGTH: usize = 64;
const DATA_URL_HEADER: &str = "data:text/plain;base64,";

pub fn userfree_to_string(s: &CefStringUserfree) -> String {
    CefString::from(s).to_string()
}

fn url_base64_encode(data: &[u8]) -> String {
    let base64 = userfree_to_string(&base64_encode(Some(data)));
    userfree_to_string(&uriencode(Some(&CefString::from(base64.as_str())), 0))
}

fn url_base64_decode(url_base64: &str) -> Option<Vec<u8>> {
    let base64 = userfree_to_string(&uridecode(
        Some(&CefString::from(url_base64)),
        0,
        UriUnescapeRule::URL_SPECIAL_CHARS_EXCEPT_PATH_SEPARATORS,
    ));
    let bin = base64_decode(Some(&CefString::from(base64.as_str())))?;
    let size = bin.size();
    let mut buf = vec![0u8; size];
    if size > 0 {
        bin.data(Some(&mut buf), 0);
    }
    Some(buf)
}

fn hex_digit(val: u8) -> u8 {
    let val = val & 15;
    if val < 10 { b'0' + val } else { b'a' + (val - 10) }
}

fn compute_hash(data: &[u8]) -> Vec<u8> {
    let mut hash = Vec::with_capacity(HEX_HASH_LENGTH);
    for x in crate::sha3::sha3_256(data) {
        hash.push(hex_digit(x >> 4));
        hash.push(hex_digit(x & 15));
    }
    hash
}

/// A random signing key (32 arbitrary bytes).
pub fn generate_data_url_sign_key() -> Vec<u8> {
    let mut key = vec![0u8; HASH_LENGTH];
    crate::common::random_bytes(&mut key);
    key
}

pub fn create_signed_data_url(data: &str, sign_key: &[u8]) -> String {
    if sign_key.len() != HASH_LENGTH {
        crate::panic_log!("Requirement 'signKey.size() == HashLength' failed");
    }
    let mut keyed = sign_key.to_vec();
    keyed.extend_from_slice(data.as_bytes());
    let mut payload = compute_hash(&keyed);
    payload.extend_from_slice(data.as_bytes());
    format!("{DATA_URL_HEADER}{}", url_base64_encode(&payload))
}

/// Returns the data contained in `data_url` if it was signed with
/// `sign_key`, otherwise None.
pub fn read_signed_data_url(data_url: &str, sign_key: &[u8]) -> Option<String> {
    let rest = data_url.strip_prefix(DATA_URL_HEADER)?;
    let url_data = url_base64_decode(rest)?;
    if url_data.len() < HEX_HASH_LENGTH {
        return None;
    }
    // The data of a URL we have signed is always valid UTF-8, so other
    // payloads cannot pass the check below.
    let data = String::from_utf8(url_data[HEX_HASH_LENGTH..].to_vec()).ok()?;
    if data_url == create_signed_data_url(&data, sign_key) {
        Some(data)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        crate::cef_app::select_api_version();
        let key = generate_data_url_sign_key();
        let other_key = generate_data_url_sign_key();
        let url = "https://example.com/a?b=c&d=%20e#f\u{e4}";
        let signed = create_signed_data_url(url, &key);
        assert!(signed.starts_with("data:text/plain;base64,"));
        assert_eq!(read_signed_data_url(&signed, &key).as_deref(), Some(url));
        assert_eq!(read_signed_data_url(&signed, &other_key), None);
        assert_eq!(read_signed_data_url("https://example.com/", &key), None);
        assert_eq!(read_signed_data_url("data:text/plain;base64,aGVsbG8=", &key), None);
        assert_eq!(read_signed_data_url("data:text/plain;base64,", &key), None);
    }
}
