//! The catalog's byte format: LEB128 varints, length-prefixed strings, and a
//! per-service string table that every name in the service points into.

use std::collections::HashMap;

use crate::CatalogError;

/// An interned string: an index into its service's string table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sym(pub(crate) u32);

#[derive(Default)]
pub(crate) struct Writer {
    pub(crate) buf: Vec<u8>,
}

impl Writer {
    pub(crate) fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub(crate) fn uv(&mut self, mut v: u64) {
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.buf.push(byte);
                return;
            }
            self.buf.push(byte | 0x80);
        }
    }

    pub(crate) fn len(&mut self, n: usize) {
        self.uv(n as u64);
    }

    /// A signed varint, zigzag-encoded.
    pub(crate) fn iv(&mut self, v: i64) {
        self.uv(((v << 1) ^ (v >> 63)) as u64);
    }

    pub(crate) fn str(&mut self, s: &str) {
        self.len(s.len());
        self.buf.extend_from_slice(s.as_bytes());
    }

    pub(crate) fn bytes(&mut self, b: &[u8]) {
        self.len(b.len());
        self.buf.extend_from_slice(b);
    }

    pub(crate) fn sym(&mut self, s: Sym) {
        self.uv(u64::from(s.0));
    }

    /// `None` is 0; `Some(s)` is `s + 1`.
    pub(crate) fn opt_sym(&mut self, s: Option<Sym>) {
        self.uv(s.map_or(0, |s| u64::from(s.0) + 1));
    }
}

pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    /// How many strings the current service's table holds; every symbol read
    /// is checked against it, so a corrupt catalog fails to decode instead of
    /// panicking later on a lookup.
    pub(crate) syms: u32,
}

const TRUNCATED: CatalogError = CatalogError::Corrupt("truncated");

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Reader {
            buf,
            pos: 0,
            syms: 0,
        }
    }

    pub(crate) fn done(&self) -> bool {
        self.pos == self.buf.len()
    }

    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    pub(crate) fn u8(&mut self) -> Result<u8, CatalogError> {
        let b = *self.buf.get(self.pos).ok_or(TRUNCATED)?;
        self.pos += 1;
        Ok(b)
    }

    pub(crate) fn uv(&mut self) -> Result<u64, CatalogError> {
        let mut v = 0u64;
        let mut shift = 0u32;
        loop {
            let b = self.u8()?;
            if shift > 63 {
                return Err(CatalogError::Corrupt("varint too long"));
            }
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
        }
    }

    pub(crate) fn u32(&mut self) -> Result<u32, CatalogError> {
        u32::try_from(self.uv()?).map_err(|_| CatalogError::Corrupt("u32 out of range"))
    }

    pub(crate) fn u16(&mut self) -> Result<u16, CatalogError> {
        u16::try_from(self.uv()?).map_err(|_| CatalogError::Corrupt("u16 out of range"))
    }

    pub(crate) fn len(&mut self) -> Result<usize, CatalogError> {
        let n = self.uv()?;
        // No count in a catalog can exceed its remaining bytes.
        if n > (self.buf.len() - self.pos) as u64 {
            return Err(CatalogError::Corrupt("count exceeds the data"));
        }
        Ok(n as usize)
    }

    pub(crate) fn iv(&mut self) -> Result<i64, CatalogError> {
        let u = self.uv()?;
        Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], CatalogError> {
        let end = self.pos.checked_add(n).ok_or(TRUNCATED)?;
        let b = self.buf.get(self.pos..end).ok_or(TRUNCATED)?;
        self.pos = end;
        Ok(b)
    }

    pub(crate) fn str(&mut self) -> Result<&'a str, CatalogError> {
        let n = self.len()?;
        let b = self.take(n)?;
        std::str::from_utf8(b).map_err(|_| CatalogError::Corrupt("invalid UTF-8"))
    }

    pub(crate) fn bytes(&mut self) -> Result<&'a [u8], CatalogError> {
        let n = self.len()?;
        self.take(n)
    }

    pub(crate) fn sym(&mut self) -> Result<Sym, CatalogError> {
        let v = self.u32()?;
        if v >= self.syms {
            return Err(CatalogError::Corrupt("symbol out of range"));
        }
        Ok(Sym(v))
    }

    pub(crate) fn opt_sym(&mut self) -> Result<Option<Sym>, CatalogError> {
        match self.u32()? {
            0 => Ok(None),
            v if v - 1 < self.syms => Ok(Some(Sym(v - 1))),
            _ => Err(CatalogError::Corrupt("symbol out of range")),
        }
    }
}

/// The writer's side of a string table: each distinct string once, in first
/// use order. Symbol 0 is always the empty string.
pub(crate) struct Interner {
    map: HashMap<String, u32>,
    list: Vec<String>,
}

impl Default for Interner {
    fn default() -> Self {
        let mut i = Interner {
            map: HashMap::new(),
            list: Vec::new(),
        };
        i.sym("");
        i
    }
}

impl Interner {
    pub(crate) fn sym(&mut self, s: &str) -> Sym {
        if let Some(&i) = self.map.get(s) {
            return Sym(i);
        }
        let i = self.list.len() as u32;
        self.list.push(s.to_owned());
        self.map.insert(s.to_owned(), i);
        Sym(i)
    }

    pub(crate) fn get(&self, s: Sym) -> &str {
        &self.list[s.0 as usize]
    }

    /// The table: a count, every length, then every string's bytes back to
    /// back, so the reader validates UTF-8 once for the whole table.
    pub(crate) fn write(&self, w: &mut Writer) {
        w.len(self.list.len());
        for s in &self.list {
            w.len(s.len());
        }
        for s in &self.list {
            w.buf.extend_from_slice(s.as_bytes());
        }
    }
}

/// The reader's side: one string holding every entry, and where each ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StrTable {
    text: Box<str>,
    ends: Vec<u32>,
}

impl StrTable {
    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self, CatalogError> {
        let n = r.len()?;
        let mut ends = Vec::with_capacity(n);
        let mut total = 0u32;
        for _ in 0..n {
            total = total
                .checked_add(r.u32()?)
                .ok_or(CatalogError::Corrupt("string table overflow"))?;
            ends.push(total);
        }
        let bytes = r.take(total as usize)?;
        let text =
            std::str::from_utf8(bytes).map_err(|_| CatalogError::Corrupt("invalid UTF-8"))?;
        if !ends.iter().all(|&e| text.is_char_boundary(e as usize)) {
            return Err(CatalogError::Corrupt("string boundary inside a character"));
        }
        r.syms = n as u32;
        Ok(StrTable {
            text: text.into(),
            ends,
        })
    }

    pub(crate) fn get(&self, s: Sym) -> &str {
        let i = s.0 as usize;
        let start = if i == 0 { 0 } else { self.ends[i - 1] as usize };
        &self.text[start..self.ends[i] as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_round_trip() {
        let mut w = Writer::default();
        let values = [
            0u64,
            1,
            127,
            128,
            300,
            16_383,
            16_384,
            u64::from(u32::MAX),
            u64::MAX,
        ];
        for v in values {
            w.uv(v);
        }
        for v in [0i64, -1, 1, i64::MIN, i64::MAX, -12345] {
            w.iv(v);
        }
        let mut r = Reader::new(&w.buf);
        for v in values {
            assert_eq!(r.uv().unwrap(), v);
        }
        for v in [0i64, -1, 1, i64::MIN, i64::MAX, -12345] {
            assert_eq!(r.iv().unwrap(), v);
        }
        assert!(r.done());
    }

    #[test]
    fn a_string_table_round_trips_and_checks_its_symbols() {
        let mut i = Interner::default();
        let a = i.sym("ListBuckets");
        let b = i.sym("é∂");
        assert_eq!(i.sym("ListBuckets"), a);
        let mut w = Writer::default();
        i.write(&mut w);
        w.sym(b);
        w.uv(99);
        let mut r = Reader::new(&w.buf);
        let t = StrTable::read(&mut r).unwrap();
        assert_eq!(t.get(Sym(0)), "");
        assert_eq!(t.get(a), "ListBuckets");
        assert_eq!(t.get(r.sym().unwrap()), "é∂");
        assert!(r.sym().is_err());
    }

    #[test]
    fn a_truncated_buffer_is_an_error_not_a_panic() {
        let mut r = Reader::new(&[0x80, 0x80]);
        assert!(r.uv().is_err());
        let mut r = Reader::new(&[0x05, b'a']);
        assert!(r.str().is_err());
    }
}
