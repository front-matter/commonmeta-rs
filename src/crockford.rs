//! Generate, encode and decode random base32 identifiers.
//! This encoder/decoder:
//! - uses Douglas Crockford Base32 encoding: https://www.crockford.com/base32.html
//! - is based on: https://github.com/front-matter/base32-url
//! - allows for ISO 7064 checksum
//! - encodes the checksum using only characters in the base32 set
//! - produces string that are URI-friendly (no '=' or '/' for instance)

use rand::Rng;
use std::fmt;

// NO i, l, o or u
const ENCODING_CHARS: &str = "0123456789abcdefghjkmnpqrstvwxyz";

#[derive(Debug)]
pub enum CrockfordError {
    InvalidCharacter(char),
    InvalidChecksum(String, u8),
    InvalidChecksumFormat(String),
}

impl fmt::Display for CrockfordError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CrockfordError::InvalidCharacter(c) => write!(f, "invalid character: {}", c),
            CrockfordError::InvalidChecksum(s, cs) => {
                write!(f, "wrong checksum {:02} for identifier {}", cs, s)
            }
            CrockfordError::InvalidChecksumFormat(s) => write!(f, "invalid checksum: {}", s),
        }
    }
}

impl std::error::Error for CrockfordError {}

/// Encode a number to a URI-friendly Douglas Crockford base32 string.
/// optionally split with '-' every n characters, pad with zeros to a minimum length,
/// and append a checksum using modulo 97-10 (ISO 7064).
pub fn encode(number: i64, split_every: usize, mut length: usize, checksum: bool) -> String {
    let original_number = number;
    let mut encoded = if number == 0 {
        "0".to_string()
    } else {
        let mut num = number;
        let mut result = String::new();
        while num > 0 {
            let remainder = (num % 32) as usize;
            num /= 32;
            result.insert(0, ENCODING_CHARS.chars().nth(remainder).unwrap());
        }
        result
    };

    if checksum && length > 2 {
        length -= 2;
    }

    if length > 0 && encoded.len() < length {
        encoded = "0".repeat(length - encoded.len()) + &encoded;
    }

    if checksum {
        let computed_checksum = generate_checksum(original_number);
        encoded.push_str(&format!("{:02}", computed_checksum));
    }

    if split_every > 0 {
        let mut result = String::new();
        let mut i = 0;

        while i < encoded.len() {
            let end = std::cmp::min(i + split_every, encoded.len());
            if !result.is_empty() {
                result.push('-');
            }
            result.push_str(&encoded[i..end]);
            i = end;
        }

        encoded = result;
    }

    encoded
}

/// Generate a random Crockford base32 string.
/// optionally split with '-' every n characters, pad with zeros to a minimum length,
/// and append a checksum using modulo 97-10 (ISO 7064).
pub fn generate(mut length: usize, split_every: usize, checksum: bool) -> String {
    if checksum && length < 3 {
        panic!("Invalid 'length'. Must be >= 3 if checksum enabled.");
    }

    // fixes number size, otherwise decoding checksum check will fail
    if checksum {
        length -= 2;
    }

    // generate a random number between 0 and 32^length
    let n = (32_f64).powi(length as i32);
    let number = rand::thread_rng().gen_range(0..n.min(i64::MAX as f64) as i64);

    encode(number, split_every, length, checksum)
}

/// Decode a URI-friendly Douglas Crockford base32 string to a number.
pub fn decode(str: &str, checksum: bool) -> Result<i64, CrockfordError> {
    let normalized = normalize(str);

    let (encoded, cs) = if checksum {
        if normalized.len() < 2 {
            return Err(CrockfordError::InvalidChecksumFormat(normalized.clone()));
        }

        // checksum is the last two characters
        let cs_str = &normalized[normalized.len() - 2..];
        match cs_str.parse::<u8>() {
            Ok(cs) => (&normalized[..normalized.len() - 2], Some(cs)),
            Err(_) => return Err(CrockfordError::InvalidChecksumFormat(cs_str.to_string())),
        }
    } else {
        (&normalized[..], None)
    };

    let mut number: i64 = 0;
    for c in encoded.chars() {
        number *= 32;
        match ENCODING_CHARS.find(c) {
            Some(pos) => number += pos as i64,
            None => return Err(CrockfordError::InvalidCharacter(c)),
        }
    }

    if let Some(cs) = cs {
        if !validate(number, cs as i64) {
            return Err(CrockfordError::InvalidChecksum(str.to_string(), cs));
        }
    }

    Ok(number)
}

/// Normalize returns a normalized encoded string for base32 encoding.
pub fn normalize(str: &str) -> String {
    str.to_string()
        .to_lowercase()
        .replace("-", "")
        .replace("i", "1")
        .replace("l", "1")
        .replace("o", "0")
}

/// Validate returns true if the encoded string is a valid base32 string with checksum.
pub fn validate(number: i64, checksum: i64) -> bool {
    checksum == generate_checksum(number)
}

/// GenerateChecksum returns the checksum for a number using ISO 7064 (mod 97-10).
pub fn generate_checksum(number: i64) -> i64 {
    97 - ((100 * number) % 97) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode() {
        let number = 12345;
        let encoded = encode(number, 0, 0, false);
        let decoded = decode(&encoded, false).unwrap();
        assert_eq!(number, decoded);
    }

    #[test]
    fn test_with_checksum() {
        let number = 12345;
        let encoded = encode(number, 0, 0, true);
        let decoded = decode(&encoded, true).unwrap();
        assert_eq!(number, decoded);
    }
}
