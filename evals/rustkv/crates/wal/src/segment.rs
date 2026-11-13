use std::fs::File;
use std::io::{self, Write};

pub struct WalSegment {
    file: File,
    committed_offset: u64,
}

impl WalSegment {
    pub fn new(file: File) -> Self {
        Self { file, committed_offset: 0 }
    }

    pub fn append_record(&mut self, payload: &[u8]) -> io::Result<u64> {
        self.file.write_all(payload)?;
        self.sync_data()?;
        self.committed_offset += payload.len() as u64;
        Ok(self.committed_offset)
    }

    pub fn sync_data(&mut self) -> io::Result<()> {
        self.file.sync_data()
    }
}
