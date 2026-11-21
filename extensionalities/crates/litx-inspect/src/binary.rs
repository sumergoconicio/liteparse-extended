//! Bounded random access: container payloads are skipped, never eagerly loaded.
use crate::xml;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub(crate) struct Source {
    file: File,
    pub len: u64,
    read: usize,
}
impl Source {
    pub fn open(path: &Path) -> xml::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        if len > 256 << 20 {
            return Err("container physical byte cap exceeded".into());
        }
        Ok(Self { file, len, read: 0 })
    }
    pub fn range(&mut self, offset: u64, size: usize) -> xml::Result<Vec<u8>> {
        if size > 16 << 20
            || offset
                .checked_add(size as u64)
                .is_none_or(|end| end > self.len)
        {
            return Err("container truncated or per-range byte cap exceeded".into());
        }
        self.read = self
            .read
            .checked_add(size)
            .ok_or("container read overflow")?;
        if self.read > 64 << 20 {
            return Err("container cumulative read byte cap exceeded".into());
        }
        self.file.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; size];
        self.file.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}

pub(crate) fn slice(bytes: &[u8], offset: usize, len: usize) -> xml::Result<&[u8]> {
    bytes
        .get(offset..offset.checked_add(len).ok_or("binary offset overflow")?)
        .ok_or_else(|| "truncated binary field".into())
}
pub(crate) fn be16(b: &[u8], i: usize) -> xml::Result<u16> {
    Ok(u16::from_be_bytes(slice(b, i, 2)?.try_into()?))
}
pub(crate) fn be32(b: &[u8], i: usize) -> xml::Result<u32> {
    Ok(u32::from_be_bytes(slice(b, i, 4)?.try_into()?))
}
