use std::{any::TypeId, hash::{Hash, Hasher, DefaultHasher}, sync::atomic::{AtomicBool, AtomicU64}};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub major: u16,
    pub minor: u8,
    pub patch: u8,
}

#[allow(unused)]
#[derive(Debug)]
pub struct MercyHeader {
    pub signature: [u8; 12],
    pub version: Version,
    pub alloc_mask: [u64; 1024],
    pub report_timestamp: AtomicU64,
    pub locked: AtomicBool,

    // TODO: mappings
    // TODO: accessing
    // TODO: root_message
    // TODO: state
    // TODO: broadcasts
}

impl Default for MercyHeader {
    fn default() -> Self {
        // Get the current time
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        MercyHeader {
            signature: *b"9989MURC_IPM",
            version: crate::VERSION.clone(),
            alloc_mask: [0; 1024],
            report_timestamp: AtomicU64::new(current_time),
            locked: AtomicBool::new(false),
        }
    }
}

pub fn typeid_to_u64(id: TypeId) -> u64 {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}
