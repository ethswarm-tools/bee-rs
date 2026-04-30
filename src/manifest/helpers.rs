//! Internal helpers for the Mantaray wire format and trie operations.

/// XOR `dst` byte-by-byte with `key`, repeating `key` as needed.
/// Empty `key` is a no-op (matches bee-js `xorCypher`).
pub fn xor_in_place(dst: &mut [u8], key: &[u8]) {
    if key.is_empty() {
        return;
    }
    for (i, b) in dst.iter_mut().enumerate() {
        *b ^= key[i % key.len()];
    }
}

/// Set bit `idx` in `buf` using LSB-first packing within each byte
/// (`bit i` lives at `byte (i>>3)`, `mask 1 << (i & 7)`).
pub fn set_bit_le(buf: &mut [u8], idx: usize) {
    buf[idx >> 3] |= 1 << (idx & 7);
}

/// Read bit `idx` from `buf` using LSB-first packing.
pub fn get_bit_le(buf: &[u8], idx: usize) -> bool {
    (buf[idx >> 3] & (1 << (idx & 7))) != 0
}

/// True iff `t` has every bit of `mask` set.
pub fn has_type(t: u8, mask: u8) -> bool {
    t & mask == mask
}

/// Longest common prefix of `a` and `b`, returned as a borrow into `a`.
pub fn common_prefix<'a>(a: &'a [u8], b: &[u8]) -> &'a [u8] {
    let n = a.len().min(b.len());
    for i in 0..n {
        if a[i] != b[i] {
            return &a[..i];
        }
    }
    &a[..n]
}

/// Right-pad `buf` with `pad` bytes so its length is a multiple of `m`.
/// If `buf` is already aligned, returns `buf` unchanged.
pub fn pad_end_to_multiple(mut buf: Vec<u8>, m: usize, pad: u8) -> Vec<u8> {
    let r = buf.len() % m;
    if r == 0 {
        return buf;
    }
    let needed = m - r;
    buf.resize(buf.len() + needed, pad);
    buf
}

/// Tiny cursor with a single error path.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn read(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.pos + n > self.buf.len() {
            return None;
        }
        let out = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Some(out)
    }

    pub fn read_byte(&mut self) -> Option<u8> {
        Some(self.read(1)?[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xor_round_trip() {
        let mut buf = vec![0xaa; 10];
        let key = b"key";
        let original = buf.clone();
        xor_in_place(&mut buf, key);
        xor_in_place(&mut buf, key);
        assert_eq!(buf, original);
    }

    #[test]
    fn xor_empty_key_noop() {
        let mut buf = vec![1, 2, 3];
        let original = buf.clone();
        xor_in_place(&mut buf, &[]);
        assert_eq!(buf, original);
    }

    #[test]
    fn set_get_bit_le() {
        let mut buf = [0u8; 4];
        set_bit_le(&mut buf, 0);
        set_bit_le(&mut buf, 9);
        set_bit_le(&mut buf, 17);
        assert!(get_bit_le(&buf, 0));
        assert!(get_bit_le(&buf, 9));
        assert!(get_bit_le(&buf, 17));
        assert!(!get_bit_le(&buf, 1));
        assert_eq!(buf, [0b0000_0001, 0b0000_0010, 0b0000_0010, 0]);
    }

    #[test]
    fn common_prefix_returns_shared() {
        assert_eq!(common_prefix(b"hello", b"help"), b"hel");
        assert_eq!(common_prefix(b"abc", b"abc"), b"abc");
        assert_eq!(common_prefix(b"abc", b"xyz"), b"" as &[u8]);
        assert_eq!(common_prefix(b"abc", b"ab"), b"ab");
    }

    #[test]
    fn pad_end_aligns_to_multiple() {
        let padded = pad_end_to_multiple(vec![1, 2, 3], 8, 0xaa);
        assert_eq!(padded, vec![1, 2, 3, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa]);
        let already_aligned = pad_end_to_multiple(vec![1; 8], 8, 0);
        assert_eq!(already_aligned, vec![1; 8]);
    }
}
