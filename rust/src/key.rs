//! Port of `src/key.{hpp,cpp}` and `src/key_codes.hpp`.
//!
//! Keys are integers: positive integers are Unicode code points and negative
//! integers are negated Windows key codes of non-character keys.

pub mod keys {
    pub const BACKSPACE: i32 = -8;
    pub const TAB: i32 = -9;
    pub const ENTER: i32 = -13;
    pub const SHIFT: i32 = -16;
    pub const CONTROL: i32 = -17;
    pub const ALT: i32 = -18;
    pub const CAPS_LOCK: i32 = -20;
    pub const ESC: i32 = -27;
    pub const SPACE: i32 = -32;
    pub const PAGE_UP: i32 = -33;
    pub const PAGE_DOWN: i32 = -34;
    pub const END: i32 = -35;
    pub const HOME: i32 = -36;
    pub const LEFT: i32 = -37;
    pub const UP: i32 = -38;
    pub const RIGHT: i32 = -39;
    pub const DOWN: i32 = -40;
    pub const INSERT: i32 = -45;
    pub const DELETE: i32 = -46;
    pub const WIN: i32 = -91;
    pub const MENU: i32 = -93;
    pub const F1: i32 = -112;
    pub const F2: i32 = -113;
    pub const F3: i32 = -114;
    pub const F4: i32 = -115;
    pub const F5: i32 = -116;
    pub const F6: i32 = -117;
    pub const F7: i32 = -118;
    pub const F8: i32 = -119;
    pub const F9: i32 = -120;
    pub const F10: i32 = -121;
    pub const F11: i32 = -122;
    pub const F12: i32 = -123;
    pub const NUM_LOCK: i32 = -144;
}

use keys::*;

const VALID_NON_CHAR_KEYS: [i32; 34] = [
    BACKSPACE, TAB, ENTER, SHIFT, CONTROL, ALT, CAPS_LOCK, ESC, SPACE, PAGE_UP, PAGE_DOWN, END,
    HOME, LEFT, UP, RIGHT, DOWN, INSERT, DELETE, WIN, MENU, F1, F2, F3, F4, F5, F6, F7, F8, F9,
    F10, F11, F12, NUM_LOCK,
];

pub fn is_valid_key(key: i32) -> bool {
    (1..=0xD7FF).contains(&key)
        || (0xE000..=0x10FFFF).contains(&key)
        || VALID_NON_CHAR_KEYS.contains(&key)
}

/// UTF-8 representation of the character of a valid key; empty for
/// non-character keys (key < 0).
pub fn key_to_utf8(key: i32) -> Vec<u8> {
    if !is_valid_key(key) {
        crate::panic_log!("Requirement 'isValidKey(key)' failed");
    }
    if key < 0 {
        return Vec::new();
    }
    let mut buf = [0u8; 4];
    char::from_u32(key as u32)
        .expect("valid keys are Unicode scalar values")
        .encode_utf8(&mut buf)
        .as_bytes()
        .to_vec()
}

/// (Windows key code, X11 native key code) pairs for some keys
/// (`keyCodes` in `src/key_codes.hpp`).
pub fn key_codes(key: i32) -> Option<(i32, i32)> {
    const DUAL: &[(u8, u8, i32, i32)] = &[
        (b'a', b'A', 65, 38),
        (b'b', b'B', 66, 56),
        (b'c', b'C', 67, 54),
        (b'd', b'D', 68, 40),
        (b'e', b'E', 69, 26),
        (b'f', b'F', 70, 41),
        (b'g', b'G', 71, 42),
        (b'h', b'H', 72, 43),
        (b'i', b'I', 73, 31),
        (b'j', b'J', 74, 44),
        (b'k', b'K', 75, 45),
        (b'l', b'L', 76, 46),
        (b'm', b'M', 77, 58),
        (b'n', b'N', 78, 57),
        (b'o', b'O', 79, 32),
        (b'p', b'P', 80, 33),
        (b'q', b'Q', 81, 24),
        (b'r', b'R', 82, 27),
        (b's', b'S', 83, 39),
        (b't', b'T', 84, 28),
        (b'u', b'U', 85, 30),
        (b'v', b'V', 86, 55),
        (b'w', b'W', 87, 25),
        (b'x', b'X', 88, 53),
        (b'y', b'Y', 89, 29),
        (b'z', b'Z', 90, 52),
        (b'`', b'~', 192, 49),
        (b'1', b'!', 49, 10),
        (b'2', b'@', 50, 11),
        (b'3', b'#', 51, 12),
        (b'4', b'$', 52, 13),
        (b'5', b'%', 53, 14),
        (b'6', b'^', 54, 15),
        (b'7', b'&', 55, 16),
        (b'8', b'*', 56, 17),
        (b'9', b'(', 57, 18),
        (b'0', b')', 48, 19),
        (b'-', b'_', 189, 20),
        (b'=', b'+', 187, 21),
        (b'[', b'{', 219, 34),
        (b']', b'}', 221, 35),
        (b'\\', b'|', 220, 51),
        (b';', b':', 186, 47),
        (b'\'', b'"', 222, 48),
        (b',', b'<', 188, 59),
        (b'.', b'>', 190, 60),
        (b'/', b'?', 191, 61),
    ];
    const SINGLE: &[(i32, i32, i32)] = &[
        (BACKSPACE, 8, 22),
        (TAB, 9, 23),
        (ENTER, 13, 36),
        (SHIFT, 16, 50),
        (CONTROL, 17, 37),
        (ALT, 18, 64),
        (CAPS_LOCK, 20, 66),
        (ESC, 27, 9),
        (SPACE, 32, 65),
        (PAGE_UP, 33, 112),
        (PAGE_DOWN, 34, 117),
        (HOME, 36, 110),
        (END, 35, 115),
        (LEFT, 37, 113),
        (UP, 38, 11),
        (RIGHT, 39, 114),
        (DOWN, 40, 116),
        (INSERT, 45, 118),
        (DELETE, 46, 119),
        (WIN, 91, 133),
        (MENU, 93, 135),
        (F1, 112, 67),
        (F2, 113, 68),
        (F3, 114, 69),
        (F4, 115, 70),
        (F5, 116, 71),
        (F6, 117, 72),
        (F7, 118, 73),
        (F8, 119, 74),
        (F9, 120, 75),
        (F10, 121, 76),
        (F11, 122, 95),
        (F12, 123, 96),
        (NUM_LOCK, 144, 77),
    ];
    for &(c1, c2, win, nat) in DUAL {
        if key == c1 as i32 || key == c2 as i32 {
            return Some((win, nat));
        }
    }
    SINGLE
        .iter()
        .find(|&&(k, _, _)| k == key)
        .map(|&(_, win, nat)| (win, nat))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validity() {
        assert!(is_valid_key('a' as i32));
        assert!(is_valid_key(0x10FFFF));
        assert!(!is_valid_key(0));
        assert!(!is_valid_key(0xD800));
        assert!(!is_valid_key(0x110000));
        assert!(is_valid_key(keys::F5));
        assert!(!is_valid_key(-1));
        assert!(!is_valid_key(-200));
    }

    #[test]
    fn utf8() {
        assert_eq!(key_to_utf8('a' as i32), b"a");
        assert_eq!(key_to_utf8(0xE4), "\u{e4}".as_bytes());
        assert_eq!(key_to_utf8(0x20AC), "\u{20ac}".as_bytes());
        assert_eq!(key_to_utf8(0x1F600), "\u{1f600}".as_bytes());
        assert!(key_to_utf8(keys::ENTER).is_empty());
    }

    #[test]
    fn codes() {
        assert_eq!(key_codes('Q' as i32), Some((81, 24)));
        assert_eq!(key_codes(keys::ENTER), Some((13, 36)));
        assert_eq!(key_codes(0xE4), None);
    }
}
