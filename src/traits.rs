//! Reader and Writer traits for format modules.

use crate::data::Data;
use crate::error::Result;

/// A format that can deserialize records into `Data`.
pub trait Reader {
    /// Parse `input` (a JSON/XML/URL string) into a `Data` record.
    fn read(input: &str) -> Result<Data>
    where
        Self: Sized;
}

/// A format that can serialize `Data` into bytes.
pub trait Writer {
    /// Serialize `data` into the target format.
    fn write(data: &Data) -> Result<Vec<u8>>
    where
        Self: Sized;
}
