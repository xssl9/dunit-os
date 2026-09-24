//! TrueType/OpenType container parsing: the big-endian byte reader, the table
//! directory, and the specific tables the rasterizer needs.

use crate::{Result, TextError};

/// A cursor over a byte slice reading big-endian integers (the byte order of
/// every scalar in the SFNT format).
#[derive(Clone, Copy)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    pub fn at(data: &'a [u8], pos: usize) -> Self {
        Reader { data, pos }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.pos = self.pos.checked_add(n).ok_or(TextError::OutOfBounds)?;
        if self.pos > self.data.len() {
            return Err(TextError::UnexpectedEof);
        }
        Ok(())
    }

    pub fn u8(&mut self) -> Result<u8> {
        let b = *self.data.get(self.pos).ok_or(TextError::UnexpectedEof)?;
        self.pos += 1;
        Ok(b)
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(((self.u8()? as u16) << 8) | self.u8()? as u16)
    }

    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(((self.u16()? as u32) << 16) | self.u16()? as u32)
    }

    /// Read a 4-byte tag as raw bytes.
    pub fn tag(&mut self) -> Result<[u8; 4]> {
        Ok([self.u8()?, self.u8()?, self.u8()?, self.u8()?])
    }
}

/// The parsed SFNT table directory: a view over the whole font file plus the
/// located byte ranges of the tables this crate consumes.
pub struct Sfnt<'a> {
    data: &'a [u8],
    /// (tag, offset, length) for every table in the directory.
    records: alloc::vec::Vec<([u8; 4], u32, u32)>,
}

impl<'a> Sfnt<'a> {
    /// Parse the offset table + table directory. Rejects TrueType Collections
    /// (`ttcf`) and PostScript-outline (`OTTO`/CFF) fonts, which v1 cannot use.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let sfnt_version = r.u32()?;
        // 0x00010000 = TrueType outlines; 'true'/'typ1' also carry glyf.
        // 'OTTO' carries CFF outlines we do not decode; 'ttcf' is a collection.
        match sfnt_version {
            0x0001_0000 | 0x7472_7565 | 0x7479_7031 => {}
            0x4F54_544F => return Err(TextError::CffUnsupported),
            _ => return Err(TextError::UnsupportedFormat("sfnt version")),
        }
        let num_tables = r.u16()?;
        r.skip(6)?; // searchRange, entrySelector, rangeShift
        let mut records = alloc::vec::Vec::with_capacity(num_tables as usize);
        for _ in 0..num_tables {
            let tag = r.tag()?;
            let _checksum = r.u32()?;
            let offset = r.u32()?;
            let length = r.u32()?;
            records.push((tag, offset, length));
        }
        Ok(Sfnt { data, records })
    }

    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Return the byte slice of the table with `tag`, if present and in bounds.
    pub fn table(&self, tag: &[u8; 4]) -> Option<&'a [u8]> {
        let (_, off, len) = self.records.iter().copied().find(|(t, _, _)| t == tag)?;
        let start = off as usize;
        let end = start.checked_add(len as usize)?;
        self.data.get(start..end)
    }

    /// Like [`Sfnt::table`] but returns a typed error for a required table.
    pub fn require(&self, tag: &'static [u8; 4]) -> Result<&'a [u8]> {
        self.table(tag).ok_or(TextError::MissingTable(tag_name(tag)))
    }

    /// The `(start, end)` byte range of a table within the whole font file.
    pub fn table_range(&self, tag: &[u8; 4]) -> Option<(usize, usize)> {
        let (_, off, len) = self.records.iter().copied().find(|(t, _, _)| t == tag)?;
        let start = off as usize;
        let end = start.checked_add(len as usize)?;
        if end <= self.data.len() {
            Some((start, end))
        } else {
            None
        }
    }

    /// [`Sfnt::table_range`] for a required table, with a typed error.
    pub fn require_range(&self, tag: &'static [u8; 4]) -> Result<(usize, usize)> {
        self.table_range(tag).ok_or(TextError::MissingTable(tag_name(tag)))
    }
}

fn tag_name(tag: &[u8; 4]) -> &'static str {
    match tag {
        b"head" => "head",
        b"maxp" => "maxp",
        b"hhea" => "hhea",
        b"hmtx" => "hmtx",
        b"cmap" => "cmap",
        b"loca" => "loca",
        b"glyf" => "glyf",
        _ => "table",
    }
}
