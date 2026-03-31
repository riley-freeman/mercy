use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::process::exit;
use std::sync::atomic::AtomicUsize;
use std::sync::{Mutex, MutexGuard};
use std::{collections, sync::LazyLock};

use crate::alloc::HasAllocId;
use crate::pal::DispatchContext;
#[cfg(target_os = "macos")]
use crate::pal::apple::AppleContext;
#[cfg(target_os = "ios")]
use crate::pal::apple::AppleContext;
#[cfg(target_os = "linux")]
use crate::pal::posix::PosixContext;
use crate::sync::Arc;
use crate::worker::Worker;

use super::error;

pub static PROCESS_CONTEXTS: LazyLock<
    std::sync::Mutex<collections::HashMap<usize, std::sync::Arc<std::sync::Mutex<ContextInner>>>>,
> = LazyLock::new(|| std::sync::Mutex::new(collections::HashMap::new()));

pub static MANAGER_CONTEXT: LazyLock<std::sync::Mutex<Option<Context>>> =
    LazyLock::new(|| std::sync::Mutex::new(None));

pub struct ContextBuilder {
    id: String,
    roles: HashMap<String, Box<dyn FnOnce(Context) -> ()>>,
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

    pub fn main(mut self, main: impl FnOnce(Context) -> () + 'static) -> Self {
        self.roles.insert("main".to_string(), Box::new(main));
        self
    }

    pub fn add_role(mut self, role: &str, main: impl FnOnce(Context) -> () + 'static) -> Self {
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
        }
        .expect(&format!("Failed to create context"));

        let role = self
            .roles
            .remove(&role_name)
            .expect(&format!("Role {} not found", role_name));

        {
            let mut guard = MANAGER_CONTEXT.lock().unwrap();
            *guard = Some(context);
        }

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

        let inner = std::sync::Arc::new(std::sync::Mutex::new(context_inner));
        let context = Context { id_hash };

        // Add the context to the process contexts
        let mut guard = PROCESS_CONTEXTS.lock().unwrap();
        register_context(&mut guard, id_hash, inner);

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

        let inner = std::sync::Arc::new(std::sync::Mutex::new(context_inner));
        let context = Context { id_hash };

        // Add the context to the process contexts
        let mut guard = PROCESS_CONTEXTS.lock().unwrap();
        register_context(&mut guard, id_hash, inner);

        if take_ownership {
            let mut guard = MANAGER_CONTEXT.lock().unwrap();
            *guard = Some(context);
        }

        Ok(context)
    }
}

static MESSAGE_CALLBACK: LazyLock<
    Mutex<Box<dyn FnMut(serde_value::Value) -> Option<serde_value::Value> + Send + 'static>>,
> = LazyLock::new(|| Mutex::new(Box::new(|_| None)));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Context {
    id_hash: u64,
}

impl Context {
    pub fn from_id(id_hash: u64) -> Option<Self> {
        let guard = PROCESS_CONTEXTS.lock().unwrap();
        if guard.contains_key(&(id_hash as usize)) {
            Some(Context { id_hash })
        } else {
            None
        }
    }

    fn inner(&self) -> std::sync::Arc<std::sync::Mutex<ContextInner>> {
        let guard = PROCESS_CONTEXTS.lock().unwrap();
        guard
            .get(&(self.id_hash as usize))
            .expect("Context not found in global registry")
            .clone()
    }

    pub fn id(&self) -> String {
        self.inner().lock().unwrap().id.clone()
    }

    pub fn close(&self) {
        let _arc = {
            let mut guard = PROCESS_CONTEXTS.lock().unwrap();
            guard.remove(&(self.id_hash as usize))
        };
        // guard is released, _arc drops here safely
    }

    pub fn worker_id() -> u64 {
        std::env::var("CRAYON_MERCY_WORKER_ID")
            .unwrap_or("0".to_string())
            .parse()
            .unwrap()
    }

    pub fn new_worker(&mut self, role: &str, args: Vec<String>) -> Result<Worker, error::Error> {
        let id = self
            .inner()
            .lock()
            .unwrap()
            .dispatch
            .spawn_worker(role, args)?;
        Ok(Worker::new(self.clone(), id))
    }

    pub fn send_message(
        &self,
        worker: &Worker,
        message: impl serde::Serialize,
        callback: impl FnOnce(serde_value::Value) + Send + 'static,
    ) -> Result<(), error::Error> {
        self.inner().lock().unwrap().dispatch.send_message(
            worker,
            serde_value::to_value(message).unwrap(),
            Box::new(callback),
        )
    }

    pub fn set_message_callback(
        &mut self,
        callback: impl FnMut(serde_value::Value) -> Option<serde_value::Value> + Send + 'static,
    ) {
        *MESSAGE_CALLBACK.lock().unwrap() = Box::new(callback);
    }

    pub fn new_mutex<T>(&mut self, data: T) -> Result<crate::sync::Mutex<T>, error::Error> {
        let mutex_id = self.inner().lock().unwrap().dispatch.mutex()?;
        Ok(crate::sync::Mutex {
            mutex_id,
            context_id: self.id_hash,
            data: std::cell::UnsafeCell::new(data),
        })
    }

    pub fn expose_mutex(&mut self, mutex_id: u64) -> Result<(), error::Error> {
        self.inner().lock().unwrap().dispatch.expose_mutex(mutex_id)
    }

    pub fn set_state<T: 'static>(
        &mut self,
        state: Arc<crate::sync::Mutex<T>>,
    ) -> Result<(), error::Error> {
        let allod_id = state.alloc_id();
        self.inner()
            .lock()
            .unwrap()
            .dispatch
            .set_worker_state(allod_id)
    }

    pub fn get_worker_state<T: 'static>(&mut self, worker_id: u64) -> Option<Arc<T>> {
        let id = self
            .inner()
            .lock()
            .unwrap()
            .dispatch
            .get_worker_state(worker_id)
            .ok()??;
        Arc::from_id(id)
    }
}

pub(crate) fn invoke_message_callback(message: serde_value::Value) -> Option<serde_value::Value> {
    (MESSAGE_CALLBACK.lock().unwrap())(message)
}

impl crate::alloc::Allocator for Context {
    fn alloc(&mut self, size: u32) -> Result<u128, error::Error> {
        self.inner().lock().unwrap().dispatch.alloc(size)
    }

    fn free(&mut self, id: u128) {
        self.inner().lock().unwrap().dispatch.free(id)
    }

    fn map_id(&mut self, id: u128) -> Result<*mut u8, error::Error> {
        self.inner().lock().unwrap().dispatch.map_id(id)
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

pub fn lock_context_database<'a>()
-> MutexGuard<'a, HashMap<usize, std::sync::Arc<std::sync::Mutex<ContextInner>>>> {
    PROCESS_CONTEXTS.lock().unwrap()
}

fn register_context(
    guard: &mut MutexGuard<HashMap<usize, std::sync::Arc<std::sync::Mutex<ContextInner>>>>,
    id_hash: u64,
    inner: std::sync::Arc<std::sync::Mutex<ContextInner>>,
) {
    if guard.contains_key(&(id_hash as usize)) {
        return; // Already registered
    }
    guard.insert(id_hash as usize, inner);
}

fn check_locked_contexts(
    guard: &MutexGuard<HashMap<usize, std::sync::Arc<std::sync::Mutex<ContextInner>>>>,
    id_hash: u64,
) -> Option<Context> {
    if guard.contains_key(&(id_hash as _)) {
        Some(Context { id_hash })
    } else {
        None
    }
}

pub fn check_registered_contexts(id_hash: u64) -> Option<Context> {
    let lock = lock_context_database();
    check_locked_contexts(&lock, id_hash)
}

fn unregister_context(
    guard: &mut MutexGuard<HashMap<usize, std::sync::Arc<std::sync::Mutex<ContextInner>>>>,
    id_hash: u64,
) {
    let _ = guard.remove(&(id_hash as usize));
}

fn hash_id(id: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}
