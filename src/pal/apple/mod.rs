pub mod xpc;

use crate::{
    alloc::Allocator,
    message::{AllocData, AllocReply, FreeData, MapIdData, Message, MessageType},
    pal::DispatchContext,
};
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use std::{
    any::Any,
    collections::HashMap,
    ffi::CString,
    sync::{Arc, Mutex, MutexGuard},
};
use xpc_sys::{
    dispatch_queue_s, xpc_connection_send_message, xpc_connection_t, xpc_object_t, xpc_shmem_map,
};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct AppleContext {
    xpc_con: xpc_connection_t,
    context_id: u64,
    #[derivative(Debug = "ignore")]
    reply_closures: Arc<Mutex<HashMap<i64, Box<dyn FnOnce(xpc::AppleObject) + Send>>>>,

    #[derivative(Debug = "ignore")]
    _handler: Option<Box<dyn Any>>,
    #[derivative(Debug = "ignore")]
    mappings: Arc<Mutex<HashMap<u128, xpc::AppleObject>>>,
}

unsafe impl Send for AppleContext {}
unsafe impl Sync for AppleContext {}

impl AppleContext {
    pub fn new(context_id: u64) -> Self {
        let reply_closures = Arc::new(Mutex::new(HashMap::new()));
        let mappings = Arc::new(Mutex::new(HashMap::new()));
        let reply_closures_weak = Arc::downgrade(&reply_closures);
        let c_id = CString::new("com.itsjustbox.crayon.mercy.MercyServerXPC")
            .expect("id contains interior null byte");
        let mut apple_context = AppleContext {
            xpc_con: std::ptr::null_mut(),
            context_id,
            reply_closures,
            _handler: None,
            mappings,
        };
        let xpc_con = unsafe {
            let connection = xpc_sys::xpc_connection_create(
                c_id.as_ptr(),
                std::ptr::null_mut::<dispatch_queue_s>(),
            );
            if connection.is_null() {
                panic!("Failed to create XPC connection");
            }

            let handler = block::ConcreteBlock::new(move |obj: xpc_object_t| {
                if obj.is_null() {
                    return;
                }
                // Use from_raw_retain to take our own reference.
                // Do NOT use XPCObject as the block parameter — its Drop
                // would call xpc_release on a borrowed reference we don't own,
                // since XPC event handlers receive non-owned pointers.
                let apple_obj = xpc::AppleObject::from_raw_retain(obj);

                // Deserialize the envelope — we only need id and reply_id here,
                // so use serde_json::Value-style ignored data field.
                let msg: Result<Message<serde::de::IgnoredAny>, _> = xpc::from_xpc(&apple_obj);
                let Ok(msg) = msg else { return };

                if msg.reply_id.is_none() {
                    // Handle a message sent to us (no reply_id means it's not a reply)
                    return;
                }

                // Handle a reply to a message we sent
                if let Some(reply_closures) = reply_closures_weak.upgrade() {
                    let mut guard = reply_closures.lock().unwrap();
                    if let Some(callback) = guard.remove(&msg.id) {
                        callback(apple_obj);
                    }
                }
            });
            let handler = handler.copy();

            xpc_sys::xpc_connection_set_event_handler(connection, &*handler as *const _ as *mut _);
            xpc_sys::xpc_connection_resume(connection);
            apple_context._handler = Some(Box::new(handler));
            connection
        };
        apple_context.xpc_con = xpc_con;
        apple_context
    }

    pub fn new_id(
        &self,
        guard: &mut MutexGuard<HashMap<i64, Box<dyn FnOnce(xpc::AppleObject) + Send>>>,
    ) -> i64 {
        let mut id: i64 = rand::random();
        while guard.contains_key(&id) {
            id = rand::random();
        }
        id
    }

    pub fn send_message<T, R, F>(
        &self,
        data: T,
        message_type: MessageType,
        callback: F,
    ) -> Result<(), xpc::Error>
    where
        T: Serialize,
        R: for<'de> Deserialize<'de> + 'static,
        F: FnOnce(Message<R>, xpc::AppleObject) + Send + 'static,
    {
        let mut guard = self.reply_closures.lock().unwrap();

        let id = self.new_id(&mut guard);

        // Wrap the typed callback: deserialize the raw reply into Message<R>,
        // then forward to the caller's callback.
        guard.insert(
            id,
            Box::new(move |apple_obj: xpc::AppleObject| {
                if let Ok(msg) = xpc::from_xpc::<Message<R>>(&apple_obj) {
                    callback(msg, apple_obj);
                }
            }),
        );

        let msg = Message::new(id, message_type, data);
        let apple_obj = xpc::to_xpc(&msg)?;

        unsafe {
            xpc_connection_send_message(self.xpc_con, apple_obj.as_ptr());
        }

        Ok(())
    }
}

impl DispatchContext for AppleContext {}

impl Allocator for AppleContext {
    fn alloc(&mut self, size: u32) -> Result<u128, crate::error::Error> {
        println!("[DEBUG] [alloc] [apple] size: {}", size);
        // Send message to the XPC manager and wait for a reply
        let data = AllocData {
            context_id: self.context_id as i64,
            size: size as i64,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.send_message::<AllocData, AllocReply, _>(data, MessageType::Alloc, move |msg, _| {
            let alloc_id = (msg.message_data.alloc_id_high as u128) << 64
                | (msg.message_data.alloc_id_low as u128);
            tx.send(alloc_id).ok();
        })
        .map_err(|err| {
            println!("[DEBUG] [alloc] [apple] err: {:?}", err);
            crate::error::Error::OperationUnsupported
        })?;

        let reply = rx
            .recv()
            .map_err(|_| crate::error::Error::OperationUnsupported)?;
        Ok(reply)
    }

    fn free(&mut self, id: u128) -> () {
        let data = FreeData {
            alloc_id_high: (id >> 64) as u64,
            alloc_id_low: id as u64,
        };
        self.send_message::<FreeData, (), _>(data, MessageType::Free, |_, _| {})
            .ok();
    }

    fn map_id(&mut self, id: u128) -> Result<*mut u8, crate::error::Error> {
        println!("[DEBUG] [map_id] [apple] id: {}", id);
        let data = MapIdData {
            alloc_id_high: (id >> 64) as u64,
            alloc_id_low: id as u64,
        };
        let (tx, rx) = std::sync::mpsc::channel();

        // Note: the callback needs access to the raw AppleObject for this to work.
        // If your current send_message only passes a typed Message<R>,
        // add a variant that also passes the raw AppleObject, or modify the wrapper.
        let mappings = self.mappings.clone();
        self.send_message::<MapIdData, serde::de::IgnoredAny, _>(
            data,
            MessageType::MapId,
            move |_, apple_obj| {
                let xpc_handle = unsafe {
                    println!("Received apple_obj: {:?}", apple_obj);

                    // Get the outer dictionary pointer
                    let root = apple_obj.as_ptr();

                    // Get "message_data"
                    let key = std::ffi::CString::new("message_data").unwrap();
                    let message_data = xpc_sys::xpc_dictionary_get_value(root, key.as_ptr());

                    // Get "xpc_shmem" (this is an xpc_object_t of type shmem)
                    let key2 = std::ffi::CString::new("xpc_shmem").unwrap();
                    xpc_sys::xpc_dictionary_get_value(message_data, key2.as_ptr())
                };

                let mut mapped_addr: *mut std::os::raw::c_void = std::ptr::null_mut();
                let _size = unsafe { xpc_shmem_map(xpc_handle, &mut mapped_addr) };

                // Store the apple_obj to keep the mapping alive.
                mappings.lock().unwrap().insert(id, apple_obj);
                tx.send(mapped_addr as usize).ok();
            },
        )
        .map_err(|_| crate::error::Error::OperationUnsupported)?;

        let reply = rx
            .recv()
            .map_err(|_| crate::error::Error::OperationUnsupported)?;
        Ok(reply as *mut u8)
    }
}
