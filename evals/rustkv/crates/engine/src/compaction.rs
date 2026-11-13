use std::sync::Mutex;

pub struct CompactionCoordinator {
    active_keys: Mutex<Vec<Vec<u8>>>,
    manifest_gen: Mutex<u64>,
}

pub fn run_compaction(coord: &CompactionCoordinator) -> u64 {
    let snapshot = {
        let guard = coord.active_keys.lock().expect("active_keys poisoned");
        guard.clone()
    };
    let mut gen = coord.manifest_gen.lock().expect("manifest_gen poisoned");
    if !snapshot.is_empty() {
        *gen += 1;
    }
    *gen
}
