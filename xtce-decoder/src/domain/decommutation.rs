use crate::domain::errors::XtceError;
use crate::domain::models::{ParameterType, SequenceContainer, XtceDb};
use crate::proto::{ParameterValue, parameter_value::Value};

/// Unpacks a big-endian unsigned integer of arbitrary bit size.
pub fn extract_uint(buffer: &[u8], start_offset_bits: usize, length_bits: usize) -> Result<u64, XtceError> {
    if length_bits == 0 {
        return Ok(0);
    }
    if length_bits > 64 {
        return Err(XtceError::DecommutationFailed(format!("Unsigned integer size too large: {length_bits} bits")));
    }
    let end_bit = start_offset_bits + length_bits;
    let required_bytes = (end_bit + 7) / 8;
    if buffer.len() < required_bytes {
        return Err(XtceError::DecommutationFailed(format!(
            "Buffer underflow: requested bit range {start_offset_bits}-{end_bit} ({required_bytes} bytes required), but buffer has only {} bytes",
            buffer.len()
        )));
    }

    let mut val: u64 = 0;
    for i in 0..length_bits {
        let bit_index = start_offset_bits + i;
        let byte_pos = bit_index / 8;
        let bit_pos = 7 - (bit_index % 8); // MSB is bit 7, LSB is bit 0
        let bit = (buffer[byte_pos] >> bit_pos) & 1;
        val = (val << 1) | (bit as u64);
    }
    Ok(val)
}

/// Unpacks a big-endian signed integer of arbitrary bit size (handling two's complement).
pub fn extract_int(buffer: &[u8], start_offset_bits: usize, length_bits: usize) -> Result<i64, XtceError> {
    let uint_val = extract_uint(buffer, start_offset_bits, length_bits)?;
    if length_bits == 0 {
        return Ok(0);
    }
    let sign_bit_mask = 1u64 << (length_bits - 1);
    if (uint_val & sign_bit_mask) != 0 {
        if length_bits == 64 {
            Ok(uint_val as i64)
        } else {
            let extended_val = uint_val | (!0u64 << length_bits);
            Ok(extended_val as i64)
        }
    } else {
        Ok(uint_val as i64)
    }
}

/// Unpacks standard IEEE-754 32/64 bit floats.
pub fn extract_float(buffer: &[u8], start_offset_bits: usize, length_bits: usize) -> Result<f64, XtceError> {
    if length_bits == 32 {
        let raw_bits = extract_uint(buffer, start_offset_bits, 32)? as u32;
        Ok(f32::from_bits(raw_bits) as f64)
    } else if length_bits == 64 {
        let raw_bits = extract_uint(buffer, start_offset_bits, 64)?;
        Ok(f64::from_bits(raw_bits))
    } else {
        Err(XtceError::DecommutationFailed(format!(
            "Unsupported float length: {length_bits} bits (only 32 and 64 bit IEEE-754 supported)"
        )))
    }
}

/// Unpacks boolean parameters.
pub fn extract_bool(buffer: &[u8], start_offset_bits: usize, length_bits: usize) -> Result<bool, XtceError> {
    let raw = extract_uint(buffer, start_offset_bits, length_bits)?;
    Ok(raw != 0)
}

/// Unpacks fixed-size or null-terminated strings.
pub fn extract_string(buffer: &[u8], start_offset_bits: usize, length_bits: usize) -> Result<String, XtceError> {
    if length_bits % 8 != 0 {
        return Err(XtceError::DecommutationFailed(format!(
            "String field length must be multiple of 8 bits, got {length_bits}"
        )));
    }
    let length_bytes = length_bits / 8;
    let mut bytes = Vec::with_capacity(length_bytes);
    for i in 0..length_bytes {
        let b = extract_uint(buffer, start_offset_bits + i * 8, 8)? as u8;
        if b == 0 {
            break; // Null terminator
        }
        bytes.push(b);
    }
    String::from_utf8(bytes).map_err(|e| {
        XtceError::DecommutationFailed(format!("Invalid UTF-8 string: {e}"))
    })
}

/// Unpacks raw binary buffers.
pub fn extract_binary(buffer: &[u8], start_offset_bits: usize, length_bits: usize) -> Result<Vec<u8>, XtceError> {
    if length_bits % 8 != 0 {
        return Err(XtceError::DecommutationFailed(format!(
            "Binary field length must be multiple of 8 bits, got {length_bits}"
        )));
    }
    let length_bytes = length_bits / 8;
    let mut bytes = Vec::with_capacity(length_bytes);
    for i in 0..length_bytes {
        let b = extract_uint(buffer, start_offset_bits + i * 8, 8)? as u8;
        bytes.push(b);
    }
    Ok(bytes)
}

#[derive(Debug, Clone)]
pub struct DecommutatedParameter {
    pub name: String,
    pub raw_value: ParameterValue,
}

pub struct DecommutationEngine;

impl DecommutationEngine {
    /// Walk through sequence container entries and extract parameters.
    pub fn decommute(
        payload: &[u8],
        container: &SequenceContainer,
        db: &XtceDb,
    ) -> Result<Vec<DecommutatedParameter>, XtceError> {
        let mut results = Vec::new();

        for entry in &container.entries {
            let parameter = db.parameters.get(&entry.parameter_name).ok_or_else(|| {
                XtceError::DecommutationFailed(format!(
                    "Parameter '{}' referenced in container '{}' is missing in ParameterSet",
                    entry.parameter_name, container.name
                ))
            })?;

            let value = match parameter.param_type {
                ParameterType::Uint => {
                    let val = extract_uint(payload, entry.start_offset_bits, entry.length_bits)?;
                    Value::IntValue(val as i64)
                }
                ParameterType::Int => {
                    let val = extract_int(payload, entry.start_offset_bits, entry.length_bits)?;
                    Value::IntValue(val)
                }
                ParameterType::Float => {
                    let val = extract_float(payload, entry.start_offset_bits, entry.length_bits)?;
                    Value::FloatValue(val)
                }
                ParameterType::Boolean => {
                    let val = extract_bool(payload, entry.start_offset_bits, entry.length_bits)?;
                    Value::BoolValue(val)
                }
                ParameterType::String => {
                    let val = extract_string(payload, entry.start_offset_bits, entry.length_bits)?;
                    Value::StringValue(val)
                }
                ParameterType::Binary => {
                    let val = extract_binary(payload, entry.start_offset_bits, entry.length_bits)?;
                    Value::BytesValue(val)
                }
            };

            results.push(DecommutatedParameter {
                name: entry.parameter_name.clone(),
                raw_value: ParameterValue { value: Some(value) },
            });
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_uint() {
        let buffer = [0b1010_1100, 0b0011_1111];
        // 12-bit unsigned from offset 0
        let val1 = extract_uint(&buffer, 0, 12).unwrap();
        assert_eq!(val1, 0b1010_1100_0011); // 2755

        // 4-bit unsigned from offset 12
        let val2 = extract_uint(&buffer, 12, 4).unwrap();
        assert_eq!(val2, 0b1111); // 15
    }

    #[test]
    fn test_extract_int() {
        let buffer = [0b1111_0001]; // -15 signed 8-bit
        let val1 = extract_int(&buffer, 0, 8).unwrap();
        assert_eq!(val1, -15);

        let buffer2 = [0b0000_1111]; // +15 signed 8-bit
        let val2 = extract_int(&buffer2, 0, 8).unwrap();
        assert_eq!(val2, 15);
    }

    #[test]
    fn test_complex_cross_byte_boundaries() {
        let buffer = [0b1010_1100, 0b0011_1111, 0b1100_0000];
        
        // 1. 3-bit uint starting at offset 5: expected 4
        let val1 = extract_uint(&buffer, 5, 3).unwrap();
        assert_eq!(val1, 4);

        // 2. 5-bit uint starting at offset 8: expected 7
        let val2 = extract_uint(&buffer, 8, 5).unwrap();
        assert_eq!(val2, 7);

        // 3. 11-bit uint starting at offset 13: expected 1984
        let val3 = extract_uint(&buffer, 13, 11).unwrap();
        assert_eq!(val3, 1984);

        // 4. 17-bit uint starting at offset 3: expected 50172
        let val4 = extract_uint(&buffer, 3, 17).unwrap();
        assert_eq!(val4, 50172);
    }

    #[test]
    fn test_malformed_payload_underflow() {
        let buffer = [0b1010_1100]; // Only 1 byte (8 bits)
        
        // Requesting 12 bits from offset 0 should fail with Buffer underflow
        let res = extract_uint(&buffer, 0, 12);
        assert!(res.is_err());
        match res {
            Err(XtceError::DecommutationFailed(msg)) => {
                assert!(msg.contains("Buffer underflow"));
            }
            _ => panic!("Expected DecommutationFailed error"),
        }
    }
}
