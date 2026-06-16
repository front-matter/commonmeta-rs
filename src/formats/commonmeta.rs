use crate::data::Data;
use crate::error::{Error, Result};

pub fn read(json: &str) -> Result<Data> {
    serde_json::from_str(json).map_err(|e| Error::Parse(e.to_string()))
}

pub fn write(data: &Data) -> Result<Vec<u8>> {
    serde_json::to_vec(data).map_err(|e| Error::Serialize(e.to_string()))
}
