use std::slice::from_raw_parts;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;
use std::mem;

use fst::raw::MmapReadOnly;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::ser::{Serialize, Serializer, SerializeTuple};

use crate::DocIndex;
use crate::data::Data;

#[derive(Debug)]
#[repr(C)]
struct Range {
    start: u64,
    end: u64,
}

#[derive(Clone, Default)]
pub struct DocIndexes {
    ranges: Data,
    indexes: Data,
}

impl DocIndexes {
    pub unsafe fn from_path<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mmap = MmapReadOnly::open_path(path)?;
        DocIndexes::from_data(Data::Mmap(mmap))
    }

    pub fn from_bytes(vec: Vec<u8>) -> io::Result<Self> {
        let len = vec.len();
        DocIndexes::from_shared_bytes(Arc::new(vec), 0, len)
    }

    pub fn from_shared_bytes(bytes: Arc<Vec<u8>>, offset: usize, len: usize) -> io::Result<Self> {
        let data = Data::Shared { bytes, offset, len };
        DocIndexes::from_data(data)
    }

    fn from_data(data: Data) -> io::Result<Self> {
        let ranges_len_offset = data.len() - mem::size_of::<u64>();
        let ranges_len = (&data[ranges_len_offset..]).read_u64::<LittleEndian>()?;
        let ranges_len = ranges_len as usize;

        let ranges_offset = ranges_len_offset - ranges_len;
        let ranges = data.range(ranges_offset, ranges_len);

        let indexes = data.range(0, ranges_offset);

        Ok(DocIndexes { ranges, indexes })
    }

    pub fn get(&self, index: u64) -> Option<&[DocIndex]> {
        self.ranges().get(index as usize).map(|Range { start, end }| {
            let start = *start as usize;
            let end = *end as usize;
            &self.indexes()[start..end]
        })
    }

    fn ranges(&self) -> &[Range] {
        let slice = &self.ranges;
        let ptr = slice.as_ptr() as *const Range;
        let len = slice.len() / mem::size_of::<Range>();
        unsafe { from_raw_parts(ptr, len) }
    }

    fn indexes(&self) -> &[DocIndex] {
        let slice = &self.indexes;
        let ptr = slice.as_ptr() as *const DocIndex;
        let len = slice.len() / mem::size_of::<DocIndex>();
        unsafe { from_raw_parts(ptr, len) }
    }
}

impl Serialize for DocIndexes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tuple = serializer.serialize_tuple(2)?;
        tuple.serialize_element(self.ranges.as_ref())?;
        tuple.serialize_element(self.indexes.as_ref())?;
        tuple.end()
    }
}

pub struct DocIndexesBuilder<W> {
    ranges: Vec<Range>,
    wtr: W,
}

impl DocIndexesBuilder<Vec<u8>> {
    pub fn memory() -> Self {
        DocIndexesBuilder::new(Vec::new())
    }
}

impl<W: Write> DocIndexesBuilder<W> {
    pub fn new(wtr: W) -> Self {
        DocIndexesBuilder {
            ranges: Vec::new(),
            wtr: wtr,
        }
    }

    pub fn insert(&mut self, indexes: &[DocIndex]) -> io::Result<()> {
        let len = indexes.len() as u64;
        let start = self.ranges.last().map(|r| r.end).unwrap_or(0);
        let range = Range { start, end: start + len };
        self.ranges.push(range);

        // write the values
        let indexes = unsafe { into_u8_slice(indexes) };
        self.wtr.write_all(indexes)
    }

    pub fn finish(self) -> io::Result<()> {
        self.into_inner().map(drop)
    }

    pub fn into_inner(mut self) -> io::Result<W> {
        // write the ranges
        let ranges = unsafe { into_u8_slice(self.ranges.as_slice()) };
        self.wtr.write_all(ranges)?;

        // write the length of the ranges
        let len = ranges.len() as u64;
        self.wtr.write_u64::<LittleEndian>(len)?;

        Ok(self.wtr)
    }
}

unsafe fn into_u8_slice<T>(slice: &[T]) -> &[u8] {
    let ptr = slice.as_ptr() as *const u8;
    let len = slice.len() * mem::size_of::<T>();
    from_raw_parts(ptr, len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn serialize_deserialize() -> Result<(), Box<Error>> {
        let a = DocIndex { document_id: 0, attribute: 3, attribute_index: 11 };
        let b = DocIndex { document_id: 1, attribute: 4, attribute_index: 21 };
        let c = DocIndex { document_id: 2, attribute: 8, attribute_index: 2 };

        let mut builder = DocIndexesBuilder::memory();

        builder.insert(&[a])?;
        builder.insert(&[a, b, c])?;
        builder.insert(&[a, c])?;

        let bytes = builder.into_inner()?;
        let docs = DocIndexes::from_bytes(bytes)?;

        assert_eq!(docs.get(0).unwrap(), &[a]);
        assert_eq!(docs.get(1).unwrap(), &[a, b, c]);
        assert_eq!(docs.get(2).unwrap(), &[a, c]);

        Ok(())
    }
}
