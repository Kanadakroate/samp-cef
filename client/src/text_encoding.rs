//! LR's GTA ANSI window uses Windows-1252. Larger WM_CHAR values are UTF-16.
pub fn window_character(wparam: usize) -> Option<u16> {
    if wparam <= u8::MAX as usize {
        let bytes = [wparam as u8];
        encoding_rs::WINDOWS_1252.decode(&bytes).0.encode_utf16().next()
    } else {
        u16::try_from(wparam).ok()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn german_ansi_input_and_utf16_are_preserved() {
        for (byte, scalar) in [(0xe4, 0x00e4), (0xf6, 0x00f6), (0xfc, 0x00fc), (0xdf, 0x00df), (0x80, 0x20ac)] {
            assert_eq!(window_character(byte), Some(scalar));
        }
        assert_eq!(window_character(b'A' as usize), Some(0x41));
        assert_eq!(window_character(0x20ac), Some(0x20ac));
        assert_eq!(window_character(0xd83d), Some(0xd83d));
        assert_eq!(window_character(0x10000), None);
    }
}
