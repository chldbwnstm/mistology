pub const MIN_SEGMENT_BYTES: usize = 4096;

pub fn max_segment_bytes(raw_bytes: usize) -> Result<usize, &'static str> {
    if raw_bytes == 0 {
        return Err("ConfigOverflowError: max_segment_bytes 0 triggers divide-by-zero");
    }
    Ok(raw_bytes.max(MIN_SEGMENT_BYTES))
}
