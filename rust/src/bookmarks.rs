//! Port of `src/bookmarks.{hpp,cpp}`: the bookmark file
//! (`~/.browservice/bookmarks`, same binary format as the C++ version) and the
//! `browservice://bookmarks/` page.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
    pub time: u64,
}

const SIGNATURE: u64 = 0xBA0F5EAF1CEB00C3;

#[derive(Default)]
pub struct Bookmarks {
    data: BTreeMap<u64, Bookmark>,
}

struct BookmarkCache {
    initialized: bool,
    by_url: BTreeMap<String, u64>,
}

static BOOKMARK_CACHE: Mutex<BookmarkCache> = Mutex::new(BookmarkCache {
    initialized: false,
    by_url: BTreeMap::new(),
});

fn populate_bookmark_cache(cache: &mut BookmarkCache, bookmarks: &Bookmarks) {
    cache.by_url.clear();
    for (id, bookmark) in &bookmarks.data {
        cache.by_url.insert(bookmark.url.clone(), *id);
    }
    cache.initialized = true;
}

fn write_le(out: &mut Vec<u8>, val: u64) {
    out.extend_from_slice(&val.to_le_bytes());
}

fn write_str(out: &mut Vec<u8>, val: &[u8]) {
    write_le(out, val.len() as u64);
    out.extend_from_slice(val);
    let pad = (0usize.wrapping_sub(val.len())) & 7;
    out.extend(std::iter::repeat_n(0u8, pad));
}

fn read_le(fp: &mut impl Read) -> Option<u64> {
    let mut bytes = [0u8; 8];
    fp.read_exact(&mut bytes).ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn read_str(fp: &mut impl Read) -> Option<Vec<u8>> {
    let size = read_le(fp)?;
    let pad = (0u64.wrapping_sub(size)) & 7;
    let padded = size.checked_add(pad)?;
    let mut buf = Vec::new();
    fp.take(padded).read_to_end(&mut buf).ok()?;
    if buf.len() as u64 != padded {
        return None;
    }
    buf.truncate(size as usize);
    Some(buf)
}

enum LoadError {
    Read,
    Format,
}

fn bookmark_path(dot_dir: &str) -> String {
    format!("{dot_dir}/bookmarks")
}

impl Bookmarks {
    pub fn data(&self) -> &BTreeMap<u64, Bookmark> {
        &self.data
    }

    /// Loads the bookmarks from `<dot_dir>/bookmarks`. Returns None and writes
    /// to the log if loading failed.
    pub fn load_from(dot_dir: &str) -> Option<Bookmarks> {
        let path = bookmark_path(dot_dir);
        match std::fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                crate::info_log!(
                    "Bookmark file '{}' does not exist, using empty set of bookmarks",
                    path
                );
                return Some(Bookmarks::default());
            }
            _ => {}
        }

        match Self::read_file(&path) {
            Ok(ret) => {
                crate::info_log!("Bookmarks successfully read from '{}'", path);
                Some(ret)
            }
            Err(LoadError::Read) => {
                crate::error_log!("Loading bookmarks failed: Reading file '{}' failed", path);
                None
            }
            Err(LoadError::Format) => {
                crate::error_log!(
                    "Loading bookmarks failed: File '{}' has invalid format",
                    path
                );
                None
            }
        }
    }

    fn read_file(path: &str) -> Result<Bookmarks, LoadError> {
        let file = std::fs::File::open(path).map_err(|_| LoadError::Read)?;
        let mut fp = std::io::BufReader::new(file);
        let mut ret = Bookmarks::default();

        if read_le(&mut fp).ok_or(LoadError::Read)? != SIGNATURE {
            return Err(LoadError::Format);
        }
        if read_le(&mut fp).ok_or(LoadError::Read)? != 0 {
            return Err(LoadError::Format);
        }
        loop {
            let has_next = read_le(&mut fp).ok_or(LoadError::Read)?;
            if has_next != 0 && has_next != 1 {
                return Err(LoadError::Format);
            }
            if has_next == 0 {
                break;
            }
            let id = read_le(&mut fp).ok_or(LoadError::Read)?;
            let url = read_str(&mut fp).ok_or(LoadError::Read)?;
            let title = read_str(&mut fp).ok_or(LoadError::Read)?;
            let time = read_le(&mut fp).ok_or(LoadError::Read)?;
            // The C++ version stores raw bytes in std::string; Rust strings
            // must be UTF-8, so invalid sequences are replaced.
            let bookmark = Bookmark {
                url: String::from_utf8_lossy(&url).into_owned(),
                title: String::from_utf8_lossy(&title).into_owned(),
                time,
            };
            if ret.data.insert(id, bookmark).is_some() {
                return Err(LoadError::Format);
            }
        }
        Ok(ret)
    }

    pub fn load() -> Option<Bookmarks> {
        Self::load_from(&crate::globals::globals().dot_dir_path)
    }

    /// Saves the bookmarks atomically (temporary file and rename). Returns
    /// false and writes to the log if saving failed.
    pub fn save_to(&self, dot_dir: &str) -> bool {
        let path = bookmark_path(dot_dir);
        const PALETTE: &[u8] = b"abcdefghijklmnopqrstuvABCDEFGHIJKLMNOPQRSTUV0123456789";
        let mut tmp_path = format!("{dot_dir}/.tmp.bookmarks.");
        for _ in 0..16 {
            let idx = (crate::common::random_u64() % PALETTE.len() as u64) as usize;
            tmp_path.push(PALETTE[idx] as char);
        }

        let mut out = Vec::new();
        write_le(&mut out, SIGNATURE);
        write_le(&mut out, 0);
        for (id, bookmark) in &self.data {
            write_le(&mut out, 1);
            write_le(&mut out, *id);
            write_str(&mut out, bookmark.url.as_bytes());
            write_str(&mut out, bookmark.title.as_bytes());
            write_le(&mut out, bookmark.time);
        }
        write_le(&mut out, 0);

        let written = std::fs::File::create(&tmp_path)
            .and_then(|mut f| f.write_all(&out).and_then(|_| f.flush()));
        if written.is_err() {
            crate::error_log!(
                "Saving bookmarks failed: Could not write temporary file '{}'",
                tmp_path
            );
            return false;
        }

        if std::fs::rename(&tmp_path, &path).is_err() {
            crate::error_log!(
                "Saving bookmarks failed: Renaming temporary file '{}' to '{}' failed",
                tmp_path,
                path
            );
            let _ = std::fs::remove_file(&tmp_path);
            return false;
        }

        crate::info_log!("Bookmarks successfully written to '{}'", path);
        populate_bookmark_cache(&mut BOOKMARK_CACHE.lock().unwrap(), self);
        true
    }

    pub fn save(&self) -> bool {
        self.save_to(&crate::globals::globals().dot_dir_path)
    }

    pub fn put_bookmark(&mut self, bookmark: Bookmark) -> u64 {
        let mut id;
        loop {
            id = crate::common::random_u64();
            if !self.data.contains_key(&id) {
                break;
            }
        }
        self.data.insert(id, bookmark);
        id
    }

    pub fn remove_bookmark(&mut self, id: u64) {
        self.data.remove(&id);
    }
}

/// Returns the bookmark ID if `url` is bookmarked in the last saved state
/// (or the state loaded on the first call). Safe to call from any thread.
pub fn cached_bookmark_id_by_url(url: &str) -> Option<u64> {
    let mut cache = BOOKMARK_CACHE.lock().unwrap();
    if !cache.initialized {
        match Bookmarks::load() {
            Some(bookmarks) => populate_bookmark_cache(&mut cache, &bookmarks),
            None => {
                cache.by_url.clear();
                cache.initialized = true;
            }
        }
    }
    cache.by_url.get(url).copied()
}

fn html_escape_string(s: &str) -> String {
    let mut ret = String::new();
    for point in crate::common::sanitize_utf8_string_to_code_points(s.as_bytes()) {
        if point == 0 || point > 0x10FFFF {
            continue;
        }
        if point == 0xD {
            continue;
        }
        if (point <= 0x1F || (0x7F..=0x9F).contains(&point))
            && point != 0x20
            && point != 0x9
            && point != 0xA
            && point != 0xC
        {
            continue;
        }
        if (0xFDD0..=0xFDEF).contains(&point) {
            continue;
        }
        if (point & 0xFFFF) == 0xFFFE || (point & 0xFFFF) == 0xFFFF {
            continue;
        }
        ret.push_str(&format!("&#{point};"));
    }
    ret
}

/// Parses an unsigned decimal like `stringstream >> uint64_t` followed by an
/// end-of-input check (`parseString<uint64_t>` in `src/common.hpp`): leading
/// whitespace and a sign are accepted, a minus sign wraps around, overflow
/// and trailing characters fail.
fn parse_u64_like_cpp(s: &str) -> Option<u64> {
    let s = s.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let val: u64 = digits.parse().ok()?;
    Some(if neg { val.wrapping_neg() } else { val })
}

fn parse_selected_bookmarks(form_data: &str, selected: &mut BTreeSet<u64>) {
    if form_data.is_empty() {
        return;
    }
    for elem in form_data.split('&') {
        const PREFIX: &str = "bookmark";
        const SUFFIX: &str = "=on";
        if elem.len() > PREFIX.len() + SUFFIX.len()
            && elem.starts_with(PREFIX)
            && elem.ends_with(SUFFIX)
            && let Some(id) = parse_u64_like_cpp(&elem[PREFIX.len()..elem.len() - SUFFIX.len()])
        {
            selected.insert(id);
        }
    }
}

/// Builds the bookmarks page. `post_bodies` are the byte elements of the POST
/// data if the request was a POST request (None for other methods).
pub fn bookmarks_page(
    mut bookmarks: Option<Bookmarks>,
    post_bodies: Option<Vec<Vec<u8>>>,
    save: impl FnOnce(&Bookmarks) -> bool,
) -> String {
    if let Some(bodies) = post_bodies {
        let mut selected = BTreeSet::new();
        for body in bodies {
            // Form data of the bookmark page is ASCII; ignore anything else.
            parse_selected_bookmarks(&String::from_utf8_lossy(&body), &mut selected);
        }
        if !selected.is_empty()
            && let Some(bookmarks) = bookmarks.as_mut()
        {
            for id in selected {
                bookmarks.remove_bookmark(id);
            }
            save(bookmarks);
        }
    }

    let mut page = String::from(concat!(
        "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"UTF-8\">",
        "<title>Bookmarks</title></head><body>\n",
        "<h1>Bookmarks</h1>\n",
        "<form action=\"browservice://bookmarks/\" method=\"POST\">\n",
    ));

    if let Some(bookmarks) = &bookmarks {
        let mut items: Vec<(&u64, &Bookmark)> = bookmarks.data.iter().collect();
        items.sort_by(|a, b| {
            (a.1.time, &a.1.title, &a.1.url).cmp(&(b.1.time, &b.1.title, &b.1.url))
        });
        for (id, bookmark) in &items {
            page += "<p style=\"margin-bottom:3px; margin-top:3px;\">\n";
            page += &format!(
                "<input type=\"checkbox\" id=\"bookmark{id}\" name=\"bookmark{id}\">\n"
            );
            page += &format!("<label for=\"bookmark{id}\">\n");
            page += &format!(
                "<a href=\"{}\">{}</a>\n",
                html_escape_string(&bookmark.url),
                html_escape_string(&bookmark.title)
            );
            page += "</label>\n";
            page += "</p>\n";
        }
        if items.is_empty() {
            page += "<p>You have no bookmarks</p>\n";
        } else {
            page += "<p><input type=\"submit\" value=\"Remove selected\"></p>\n";
        }
    } else {
        page += "<p style=\"color:#FF0000;\">Loading bookmarks failed (see log)</p>\n";
    }

    page += "</form>\n";
    page += "</body></html>\n";
    page
}

/// Handles a request to `browservice://bookmarks/` (IO thread).
pub fn handle_bookmarks_request(request: &mut cef::Request) -> String {
    use cef::*;

    let method = crate::data_url::userfree_to_string(&request.method());
    let post_bodies = if method == "POST" {
        let mut bodies = Vec::new();
        if let Some(post_data) = request.post_data() {
            let mut elements: Vec<Option<PostDataElement>> = Vec::new();
            post_data.elements(Some(&mut elements));
            for elem in elements.into_iter().flatten() {
                if elem.get_type() == PostdataelementType::BYTES {
                    let size = elem.bytes_count();
                    let mut data = vec![0u8; size];
                    if elem.bytes(size, data.as_mut_ptr()) == size {
                        bodies.push(data);
                    }
                }
            }
        }
        Some(bodies)
    } else {
        None
    };
    bookmarks_page(Bookmarks::load(), post_bodies, |b| b.save())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir() -> String {
        let dir = format!(
            "{}/bookmarks_test_{}",
            std::env::temp_dir().display(),
            crate::common::random_u64()
        );
        std::fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn save_load_roundtrip_and_format() {
        let dir = tmp_dir();
        assert!(Bookmarks::load_from(&dir).unwrap().data().is_empty());

        let mut b = Bookmarks::default();
        let id1 = b.put_bookmark(Bookmark {
            url: "https://a.example/".into(),
            title: "A".into(),
            time: 5,
        });
        let id2 = b.put_bookmark(Bookmark {
            url: "https://b.example/xyz".into(),
            title: "Bee title".into(),
            time: 3,
        });
        assert!(b.save_to(&dir));

        let raw = std::fs::read(format!("{dir}/bookmarks")).unwrap();
        assert_eq!(&raw[..8], &SIGNATURE.to_le_bytes());
        assert_eq!(raw.len() % 8, 0);

        let loaded = Bookmarks::load_from(&dir).unwrap();
        assert_eq!(loaded.data(), b.data());
        assert_eq!(loaded.data()[&id1].title, "A");

        let page = bookmarks_page(Some(loaded), None, |_| unreachable!());
        let pos_a = page.find(&format!("bookmark{id1}")).unwrap();
        let pos_b = page.find(&format!("bookmark{id2}")).unwrap();
        assert!(pos_b < pos_a, "sorted by time");
        assert!(page.contains("&#66;&#101;&#101;"));
        assert!(page.contains("Remove selected"));

        // Remove one through form data.
        let form = format!("bookmark{id2}=on&other=1").into_bytes();
        let mut saved = false;
        let page = bookmarks_page(
            Bookmarks::load_from(&dir),
            Some(vec![form]),
            |b| {
                saved = true;
                b.save_to(&dir)
            },
        );
        assert!(saved);
        assert!(!page.contains(&format!("bookmark{id2}")));
        assert_eq!(Bookmarks::load_from(&dir).unwrap().data().len(), 1);

        std::fs::write(format!("{dir}/bookmarks"), b"garbage!garbage!").unwrap();
        assert!(Bookmarks::load_from(&dir).is_none());
        let page = bookmarks_page(None, None, |_| true);
        assert!(page.contains("Loading bookmarks failed"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_page() {
        let page = bookmarks_page(Some(Bookmarks::default()), None, |_| true);
        assert!(page.contains("<p>You have no bookmarks</p>"));
    }

    #[test]
    fn parse_ids() {
        assert_eq!(parse_u64_like_cpp("123"), Some(123));
        assert_eq!(parse_u64_like_cpp("+7"), Some(7));
        assert_eq!(parse_u64_like_cpp("-1"), Some(u64::MAX));
        assert_eq!(parse_u64_like_cpp("12a"), None);
        assert_eq!(parse_u64_like_cpp("99999999999999999999"), None);
        let mut s = BTreeSet::new();
        parse_selected_bookmarks("bookmark1=on&bookmark2=off&bookmark=on&bookmark3=on", &mut s);
        assert_eq!(s.into_iter().collect::<Vec<_>>(), vec![1, 3]);
    }

    #[test]
    fn escape() {
        assert_eq!(html_escape_string("a<\r\x01\u{fdd0}"), "&#97;&#60;");
    }
}
