use rustkv_wal::segment::WalSegment;
use std::collections::BTreeMap;
use std::io;

#[derive(Default)]
pub struct MemTable {
    entries: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl MemTable {
    pub fn flush_batch(
        &mut self,
        wal: &mut WalSegment,
        batch: &[(Vec<u8>, Vec<u8>)],
    ) -> io::Result<usize> {
        let mut owned_frame = Vec::new();
        for (k, v) in batch {
            owned_frame.extend_from_slice(&(k.len() as u32).to_le_bytes());
            owned_frame.extend_from_slice(k);
            owned_frame.extend_from_slice(&(v.len() as u32).to_le_bytes());
            owned_frame.extend_from_slice(v);
        }
        wal.append_record(&owned_frame)?;
        wal.sync_data()?;
        for (k, v) in batch {
            self.entries.insert(k.clone(), v.clone());
        }
        Ok(batch.len())
    }
}
