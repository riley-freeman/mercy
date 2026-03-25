use std::{
    collections::HashMap,
    env::args,
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::{Command, Stdio},
    sync::{Arc, Mutex, MutexGuard},
    thread::JoinHandle,
    time::Duration,
};

use derivative::Derivative;
use serde::{Deserialize, Serialize};
use shared_memory::{Shmem, ShmemConf};

use crate::{
    alloc::Allocator,
    error::Error,
    message::{AllocData, AllocReply, FreeData, MapIdData, Message, MessageType},
    pal::DispatchContext,
};

pub mod server;

#[derive(Derivative)]
#[derivative(Debug)]
pub struct PosixContext {
    manager_stream: Arc<Mutex<UnixStream>>,
    manager_thread: JoinHandle<()>,
    manager_child: Option<std::process::Child>,

    hashed_context_id: u64,

    #[derivative(Debug = "ignore")]
    reply_closures: Arc<Mutex<HashMap<i64, Box<dyn FnOnce(String) + Send>>>>,

    #[derivative(Debug = "ignore")]
    shmems: HashMap<u128, Shmem>,

    owner: bool,
}

unsafe impl Send for PosixContext {}
unsafe impl Sync for PosixContext {}

impl Drop for PosixContext {
    fn drop(&mut self) {
        if self.owner {
            // Send a shutdown message to the manager
            self.send_message::<(), (), _>((), MessageType::Shutdown, |_, _| {})
                .unwrap();
        }
    }
}

impl PosixContext {
    pub fn new(context_id: &str, hashed_context_id: u64) -> Self {
        let our_args = args().collect::<Vec<String>>();
        let our_command = our_args[0].clone();

        let socket_path = new_unix_socket_path(context_id);

        println!(
            "[DEBUG] [posix] Spawning manager with command: {}",
            our_command
        );
        let manager_child = Command::new(our_command)
            .args(our_args[1..].iter())
            .env("CRAYON_MERCY_JOB_NAME", "manager")
            .env("CRAYON_MERCY_POSIX_MANAGER_PATH", &socket_path)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();

        let manager_stream = loop {
            match UnixStream::connect(&socket_path) {
                Ok(stream) => break stream,
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::ConnectionRefused
                        || e.kind() == std::io::ErrorKind::NotFound
                    {
                        std::thread::sleep(Duration::from_millis(10));
                    } else {
                        panic!("Failed to connect to manager: {}", e);
                    }
                }
            }
        };
        let manager_stream_clone = manager_stream.try_clone().unwrap();
        let manager_stream = Arc::new(Mutex::new(manager_stream));

        let reply_closures = Arc::new(Mutex::new(HashMap::new()));
        let reply_closures_clone = Arc::clone(&reply_closures);

        let manager_thread = std::thread::spawn(move || {
            Self::handle_messages(manager_stream_clone, reply_closures_clone);
        });

        Self {
            manager_stream,
            manager_thread,
            reply_closures,
            hashed_context_id,
            manager_child: Some(manager_child),
            shmems: HashMap::new(),
            owner: true,
        }
    }

    pub fn open(context_id: &str, hashed_context_id: u64) -> Result<Self, Error> {
        let socket_path = new_unix_socket_path(context_id);

        let manager_stream =
            UnixStream::connect(&socket_path).map_err(|e| Error::IoError { io_error: e })?;
        let manager_stream_clone = manager_stream.try_clone().unwrap();
        let manager_stream = Arc::new(Mutex::new(manager_stream));

        let reply_closures = Arc::new(Mutex::new(HashMap::new()));
        let reply_closures_clone = Arc::clone(&reply_closures);

        let manager_thread = std::thread::spawn(move || {
            Self::handle_messages(manager_stream_clone, reply_closures_clone);
        });

        Ok(Self {
            manager_stream,
            manager_thread,
            manager_child: None,
            hashed_context_id,
            reply_closures: Arc::new(Mutex::new(HashMap::new())),
            shmems: HashMap::new(),
            owner: false,
        })
    }

    pub fn new_id(
        &self,
        guard: &mut MutexGuard<HashMap<i64, Box<dyn FnOnce(String) + Send>>>,
    ) -> i64 {
        let mut id: i64 = rand::random();
        while guard.contains_key(&id) {
            id = rand::random();
        }
        id
    }

    pub fn send_message<T, R, F>(
        &mut self,
        data: T,
        message_type: MessageType,
        callback: F,
    ) -> Result<(), std::io::Error>
    where
        T: Serialize,
        R: for<'de> Deserialize<'de> + 'static,
        F: FnOnce(Message<R>, String) + Send + 'static,
    {
        let mut guard = self.reply_closures.lock().unwrap();
        let id = self.new_id(&mut guard);
        guard.insert(
            id,
            Box::new(move |reply_str| {
                if let Ok(msg) = serde_json::from_str::<Message<R>>(&reply_str) {
                    callback(msg, reply_str.clone());
                }
            }),
        );

        let msg = Message::new(id, message_type, data);
        let msg_str = serde_json::to_string(&msg).unwrap();
        self.manager_stream
            .lock()
            .unwrap()
            .write_all(msg_str.as_bytes())
            .unwrap();

        Ok(())
    }

    fn handle_messages(
        mut manager_stream: UnixStream,
        reply_closures: Arc<Mutex<HashMap<i64, Box<dyn FnOnce(String) + Send>>>>,
    ) {
        let mut buffer = [0; 1024];
        loop {
            let length = manager_stream.read(&mut buffer).unwrap();
            // If the manager stream is closed, break the loop.
            if length == 0 {
                break;
            }

            let msg_str = String::from_utf8_lossy(&buffer[..length]);
            let msg: Message<serde_json::Value> = serde_json::from_str(&msg_str).unwrap();

            if msg.reply_id.is_none() {
                // Handle a message sent to us (no reply id means it's not a reply)
                continue;
            }

            let mut reply_closures = reply_closures.lock().unwrap();
            if let Some(callback) = reply_closures.remove(&msg.reply_id.unwrap()) {
                callback(msg_str.to_string());
            }
        }
    }
}

impl DispatchContext for PosixContext {}

impl Allocator for PosixContext {
    fn alloc(&mut self, size: u32) -> Result<u128, crate::error::Error> {
        // Send message to the server and wait for a reply
        let data = AllocData {
            context_id: self.hashed_context_id as i64,
            size: size as i64,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.send_message::<AllocData, AllocReply, _>(data, MessageType::Alloc, move |msg, _| {
            let alloc_id = (msg.message_data.alloc_id_high as u128) << 64
                | (msg.message_data.alloc_id_low as u128);
            tx.send(alloc_id).ok();
        })
        .map_err(|err| {
            println!("[DEBUG] [alloc] [posix] err: {:?}", err);
            crate::error::Error::OperationUnsupported
        })?;

        let reply = match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(reply) => reply,
            Err(e) => {
                println!("[DEBUG] [alloc] [posix] err: {:?}", e);
                return Err(crate::error::Error::OperationUnsupported);
            }
        };

        Ok(reply)
    }

    fn free(&mut self, id: u128) {
        let data = FreeData {
            alloc_id_high: (id >> 64) as u64,
            alloc_id_low: id as u64,
        };
        self.send_message::<FreeData, (), _>(data, MessageType::Free, |_, _| {})
            .ok();
        self.shmems.remove(&id);
    }

    fn map_id(&mut self, id: u128) -> Result<*mut u8, crate::error::Error> {
        let data = MapIdData {
            alloc_id_high: (id >> 64) as u64,
            alloc_id_low: id as u64,
        };
        let size = (id >> 32) as u32;

        let (tx, rx) = std::sync::mpsc::channel();
        self.send_message::<MapIdData, MapIdReply, _>(data, MessageType::MapId, move |msg, _| {
            tx.send(msg.message_data.os_id).ok();
        })
        .map_err(|err| {
            println!("[DEBUG] [map_id] [posix] err: {:?}", err);
            crate::error::Error::OperationUnsupported
        })?;

        let reply = rx
            .recv()
            .map_err(|_| crate::error::Error::OperationUnsupported)?;
        let shmem = ShmemConf::new()
            .os_id(reply)
            .size(size as usize)
            .open()
            .unwrap();
        let ptr = shmem.as_ptr();
        self.shmems.insert(id, shmem);
        Ok(ptr)
    }
}

fn new_unix_socket_path(context_id: &str) -> String {
    format!("/tmp/mercy.{}", context_id)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MapIdReply {
    pub os_id: String,
}
