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
    pub alloc_mask: MLAllocMask,
    // pub alloc_mask: [u64; 1024],
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
            alloc_mask: MLAllocMask::default(),
            report_timestamp: AtomicU64::new(current_time),
            locked: AtomicBool::new(false),
        }
    }
}

#[derive(Debug)]
pub struct MLAllocMask {
    f: u16,
    s: [u64; 16],
    t: [u64; 1024],
}

impl Default for MLAllocMask {
    fn default() -> Self {
        Self { f: 0, s: [0; 16], t: [0; 1024] }
    }
}

impl MLAllocMask {
    pub fn find_available_id(&self) -> Option<u16> {
        // Find the first available bit in the first level
        let f = (!self.f).trailing_zeros() as u16;

        if f >= 16 { None }
        else {
            // Find the next available bit in the second level
            let s = (!self.s[f as usize]).trailing_zeros() as u16;

            if s >= 64 { None }
            else {
                // Finally find the next available bit in the third level
                let i = f * 64 + s;
                let t = (!self.t[i as usize]).trailing_zeros() as u16;

                if t < 64 {
                    Some(i * 64 + t)
                } else {
                    None
                }
            }
        }
    }

    pub fn reserve_id(&mut self, id: u16) {
        let id = id as usize;
        let t = id >> 6;
        let i = id % 64;

        self.t[t] |= 1 << i;
        if self.t[t] == u64::MAX {
            let s = t >> 6;
            let i = t % 64;

            self.s[s] |= 1 << i;
            if self.s[s] == u64::MAX {
                self.f |= 1 << s;
            }
        }
    }

    pub fn free_id(&mut self, id: u16) {
        let id = id as usize;

        let t = id >> 6;
        let i = id % 64;
        self.t[t] &= !(1 << i);

        let s = t >> 6;
        let i = t % 64;
        self.s[s] &= !(1 << i);

        self.f &= !(1 << s);
    }
}

#[test]
fn the_alloc_mask_test() {
    let mut alloc_mask = MLAllocMask::default();
    let count = u16::MAX as usize + 1;

    for i in 0..count {
        assert_eq!(alloc_mask.find_available_id(), Some(i as u16));
        alloc_mask.reserve_id(i as u16);
    }

    assert_eq!(alloc_mask.find_available_id(), None);

    alloc_mask.free_id(0);
    assert_eq!(alloc_mask.find_available_id(), Some(0));
    alloc_mask.reserve_id(0);

    alloc_mask.free_id(67);     // 😳
    assert_eq!(alloc_mask.find_available_id(), Some(67));
    alloc_mask.reserve_id(67);

    assert_eq!(alloc_mask.find_available_id(), None);

    for i in 0..count {
        alloc_mask.free_id(i as u16);
    }

    assert_eq!(alloc_mask.f, 0);
}


pub fn typeid_to_u64(id: TypeId) -> u64 {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}
