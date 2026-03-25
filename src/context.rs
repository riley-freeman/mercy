use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::mem::{self, take};
use std::process::exit;
use std::sync::{Arc, MutexGuard};
use std::{collections, sync::LazyLock};

use crate::pal::DispatchContext;
#[cfg(target_os = "macos")]
use crate::pal::apple::AppleContext;
#[cfg(target_os = "ios")]
use crate::pal::apple::AppleContext;
#[cfg(target_os = "linux")]
use crate::pal::posix::PosixContext;

use super::error;
use super::mapping;

static PROCESS_CONTEXTS: LazyLock<std::sync::Mutex<collections::HashMap<usize, WeakContext>>> =
    LazyLock::new(|| std::sync::Mutex::new(collections::HashMap::new()));

pub struct ContextBuilder {
    id: String,
    roles: HashMap<String, Box<dyn FnOnce(Result<Context, error::Error>) -> ()>>,
}

impl std::fmt::Debug for ContextBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextBuilder")
            .field("id", &self.id)
            .field("roles", &self.roles.keys())
            .finish()
    }
}

impl ContextBuilder {
    pub fn new(id: &str) -> Self {
        ContextBuilder {
            id: String::from(id),
            roles: HashMap::new(),
        }
    }

    pub fn id(mut self, id: &str) -> Self {
        self.id = String::from(id);
        self
    }

    pub fn main(
        mut self,
        main: impl FnOnce(Result<Context, error::Error>) -> () + 'static,
    ) -> Self {
        self.roles.insert("main".to_string(), Box::new(main));
        self
    }

    pub fn add_role(
        mut self,
        role: &str,
        main: impl FnOnce(Result<Context, error::Error>) -> () + 'static,
    ) -> Self {
        if role.eq("main") || role.eq("manager") {
            panic!(
                "{}",
                crate::error::Error::RoleNameReserved {
                    name: role.to_string()
                }
            );
        }

        self.roles.insert(role.to_string(), Box::new(main));
        self
    }

    pub fn start(mut self) -> ! {
        let role_name = std::env::var("CRAYON_MERCY_ROLE_NAME").unwrap_or(String::from("main"));
        if role_name.eq("manager") {
            #[cfg(target_os = "linux")]
            crate::pal::posix::server::start_server(&self.id).unwrap();
            exit(0);
        }

        let take_ownership = if role_name.eq("main") { true } else { false };

        let context = match ContextInner::new(&self.id) {
            Ok(context) => Ok(context),
            // Open if manager is already running
            Err(error::Error::IdAlreadyExists { id: _ }) => {
                ContextInner::open(&self.id, take_ownership)
            }
            Err(e) => Err(e),
        };

        let role = self
            .roles
            .remove(&role_name)
            .expect(&format!("Role {} not found", role_name));
        role(context);
        exit(0)
    }
}

#[derive(Debug)]
pub struct ContextInner {
    id: String,
    id_hash: u64,
    dispatch: std::boxed::Box<dyn DispatchContext>,
}

impl ContextInner {
    fn new(id: &str) -> Result<Context, error::Error> {
        let id_hash = hash_id(id);

        #[cfg(target_os = "macos")]
        let dispatch = std::boxed::Box::new(AppleContext::new(id_hash));
        #[cfg(target_os = "ios")]
        let dispatch = std::boxed::Box::new(AppleContext::new(id_hash));
        #[cfg(target_os = "macos")]
        let dispatch = std::boxed::Box::new(AppleContext::new(id_hash));
        #[cfg(target_os = "linux")]
        let dispatch = std::boxed::Box::new(PosixContext::new(id, id_hash)?);

        let context_inner = ContextInner {
            id: String::from(id),
            id_hash,
            dispatch,
        };

        let context = Context {
            inner: std::sync::Arc::new(std::sync::Mutex::new(context_inner)),
            id: String::from(id),
            id_hash,
        };

        // Add the context to the process contexts
        let mut guard = PROCESS_CONTEXTS.lock().unwrap();
        register_context(&mut guard, &context);

        Ok(context)
    }

    fn open(id: &str, take_ownership: bool) -> Result<Context, error::Error> {
        let id_hash = hash_id(id);

        #[cfg(target_os = "macos")]
        let dispatch = std::boxed::Box::new(AppleContext::new(id_hash));
        #[cfg(target_os = "ios")]
        let dispatch = std::boxed::Box::new(AppleContext::new(id_hash));
        #[cfg(target_os = "linux")]
        let dispatch = std::boxed::Box::new(PosixContext::open(id, id_hash, take_ownership)?);

        let context_inner = ContextInner {
            id: String::from(id),
            id_hash,
            dispatch,
        };

        let context = Context {
            inner: std::sync::Arc::new(std::sync::Mutex::new(context_inner)),
            id: String::from(id),
            id_hash,
        };

        // Add the context to the process contexts
        let mut guard = PROCESS_CONTEXTS.lock().unwrap();
        register_context(&mut guard, &context);

        Ok(context)
    }
}

#[derive(Debug, Clone)]
pub struct Context {
    inner: std::sync::Arc<std::sync::Mutex<ContextInner>>,
    id: String,
    id_hash: u64,
}
type WeakContext = std::sync::Weak<std::sync::Mutex<ContextInner>>;

impl Context {
    pub fn id(&self) -> String {
        self.id.clone()
    }
}

impl crate::alloc::Allocator for Context {
    fn alloc(&mut self, size: u32) -> Result<u128, error::Error> {
        self.inner.lock().unwrap().dispatch.alloc(size)
    }

    fn free(&mut self, id: u128) {
        self.inner.lock().unwrap().dispatch.free(id)
    }

    fn map_id(&mut self, id: u128) -> Result<*mut u8, error::Error> {
        self.inner.lock().unwrap().dispatch.map_id(id)
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

pub fn lock_context_database<'a>() -> MutexGuard<'a, HashMap<usize, WeakContext>> {
    PROCESS_CONTEXTS.lock().unwrap()
}

fn register_context(guard: &mut MutexGuard<HashMap<usize, WeakContext>>, context: &Context) {
    let id_hash = context.inner.lock().unwrap().id_hash;
    if guard.contains_key(&(id_hash as usize)) {
        return; // Start praying.
    }
    guard.insert(id_hash as usize, std::sync::Arc::downgrade(&context.inner));
}

fn check_locked_contexts(
    guard: &MutexGuard<HashMap<usize, WeakContext>>,
    id_hash: u64,
) -> Option<Context> {
    if let Some(context) = guard.get(&(id_hash as _)) {
        if let Some(context) = context.upgrade() {
            let context_inner = context.lock().unwrap();

            let id = context_inner.id.clone();
            let id_hash = context_inner.id_hash;

            std::mem::drop(context_inner);

            let context = Context {
                inner: context,
                id,
                id_hash,
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
