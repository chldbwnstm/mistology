#[allow(non_snake_case)]
pub fn verifyFrameChecksum(frame: &[u8]) -> Result<&[u8], &'static str> {
    if frame.len() < 4 {
        return Err("CrcEndianError: frame shorter than 4-byte trailer");
    }
    let (payload, trailer) = frame.split_at(frame.len() - 4);
    let expected = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    let mut sum = 0u32;
    for &b in payload {
        sum = sum.wrapping_mul(31).wrapping_add(u32::from(b));
    }
    if sum != expected {
        return Err("CrcEndianError: little-endian trailer read as big-endian");
    }
    Ok(payload)
}
