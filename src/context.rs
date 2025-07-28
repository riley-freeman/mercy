use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::MutexGuard;
use std::{sync::LazyLock, collections};

use super::error;
use super::header::MercyHeader;
use super::mapping;

static PROCESS_CONTEXTS: LazyLock<std::sync::Mutex<collections::HashMap<usize, WeakContext>>> = LazyLock::new(|| {
    std::sync::Mutex::new(collections::HashMap::new())
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBuilder {
    id: String,
}

impl ContextBuilder {
    pub fn new(id: &str) -> Self {
        ContextBuilder {
            id: String::from(id),
        }
    }

    pub fn id(mut self, id: &str) -> Self {
        self.id = String::from(id);
        self
    }

    pub fn build(self) -> Result<Context, error::Error> {
        ContextInner::new(&self.id)
    }

    pub fn open(self) -> Result<Context, error::Error> {
        ContextInner::open(&self.id, false)
    }

    pub fn build_or_open(self) -> Result<Context, error::Error> {
        match ContextInner::new(&self.id) {
            Ok(context) => Ok(context),
            // Open if it already exists
            Err(error::Error::IdAlreadyExists { id: _ }) => { 
                let context = ContextInner::open(&self.id, true)?;
                Ok(context)
            },
            Err(e) => Err(e),
        }
    }
}

#[derive(Debug)]
pub struct ContextInner {
    id: String,
    id_hash: u64,
    header_id: u128,
    mappings: std::collections::HashMap<u128, Box<dyn mapping::Mapping>>,
}

impl ContextInner {
    fn new (id: &str) -> Result<Context, error::Error> {
        let id_hash = hash_id(id);

        // Lock the process contexts
        let mut guard = lock_context_database();


        // Check the process contexts
        if let Some(context) = check_locked_contexts(&guard, id_hash) {
            return Ok(context);
        }

        // Continue with creating the context
        let os_id = construct_os_id(id_hash, 0);

        let size = size_of::<MercyHeader>();
        let mapping = mapping::new_mapping(&os_id, size)?;
        let header_id = (id_hash as u128) << 64 | (size as u128) << 32 | 0_u128 << 16;

        let mut mappings = std::collections::HashMap::new();
        mappings.insert(header_id, mapping);

        let context_inner = ContextInner {
            id: String::from(id),
            id_hash,
            header_id,
            mappings
        };

        let context = Context { 
            inner: std::sync::Arc::new(std::sync::Mutex::new(context_inner)),
            id: String::from(id),
            id_hash,
            header_id,
        };

        // Add the context to the process contexts
        register_context(&mut guard, &context);

        Ok(context)
    }

    fn open(id: &str, take_ownership: bool) -> Result<Context, error::Error> {
        let id_hash = hash_id(id);

        // Lock the process contexts
        let mut guard = lock_context_database();

        // Check the process contexts
        if let Some(context) = check_locked_contexts(&guard, id_hash) {
            return Ok(context);
        }

        // Continue with opening the context
        let os_id = construct_os_id(id_hash, 0);

        let size = size_of::<MercyHeader>();
        let header_id = (id_hash as u128) << 64 | (size as u128) << 32 | 0_u128 << 16;
        let mut mapping = mapping::open_mapping(&os_id)?;

        // Take ownership if requested
        if take_ownership {
            unsafe { mapping.set_ownership(true) };
        }

        let mut mappings = std::collections::HashMap::new();
        mappings.insert(header_id, mapping);

        let context_inner = ContextInner {
            id: String::from(id),
            id_hash,
            header_id,
            mappings,
        };

        let context = Context {
            inner: std::sync::Arc::new(std::sync::Mutex::new(context_inner)),
            id: String::from(id),
            id_hash,
            header_id,
        };

        // Add the context to the process contexts
        register_context(&mut guard, &context);

        Ok(context)
    }
}


#[derive(Debug, Clone)]
pub struct Context {
    inner: std::sync::Arc<std::sync::Mutex<ContextInner>>,
    id: String,
    id_hash: u64,
    header_id: u128,
}
type WeakContext = std::sync::Weak<std::sync::Mutex<ContextInner>>;

impl Context {
    pub fn id(&self) -> String {
        self.id.clone()
    }

    pub fn id_hash(&self) -> u64 {
        self.id_hash
    }

    pub fn header_alloc_id(&self) -> u128 {
        self.header_id
    }
}

impl crate::alloc::Allocator for Context {
    fn alloc(&mut self, size: u32) -> Result<u128, error::Error> {
        let mut c = self.inner.lock().unwrap();

        let header_id = c.header_id;

        // Get the first available bit from the block mask
        let header_mapping = c.mappings.get_mut(&header_id).ok_or(error::Error::OperationUnsupported)?;
        let mercy_header = unsafe { &mut *(header_mapping.ptr_mut() as *mut MercyHeader) };
        let alloc_mask = &mut mercy_header.alloc_mask;

        for i in 0.. alloc_mask.len() {
            let mask = &mut alloc_mask[i];
            if *mask == u64::MAX {
                continue; // This block is full, skip it
            }

            let first_available_bit_index = (!*mask).trailing_zeros();
            let block_id = 64 * i + first_available_bit_index as usize;

            // Create a new mapping
            let os_id = construct_os_id(c.id_hash, block_id as u16);
            let mut mapping = match mapping::new_mapping(&os_id, size as usize) {
                Ok(mapping) => mapping,
                Err(_) => continue,
            };
            unsafe { mapping.set_ownership(true)}

            // Set the bit to 1
            *mask |= 1 << first_available_bit_index;

            // create an allocation ID
            let allocation_id = (c.id_hash as u128) << 64 | (size as u128) << 32 | (block_id as u128) << 16;

            // Add the mapping to the context
            c.mappings.insert(allocation_id, mapping);

            return Ok(allocation_id);
        }

        // Return null if no block is found
        Err(error::Error::NoBlocksAvailable { requested: size as usize })
    }
    
    fn free(&mut self, id: u128) {
        let mut c = self.inner.lock().unwrap();
        let block_id = (id >> 16) as u16;

        let header_id = c.header_id;

        // Get the header mapping
        if let Some(header_mapping) = c.mappings.get_mut(&header_id) {
            let mercy_header = unsafe { &mut *(header_mapping.ptr_mut() as *mut MercyHeader) };
            let alloc_mask = &mut mercy_header.alloc_mask;

            let first_level_index = (block_id / 64) as usize;
            let second_level_index = (block_id % 64) as usize;

            // Set the appropriate bit to 0
            alloc_mask[first_level_index] &= !(1 << second_level_index);
        }

        // Remove the mapping from the context
        let _ = c.mappings.remove(&id);
    } 

    fn map_id(&mut self, id: u128) -> Option<*mut u8> {
        let mut c = self.inner.lock().unwrap();
        let mapping = match c.mappings.get_mut(&id) {
            Some(mapping) => mapping,
            None => {
                // Create the mapping in the context.
                let context_id = (id >> 64) as u64;
                let _size = (id >> 32) as u32;
                let block_id = (id >> 16) as u16;

                let os_id = construct_os_id(context_id, block_id);
                let mut mapping = match mapping::open_mapping(&os_id) {
                    Ok(mapping) => mapping,
                    Err(_) => { return None; }
                };

                // Make ourselves the owner if the context ID matches
                if context_id == c.id_hash {
                    unsafe { mapping.set_ownership(true) }
                }

                c.mappings.insert(id, mapping);
                c.mappings.get_mut(&id).unwrap()
            }
        };
        
        // Return the pointer to the mapping
        Some(mapping.ptr_mut())
    }
}

impl Drop for ContextInner {
    fn drop(&mut self) {
        // Lock the process contexts
        let mut guard = PROCESS_CONTEXTS.lock().unwrap();

        // Unregister the context
        unregister_context(&mut guard, self.id_hash);
    }
}

pub fn lock_context_database<'a>() ->  MutexGuard<'a, HashMap<usize, WeakContext>> {
    PROCESS_CONTEXTS.lock().unwrap()
}

fn register_context(guard: &mut MutexGuard<HashMap<usize, WeakContext>>, context: &Context) {
    let id_hash = context.inner.lock().unwrap().id_hash;
    if guard.contains_key(&(id_hash as usize)) {
        return;     // Start praying.
    }
    guard.insert(id_hash as usize, std::sync::Arc::downgrade(&context.inner));
}

fn check_locked_contexts(guard: &MutexGuard<HashMap<usize, WeakContext>>, id_hash: u64) -> Option<Context> {
    if let Some(context) = guard.get(&(id_hash as _)) {
        if let Some(context) = context.upgrade() {
            let context_inner = context.lock().unwrap();

            let id = context_inner.id.clone();
            let id_hash = context_inner.id_hash;
            let header_id = context_inner.header_id;

            std::mem::drop(context_inner);

            let context = Context {
                inner: context,
                id,
                id_hash,
                header_id
            };

            return Some(context);
        }
    }
    None
}

pub fn check_registered_contexts(id_hash: u64) -> Option<Context> {
    let lock = lock_context_database();
    check_locked_contexts(&lock, id_hash)
}

fn unregister_context(guard: &mut MutexGuard<HashMap<usize, WeakContext>>, id_hash: u64) {
    let _ = guard.remove(&(id_hash as usize));
}

fn hash_id(id: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

fn construct_os_id(id_hash: u64, block_id: u16) -> String {
    let mut os_id = id_hash.to_le_bytes().to_vec();
    let block_id = block_id.to_string();

    os_id.extend_from_slice(block_id.as_bytes());
    unproblematicize_id(&mut os_id);
    String::from_utf8_lossy(&os_id).to_string()
}

#[cfg(unix)]
fn unproblematicize_id(id: &mut [u8]) {
    for v in id {
        let clamp = *v % 66;
        *v = match clamp {
            0..10 => clamp + b'0',
            10..37 => clamp - 10 + b'A',
            37..63 => clamp - 37 + b'a',
            63 => b'.',
            64 => b'_',
            _ => b'-',
        }
    }
}
