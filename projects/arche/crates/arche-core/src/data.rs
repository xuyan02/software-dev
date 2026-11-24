use anyhow::Result;

use crate::hash::{Hash32, ObjectHash, SymbolHash};
use crate::object::ObjectKind;
use crate::source::{SourcePath, SourceSpan};

/// A typed reader over one stored object record.
///
/// The store consumes its own record envelope and supplies `version` and
/// `kind`. Magic, if the backend uses one, is private to the store: loose
/// files have a magic, while pack entries do not. The codec then reads only
/// object fields; it never sees raw record bytes or magic.
pub trait InputObject {
    fn version(&self) -> u8;
    fn kind(&self) -> ObjectKind;

    fn u8(&mut self) -> Result<u8>;
    fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }
    fn u16(&mut self) -> Result<u16>;
    fn u32(&mut self) -> Result<u32>;
    fn u64(&mut self) -> Result<u64>;
    fn u128(&mut self) -> Result<u128>;
    fn i8(&mut self) -> Result<i8>;
    fn i16(&mut self) -> Result<i16>;
    fn i32(&mut self) -> Result<i32>;
    fn i64(&mut self) -> Result<i64>;
    fn i128(&mut self) -> Result<i128>;
    fn f32(&mut self) -> Result<f32>;
    fn f64(&mut self) -> Result<f64>;

    fn hash(&mut self) -> Result<Hash32>;
    fn object(&mut self) -> Result<ObjectHash>;
    fn symbol(&mut self) -> Result<SymbolHash>;
    fn text(&mut self) -> Result<String>;
    fn objects(&mut self) -> Result<Vec<ObjectHash>>;
    fn symbols(&mut self) -> Result<Vec<SymbolHash>>;

    fn source_path(&mut self) -> Result<SourcePath> {
        Ok(SourcePath {
            path: self.symbol()?,
        })
    }

    fn source_span(&mut self) -> Result<SourceSpan> {
        Ok(SourceSpan {
            path: self.symbol()?,
            start: self.u32()?,
            end: self.u32()?,
        })
    }

    fn finish(&self) -> Result<()>;
}

/// A typed writer for one stored object record.
///
/// The codec writes only object fields. `commit` adds whatever envelope the
/// backend needs, including a loose-file magic when applicable, and stores the
/// record under the object hash computed by the codec.
pub trait OutputObject {
    fn version(&self) -> u8;
    fn kind(&self) -> ObjectKind;

    fn u8(&mut self, value: u8);
    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }
    fn u16(&mut self, value: u16);
    fn u32(&mut self, value: u32);
    fn u64(&mut self, value: u64);
    fn u128(&mut self, value: u128);
    fn i8(&mut self, value: i8);
    fn i16(&mut self, value: i16);
    fn i32(&mut self, value: i32);
    fn i64(&mut self, value: i64);
    fn i128(&mut self, value: i128);
    fn f32(&mut self, value: f32);
    fn f64(&mut self, value: f64);

    fn hash(&mut self, value: Hash32);
    fn object(&mut self, value: ObjectHash);
    fn symbol(&mut self, value: SymbolHash);
    fn text(&mut self, value: &str);
    fn objects(&mut self, values: &[ObjectHash]);
    fn symbols(&mut self, values: &[SymbolHash]);

    fn source_path(&mut self, value: SourcePath) {
        self.symbol(value.path);
    }

    fn source_span(&mut self, value: SourceSpan) {
        self.symbol(value.path);
        self.u32(value.start);
        self.u32(value.end);
    }

    fn commit(self: Box<Self>, object_hash: ObjectHash) -> Result<()>;
}
