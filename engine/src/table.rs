//! Read-only sorted lookup table embedded in the binary by `build.rs`.
//! See `build.rs` for the byte layout.

pub struct Table {
    bytes: &'static [u8],
}

impl Table {
    pub const fn new(bytes: &'static [u8]) -> Self {
        Table { bytes }
    }

    pub fn len(&self) -> usize {
        self.u32_at(0) as usize
    }

    /// Payload of the record whose key equals `key`, if any.
    pub fn get(&self, key: &str) -> Option<&'static [u8]> {
        let key = key.as_bytes();
        let (mut lo, mut hi) = (0usize, self.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            let (k, payload) = self.record(mid);
            match k.cmp(key) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(payload),
            }
        }
        None
    }

    /// Key of the i-th record (sorted order).
    #[allow(dead_code)]
    pub fn key(&self, i: usize) -> &'static str {
        std::str::from_utf8(self.record(i).0).expect("table keys are UTF-8")
    }

    fn record(&self, i: usize) -> (&'static [u8], &'static [u8]) {
        let off = self.u32_at(4 + 4 * i) as usize;
        let klen = u16::from_le_bytes([self.bytes[off], self.bytes[off + 1]]) as usize;
        let bytes: &'static [u8] = self.bytes;
        (&bytes[off + 2..off + 2 + klen], &bytes[off + 2 + klen..])
    }

    fn u32_at(&self, at: usize) -> u32 {
        u32::from_le_bytes(self.bytes[at..at + 4].try_into().unwrap())
    }
}

/// Little-endian readers for record payloads.
pub fn read_u32(p: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(p[at..at + 4].try_into().unwrap())
}

pub fn read_u16(p: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([p[at], p[at + 1]])
}
