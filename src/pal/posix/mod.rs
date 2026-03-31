use std::{
    collections::HashMap,
    env::args,
    fs,
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
    message::{
        AllocData, AllocReply, FreeData, MapIdData, Message, MessageType, NewWorkerData,
        NewWorkerReply, SendWorkerMessage, SendWorkerReply,
    },
    pal::DispatchContext,
    worker::Worker,
};

pub mod server;

#[derive(Derivative)]
#[derivative(Debug)]
pub struct PosixContext {
    manager_stream: UnixStream,
    manager_thread: JoinHandle<()>,
    manager_child: Option<std::process::Child>,

    hashed_family_id: u64,
    worker_id: u64,

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
        println!("[DEBUG] [posix] Dropping context...");
        if self.owner {
            // Send a shutdown message to the manager (best-effort; the
            // manager may already have exited, producing a BrokenPipe).
            // let _ = self.send_message::<(), (), _>((), MessageType::Shutdown, |_, _| {});
            self.send_wrapped_message::<(), (), _>((), MessageType::Shutdown, |_, _| {})
                .unwrap();
        }
    }
}

impl PosixContext {
    pub fn new(family_id: &str, hashed_family_id: u64) -> Result<Self, Error> {
        let socket_path = new_unix_socket_path(family_id);
        // Check if we already have a manager.
        if fs::exists(&socket_path).map_err(|e| Error::IoError { io_error: e })? {
            match UnixStream::connect(&socket_path) {
                Ok(_) => {
                    return Err(Error::IdAlreadyExists {
                        id: String::from(family_id),
                    });
                }
                Err(e) => {
                    println!(
                        "[DEBUG] [posix] broken manager found. removing socket file: {}",
                        e
                    );
                    fs::remove_file(&socket_path).map_err(|e| Error::IoError { io_error: e })?;
                }
            }
        }

        let our_args = args().collect::<Vec<String>>();
        let our_command = our_args[0].clone();

        println!(
            "[DEBUG] [posix] Spawning manager with command: {}",
            our_command
        );

        // Run the program again with different env vars to start the manager.
        let manager_child = Command::new(our_command)
            .args(our_args[1..].iter())
            .env("CRAYON_MERCY_ROLE_NAME", "manager")
            .env("CRAYON_MERCY_POSIX_MANAGER_PATH", &socket_path)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| Error::CannotStartProcess { io_error: e })?;

        // Connect to the manager, retrying if it's not ready yet.
        let mut manager_stream = loop {
            match UnixStream::connect(&socket_path) {
                Ok(stream) => break stream,
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::ConnectionRefused
                        || e.kind() == std::io::ErrorKind::NotFound
                    {
                        std::thread::sleep(Duration::from_millis(10));
                    } else {
                        return Err(Error::IoError { io_error: e });
                    }
                }
            }
        };

        manager_stream
            .write_all(&crate::context::Context::worker_id().to_ne_bytes())
            .unwrap();

        let reply_closures = Arc::new(Mutex::new(HashMap::new()));
        let manager_thread = Self::start_manager_thread(&manager_stream, &reply_closures);

        let worker_id = crate::context::Context::worker_id();

        Ok(Self {
            manager_stream,
            manager_thread,
            reply_closures,
            hashed_family_id,
            worker_id,
            manager_child: Some(manager_child),
            shmems: HashMap::new(),
            owner: true,
        })
    }

    pub fn open(
        family_id: &str,
        hashed_family_id: u64,
        take_ownership: bool,
    ) -> Result<Self, Error> {
        let socket_path = new_unix_socket_path(family_id);

        let manager_stream = UnixStream::connect(&socket_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::IdNotFound {
                    id: String::from(family_id),
                }
            } else {
                Error::IoError { io_error: e }
            }
        })?;

        let mut manager_stream = manager_stream;
        manager_stream
            .write_all(&crate::context::Context::worker_id().to_ne_bytes())
            .unwrap();

        let reply_closures = Arc::new(Mutex::new(HashMap::new()));

        let manager_thread = Self::start_manager_thread(&manager_stream, &reply_closures);

        let worker_id = crate::context::Context::worker_id();

        Ok(Self {
            manager_stream,
            manager_thread,
            manager_child: None,
            hashed_family_id,
            worker_id,
            reply_closures: Arc::new(Mutex::new(HashMap::new())),
            shmems: HashMap::new(),
            owner: take_ownership,
        })
    }

    fn start_manager_thread(
        stream: &UnixStream,
        reply_closures: &Arc<Mutex<HashMap<i64, Box<dyn FnOnce(String) + Send>>>>,
    ) -> JoinHandle<()> {
        let stream_clone = stream.try_clone().unwrap();
        let reply_closures_clone = Arc::clone(reply_closures);
        let worker_id = crate::context::Context::worker_id();

        std::thread::spawn(move || {
            Self::handle_messages(worker_id, stream_clone, reply_closures_clone);
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

    pub fn send_wrapped_message<T, R, F>(
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
            Box::new(
                move |reply_str| match serde_json::from_str::<Message<R>>(&reply_str) {
                    Ok(msg) => callback(msg, reply_str.clone()),
                    Err(e) => println!("ERROR PARSING ResponseWorker: {:?}", e),
                },
            ),
        );

        let msg = Message::new(id, message_type, data);
        let mut msg_str = serde_json::to_string(&msg).unwrap();
        msg_str.push('\n');
        self.manager_stream.write_all(msg_str.as_bytes())?;

        Ok(())
    }

    fn handle_messages(
        worker_id: u64,
        mut manager_stream: UnixStream,
        reply_closures: Arc<Mutex<HashMap<i64, Box<dyn FnOnce(String) + Send>>>>,
    ) {
        let mut buffer = [0; 1024];
        let mut pending = String::new();
        loop {
            let length = manager_stream.read(&mut buffer).unwrap();
            // If the manager stream is closed, break the loop.
            if length == 0 {
                break;
            }

            pending.push_str(&String::from_utf8_lossy(&buffer[..length]));

            // Process all complete newline-delimited messages.
            while let Some(newline_pos) = pending.find('\n') {
                let line: String = pending.drain(..=newline_pos).collect();
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                // Try to parse it as a reply to a manager action
                if let Ok(msg) = serde_json::from_str::<Message<serde_json::Value>>(line) {
                    if let Some(reply_id) = msg.reply_id {
                        let mut reply_closures = reply_closures.lock().unwrap();
                        if let Some(callback) = reply_closures.remove(&reply_id) {
                            callback(line.to_string());
                        }
                        continue;
                    }
                }

                // If it wasn't a manager reply, it's a message sent to us
                if let Ok(msg) = serde_json::from_str::<Message<serde_value::Value>>(line) {
                    let response = crate::context::invoke_message_callback(msg.message_data);
                    if let Some(response) = response {
                        let data = SendWorkerReply {
                            worker_id: worker_id,
                            message_data: response,
                        };

                        let msg =
                            Message::with_reply(msg.id, msg.id, MessageType::ResponseWorker, data);
                        let mut msg_str = serde_json::to_string(&msg).unwrap();
                        msg_str.push('\n');
                        manager_stream.write_all(msg_str.as_bytes()).ok();
                    }
                }
            }
        }
    }
}

impl DispatchContext for PosixContext {
    fn spawn_worker(&mut self, role: &str, args: Vec<String>) -> Result<u64, Error> {
        let data = NewWorkerData {
            worker_role: role.to_string(),
            arguments: args,
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.send_wrapped_message::<NewWorkerData, NewWorkerReply, _>(
            data,
            MessageType::NewWorker,
            move |msg, _| {
                tx.send(msg.message_data.worker_id).ok();
            },
        )
        .map_err(|err| {
            println!("[DEBUG] [new_worker] [posix] err: {:?}", err);
            crate::error::Error::OperationUnsupported
        })?;

        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(reply) => Ok(reply),
            Err(e) => {
                println!("[DEBUG] [new_worker] [posix] err: {:?}", e);
                return Err(crate::error::Error::OperationUnsupported);
            }
        }
    }

    fn send_message(
        &mut self,
        worker: &Worker,
        message: serde_value::Value,
        callback: Box<dyn FnOnce(serde_value::Value) + Send + 'static>,
    ) -> Result<(), crate::error::Error> {
        let data = SendWorkerMessage {
            worker_id: worker.id(),
            message_data: message,
        };

        self.send_wrapped_message::<SendWorkerMessage, serde_value::Value, _>(
            data,
            MessageType::SendWorker,
            |msg, _| {
                callback(msg.message_data);
            },
        )
        .map_err(|err| {
            println!("[DEBUG] [send_message] [posix] err: {:?}", err);
            crate::error::Error::CannotSendWorkerMessage { io_error: err }
        })?;
        Ok(())
    }
}

impl Allocator for PosixContext {
    fn alloc(&mut self, size: u32) -> Result<u128, crate::error::Error> {
        // Send message to the server and wait for a reply
        let data = AllocData {
            family_id: self.hashed_family_id as i64,
            size: size as i64,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.send_wrapped_message::<AllocData, AllocReply, _>(
            data,
            MessageType::Alloc,
            move |msg, _| {
                let alloc_id = (msg.message_data.alloc_id_high as u128) << 64
                    | (msg.message_data.alloc_id_low as u128);
                tx.send(alloc_id).ok();
            },
        )
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
        self.send_wrapped_message::<FreeData, (), _>(data, MessageType::Free, |_, _| {})
            .ok();
        self.shmems.remove(&id);
    }

    fn map_id(&mut self, id: u128) -> Result<*mut u8, crate::error::Error> {
        if self.shmems.contains_key(&id) {
            return Ok(self.shmems[&id].as_ptr());
        }

        let data = MapIdData {
            alloc_id_high: (id >> 64) as u64,
            alloc_id_low: id as u64,
        };
        let size = (id >> 32) as u32;

        let (tx, rx) = std::sync::mpsc::channel();
        self.send_wrapped_message::<MapIdData, MapIdReply, _>(
            data,
            MessageType::MapId,
            move |msg, _| {
                tx.send(msg.message_data.os_id).ok();
            },
        )
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

fn new_unix_socket_path(family_id: &str) -> String {
    format!("/tmp/mercy.{}", family_id)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MapIdReply {
    pub os_id: String,
}
