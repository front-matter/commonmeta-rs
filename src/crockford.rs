/*
Copyright © 2025 Front Matter <info@front-matter.io>
*/

use base32::{decode, encode, Alphabet};

// NO i, l, o or u
const ENCODING_CHARS: &str = "0123456789abcdefghjkmnpqrstvwxyz";

/// Generate, encode and decode random base32 identifiers.
/// This encoder/decoder:
/// - uses Douglas Crockford Base32 encoding: https://www.crockford.com/base32.html
/// - allows for ISO 7064 checksum
/// - encodes the checksum using only characters in the base32 set
/// - produces string that are URI-friendly (no '=' or '/' for instance)
/// This is based on: https://github.com/front-matter/base32-url
///
/// # Arguments
///
/// * `number` - The number to encode
/// * `split_every` - Split the output string every n characters with a dash
/// * `length` - Pad the output string with leading zeros to reach this length
/// * `checksum` - Whether to append an ISO 7064 checksum
pub fn encode_number(number: i64, split_every: usize, length: usize, checksum: bool) -> String {
    let original_number = number;

    // Convert number to bytes
    let bytes = number.to_be_bytes();

    // Use the base32 crate to encode
    let mut encoded = encode(Alphabet::Crockford, &bytes);

    // Trim leading zeros but make sure we don't end up with an empty string
    encoded = encoded.trim_start_matches('0').to_string();
    if encoded.is_empty() {
        encoded = "0".to_string();
    }

    // Apply padding to meet minimum length
    let mut final_length = length;
    if checksum && final_length > 2 {
        final_length -= 2;
    }

    if final_length > 0 && encoded.len() < final_length {
        encoded = "0".repeat(final_length - encoded.len()) + &encoded;
    }

    // Add checksum if requested
    if checksum {
        let computed_checksum = generate_checksum(original_number);
        encoded += &format!("{:02}", computed_checksum);
    }

    // Add dashes if requested
    if split_every > 0 {
        let mut splits = Vec::new();
        let mut i = 0;
        while i < encoded.len() {
            let end = std::cmp::min(i + split_every, encoded.len());
            splits.push(&encoded[i..end]);
            i = end;
        }
        encoded = splits.join("-");
    }

    encoded
}

/// Generate a random Crockford base32 string.
/// Optionally split with '-' every n characters, pad with zeros to a minimum length,
/// and append a checksum using modulo 97-10 (ISO 7064).
pub fn generate(length: usize, split_every: usize, checksum: bool) -> String {
    if checksum && length < 3 {
        panic!("Invalid 'length'. Must be >= 3 if checksum enabled.");
    }

    // fixes number size, otherwise decoding checksum check will fail
    let adjusted_length = if checksum { length - 2 } else { length };

    // generate a random number between 0 and 32^length
    use rand::{thread_rng, Rng};
    let max = (32_u64).pow(adjusted_length as u32);
    let number = thread_rng().gen_range(0..max) as i64;

    encode_number(number, split_every, adjusted_length, checksum)
}

/// Decode a URI-friendly Douglas Crockford base32 string to a number.
pub fn decode_to_number(s: &str, checksum: bool) -> Result<i64, String> {
    let mut encoded = normalize(s);
    let mut cs: i64 = 0;

    // Handle checksum
    if checksum {
        // checksum is the last two characters
        if encoded.len() < 2 {
            return Err(format!("input string too short for checksum: {}", s));
        }

        let checksum_str = &encoded[encoded.len() - 2..];
        cs = match checksum_str.parse::<i64>() {
            Ok(num) => num,
            Err(_) => return Err(format!("invalid checksum: {}", checksum_str)),
        };

        encoded = encoded[..encoded.len() - 2].to_string();
    }

    // Ensure we have enough padding for base32 decoding
    let padding_needed = encoded.len() % 8;
    if padding_needed != 0 {
        encoded = "0".repeat(8 - padding_needed) + &encoded;
    }

    // Use the base32 crate to decode
    let bytes = decode(Alphabet::Crockford, &encoded)
        .map_err(|e| format!("error during Base32-decoding: {}", e))?;

    // Convert bytes to i64
    let mut number: i64 = 0;
    for byte in bytes {
        number = (number << 8) | byte as i64;
    }

    // Validate checksum if needed
    if checksum && !validate(number, cs) {
        return Err(format!("wrong checksum {} for identifier {}", cs, s));
    }

    Ok(number)
}

/// Normalize returns a normalized encoded string for base32 encoding.
pub fn normalize(s: &str) -> String {
    s.to_lowercase()
        .replace("-", "")
        .replace("i", "1")
        .replace("l", "1")
        .replace("o", "0")
}

/// Validate returns true if the encoded number is a valid base32 string with checksum.
pub fn validate(number: i64, checksum: i64) -> bool {
    generate_checksum(number) == checksum
}

/// Generate checksum for a number using ISO 7064 (mod 97-10).
/// The algorithm computes: 98 - ((100 * number) mod 97)
pub fn generate_checksum(number: i64) -> i64 {
    let mod_result = (100 * number) % 97;
    97 - mod_result + 1
}
