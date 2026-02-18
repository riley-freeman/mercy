pub mod xpc;

use crate::{alloc::Allocator, pal::DispatchContext};
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ffi::CString,
    mem,
    sync::{Arc, Mutex, MutexGuard},
};
use block::ConcreteBlock;
use xpc_sys::{
    dispatch_queue_s, xpc_connection_send_message, xpc_connection_t, xpc_object_t, xpc_shmem_map,
};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct AppleContext {
    xpc_con: xpc_connection_t,
    #[derivative(Debug = "ignore")]
    reply_closures: Arc<Mutex<HashMap<i64, Box<dyn FnOnce(xpc::AppleObject) + Send>>>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum MessageType {
    Alloc,
    Free,
    MapId,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Message<T> {
    pub id: i64,
    pub reply_id: Option<i64>,
    pub message_type: MessageType,
    pub message_data: T,
}

impl<T> Message<T> {
    pub fn new(id: i64, message_type: MessageType, data: T) -> Self {
        Self {
            id,
            reply_id: None,
            message_type,
            message_data: data,
        }
    }

    pub fn with_reply(id: i64, reply_id: i64, message_type: MessageType, data: T) -> Self {
        Self {
            id,
            reply_id: Some(reply_id),
            message_type,
            message_data: data,
        }
    }
}

unsafe impl Send for AppleContext {}
unsafe impl Sync for AppleContext {}

impl AppleContext {
    pub fn new(_id: &str) -> Self {
        let reply_closures = Arc::new(Mutex::new(HashMap::<
            i64,
            Box<dyn FnOnce(xpc::AppleObject) + Send>,
        >::new()));
        let reply_closures_weak = Arc::downgrade(&reply_closures);
        let c_id = CString::new("com.itsjustbox.crayon.mercy.MercyServerXPC")
            .expect("id contains interior null byte");
        let xpc_con = unsafe {
            let connection = xpc_sys::xpc_connection_create(
                c_id.as_ptr(),
                std::ptr::null_mut::<dispatch_queue_s>(),
            );
            if connection.is_null() {
                panic!("Failed to create XPC connection");
            }

            let handler = ConcreteBlock::new(move |obj: xpc_object_t| {
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
            mem::forget(handler);
            xpc_sys::xpc_connection_resume(connection);
            connection
        };

        AppleContext {
            xpc_con,
            reply_closures,
        }
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
        F: FnOnce(Message<R>) + Send + 'static,
    {
        let mut guard = self.reply_closures.lock().unwrap();

        let id = self.new_id(&mut guard);

        // Wrap the typed callback: deserialize the raw reply into Message<R>,
        // then forward to the caller's callback.
        guard.insert(
            id,
            Box::new(move |apple_obj: xpc::AppleObject| {
                if let Ok(msg) = xpc::from_xpc::<Message<R>>(&apple_obj) {
                    callback(msg);
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

#[derive(Debug, Serialize, Deserialize)]
pub struct AllocData {
    pub size: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AllocReply {
    pub alloc_id: [u8; 16],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FreeData {
    pub alloc_id: [u8; 16],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MapIdData {
    pub alloc_id: [u8; 16],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MapIdReply {
    pub xpc_handle_int: usize,
}

impl Allocator for AppleContext {
    fn alloc(&mut self, size: u32) -> Result<u128, crate::error::Error> {
        // Send message to the XPC manager and wait for a reply
        let data = AllocData { size: size as i64 };
        let (tx, rx) = std::sync::mpsc::channel();
        self.send_message::<AllocData, AllocReply, _>(data, MessageType::Alloc, move |msg| {
            tx.send(msg.message_data.alloc_id).ok();
        })
        .map_err(|_| crate::error::Error::OperationUnsupported)?;

        let reply = rx
            .recv()
            .map_err(|_| crate::error::Error::OperationUnsupported)?;
        Ok(u128::from_ne_bytes(reply))
    }

    fn free(&mut self, id: u128) -> () {
        let data = FreeData {
            alloc_id: id.to_ne_bytes(),
        };
        self.send_message::<FreeData, (), _>(data, MessageType::Free, |_| {})
            .ok();
    }

    fn map_id(&mut self, id: u128) -> Result<*mut u8, crate::error::Error> {
        let data = MapIdData {
            alloc_id: id.to_ne_bytes(),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.send_message::<MapIdData, MapIdReply, _>(data, MessageType::MapId, move |msg| {
            let xpc_handle = msg.message_data.xpc_handle_int as xpc_object_t;
            let region = unsafe { xpc_shmem_map(xpc_handle, std::ptr::null_mut()) };
            tx.send(region).ok();
        })
        .map_err(|_| crate::error::Error::OperationUnsupported)?;

        let reply = rx
            .recv()
            .map_err(|_| crate::error::Error::OperationUnsupported)?;
        Ok(reply as *mut u8)
    }
}
