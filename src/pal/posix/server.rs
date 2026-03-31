use std::{
    collections::{HashMap, LinkedList},
    env::args,
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    process::exit,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::sleep,
    time::Duration,
};

use crate::{
    message::{GetPlatformMutex, GetPlatformMutexReply},
    sync::ArcReferenceCounts,
};

use libc::{
    PTHREAD_PROCESS_SHARED, key_t, pthread_mutex_init, pthread_mutex_t, pthread_mutexattr_init,
    pthread_mutexattr_setpshared, pthread_mutexattr_t,
};
use serde_json::Value;
use shared_memory::{Shmem, ShmemConf};

struct SendShmem(Shmem);
unsafe impl Send for SendShmem {}

use crate::{
    error::Error,
    message::{
        AllocData, AllocReply, FreeData, GetWorkerStateData, GetWorkerStateReply, MapIdData,
        Message, MessageType, NewMutexData, NewMutexReply, NewWorkerData, NewWorkerReply,
        SendWorkerMessage, SendWorkerReply, SetWorkerStateData,
    },
    pal::posix::{MapIdReply, new_unix_socket_path, worker::WorkerInfo},
};
use std::process::Command;

static WORKERS: LazyLock<Mutex<HashMap<u64, WorkerInfo>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static ALLOCATIONS: LazyLock<Mutex<HashMap<u16, (String, SendShmem)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static MUTEXES: LazyLock<Mutex<HashMap<u64, pthread_mutex_t>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static AVAILABLE_BLOCKS: LazyLock<Mutex<LinkedList<u16>>> =
    LazyLock::new(|| Mutex::new(LinkedList::new()));

static NEXT_BLOCK_ID: LazyLock<Mutex<u16>> = LazyLock::new(|| Mutex::new(0));

pub fn start_server(family_id: &str) -> Result<(), Error> {
    let socket_path =
        std::env::var("CRAYON_MERCY_POSIX_MANAGER_PATH").unwrap_or(new_unix_socket_path(family_id));

    let listener = match UnixListener::bind(&socket_path) {
        Ok(listener) => listener,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                println!("[DEBUG] [posix] [server] Socket already in use, removing...");
                std::fs::remove_file(&socket_path).unwrap();
                UnixListener::bind(&socket_path).unwrap()
            } else {
                return Err(Error::IoError { io_error: e });
            }
        }
    };
    println!(
        "[DEBUG] [posix] [server] Server listening on {}",
        &socket_path
    );

    let mut socket_threads = vec![];
    let is_running = Arc::new(AtomicBool::new(true));

    for stream in listener.incoming() {
        if !is_running.load(Ordering::Relaxed) {
            break;
        }

        match stream {
            Ok(mut stream) => {
                println!(
                    "[DEBUG] [posix] [server] Accepted connection from {:?}",
                    stream.peer_addr().unwrap()
                );

                // Register the worker
                let mut buf = [0; 8];
                let bytes_read = stream.read(&mut buf).unwrap();
                if bytes_read == 0 {
                    continue;
                }

                let worker_id = u64::from_ne_bytes(buf);
                WORKERS
                    .lock()
                    .unwrap()
                    .insert(worker_id, WorkerInfo::new(stream.try_clone().unwrap()));

                // Create a new thread to handle this client's messages.
                let family_id_clone = String::from(family_id);
                let is_running_clone = Arc::clone(&is_running);
                let socket_path_clone = socket_path.clone();
                let thread = std::thread::spawn(move || {
                    handle_client_messages(
                        stream,
                        worker_id,
                        &family_id_clone,
                        is_running_clone,
                        socket_path_clone,
                    );
                });
                socket_threads.push(thread);
            }
            Err(e) => eprintln!(
                "[DEBUG] [posix] [server] Failed to accept connection: {}",
                e
            ),
        }
    }
    println!("[DEBUG] [posix] [server] Server shutting down...");

    for thread in socket_threads {
        thread.join().unwrap();
    }
    let _ = std::fs::remove_file(&socket_path);

    println!("[DEBUG] [posix] [server] Goodbye!");
    Ok(())
}

fn handle_client_messages(
    mut stream: UnixStream,
    worker_id: u64,
    client_id: &str,
    is_running: Arc<AtomicBool>,
    socket_path: String,
) {
    let mut buffer = [0; 1024];
    let mut pending = String::new();

    loop {
        let bytes_read = stream.read(&mut buffer).unwrap_or(0);
        if bytes_read == 0 {
            // We want to quit this thread because the connection terminated.
            if let Some(worker) = WORKERS.lock().unwrap().remove(&worker_id) {
                if let Some(state_id) = worker.state {
                    decrement_rc(state_id);
                }
            }
            break;
        }

        pending.push_str(&String::from_utf8_lossy(&buffer[..bytes_read]));

        // Process all complete newline-delimited messages in the buffer.
        while let Some(newline_pos) = pending.find('\n') {
            let line: String = pending.drain(..=newline_pos).collect();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            println!("[DEBUG] [posix] [server] Received message: {line}",);
            let value: Value = serde_json::from_str(line).expect("Failed to parse message");

            let message_type = value.get("message_type").unwrap().as_str().unwrap();
            match message_type {
                "Alloc" => handle_alloc_message(
                    client_id,
                    serde_json::from_value(value).unwrap(),
                    &mut stream,
                ),

                "Free" => handle_free_message(serde_json::from_value(value).unwrap()),

                "MapId" => {
                    handle_map_id_message(serde_json::from_value(value).unwrap(), &mut stream)
                }

                "Shutdown" => {
                    // set running to false and start a dummy connection
                    is_running.store(false, Ordering::Relaxed);
                    let _ = UnixStream::connect(&socket_path);
                    return;
                }

                "NewWorker" => {
                    handle_new_worker_message(serde_json::from_value(value).unwrap(), &mut stream)
                }

                "SendWorker" => {
                    handle_send_worker_message(serde_json::from_value(value).unwrap(), &mut stream)
                }

                "ResponseWorker" => handle_response_worker_message(
                    serde_json::from_value(value).unwrap(),
                    &mut stream,
                ),

                "SetWorkerState" => handle_set_worker_state(
                    worker_id,
                    serde_json::from_value(value).unwrap(),
                    &mut stream,
                ),

                "GetWorkerState" => {
                    handle_get_worker_state(serde_json::from_value(value).unwrap(), &mut stream)
                }

                "NewMutex" => {
                    handle_new_mutex_message(serde_json::from_value(value).unwrap(), &mut stream)
                }
                "GetPlatformMutex" => handle_get_platform_mutex_message(
                    serde_json::from_value(value).unwrap(),
                    &mut stream,
                ),

                _ => Err(Error::UnexpectedMessageType {
                    message_type: message_type.to_string(),
                }),
            }
            .unwrap()
        }
    }
}

fn decrement_rc(alloc_id: u128) {
    let mut allocations = ALLOCATIONS.lock().unwrap();
    let mut available_blocks = AVAILABLE_BLOCKS.lock().unwrap();
    if alloc_id == 0 {
        return;
    }

    let block_id = (alloc_id >> 16) as u16;
    if let Some((_, shmem)) = allocations.get(&block_id) {
        let rcs = unsafe { &mut *(shmem.0.as_ptr() as *mut ArcReferenceCounts) };
        let prev = rcs.count.fetch_sub(1, Ordering::AcqRel);

        if prev <= 1 {
            // Strong count hit zero, free the data block and this RCS block
            let data_id = rcs.data_id;

            // 1. Free the data block
            let data_block_id = (data_id >> 16) as u16;
            if let Some(_) = allocations.remove(&data_block_id) {
                available_blocks.push_back(data_block_id);
            }

            // 2. Free the RCS block if no weaks
            if rcs.weaks.load(Ordering::Acquire) <= 0 {
                allocations.remove(&block_id);
                available_blocks.push_back(block_id);
            }
        }
    }
}

fn handle_alloc_message(
    family_id: &str,
    message: Message<AllocData>,
    stream: &mut UnixStream,
) -> Result<(), Error> {
    let mut allocations = ALLOCATIONS.lock().unwrap();
    let mut available_blocks = AVAILABLE_BLOCKS.lock().unwrap();
    let mut next_block_id = NEXT_BLOCK_ID.lock().unwrap();
    println!("[DEBUG] [posix] [server] Handling alloc message");
    println!("[DEBUG] [posix] [server] Allocating block_id.");

    let alloc_data = message.message_data;
    let family_id_hash = alloc_data.family_id;
    let size = alloc_data.size;

    let mut block_id = available_blocks.pop_front().unwrap_or(*next_block_id);
    if block_id == *next_block_id {
        *next_block_id += 1;
    }

    let (shmem, os_id) = loop {
        let os_id = format!("{family_id}.mercy_server_block.{block_id}");
        let res = ShmemConf::new()
            .os_id(&os_id)
            .size(alloc_data.size as usize)
            .create();
        println!(
            "[DEBUG] [posix] [server] Created shmem with os_id: {}",
            os_id
        );

        match res {
            Ok(shmem) => break (shmem, os_id),
            Err(shared_memory::ShmemError::MappingIdExists) => {
                eprintln!("[ERROR] [posix] [server] block already in use, retrying!");
                block_id = available_blocks.pop_front().unwrap_or(*next_block_id);
                if block_id == *next_block_id {
                    *next_block_id += 1;
                }
            }
            Err(e) => return Err(Error::ShmemError { shmem_error: e }),
        }
    };

    let alloc_id = (family_id_hash as u128) << 64 | (size as u128) << 32 | (block_id as u128) << 16;

    allocations.insert(block_id, (os_id, SendShmem(shmem)));

    let reply = Message::with_reply(
        message.id,
        message.id,
        MessageType::Alloc,
        AllocReply {
            alloc_id_high: (alloc_id >> 64) as u64,
            alloc_id_low: alloc_id as u64,
        },
    );

    let mut bytes = serde_json::to_vec(&reply).unwrap();
    bytes.push(b'\n');
    stream.write_all(&bytes).unwrap();

    Ok(())
}

fn handle_free_message(message: Message<FreeData>) -> Result<(), Error> {
    let mut allocations = ALLOCATIONS.lock().unwrap();
    let mut available_blocks = AVAILABLE_BLOCKS.lock().unwrap();
    let block_id = (message.message_data.alloc_id_low >> 16) as u16;
    allocations.remove(&block_id);
    available_blocks.push_back(block_id);

    Ok(())
}

fn handle_map_id_message(
    message: Message<MapIdData>,
    stream: &mut UnixStream,
) -> Result<(), Error> {
    let allocations = ALLOCATIONS.lock().unwrap();
    let alloc_id = (message.message_data.alloc_id_high as u128) << 64
        | message.message_data.alloc_id_low as u128;
    let block_id = (alloc_id >> 16) as u16;

    println!("Block ID: {}", block_id);
    let (os_id, _) = allocations.get(&block_id).unwrap();

    // Return the OS ID to the client
    let data = MapIdReply {
        os_id: os_id.clone(),
    };
    let reply = Message::with_reply(message.id, message.id, MessageType::MapId, data);
    let mut bytes = serde_json::to_vec(&reply).unwrap();
    bytes.push(b'\n');
    stream.write_all(&bytes).unwrap();
    Ok(())
}

fn handle_new_worker_message(
    message: Message<NewWorkerData>,
    stream: &mut UnixStream,
) -> Result<(), Error> {
    static NEXT_WORKER_ID: AtomicU64 = AtomicU64::new(1);
    let worker_id = NEXT_WORKER_ID.fetch_add(1, Ordering::Relaxed);

    let role = message.message_data.worker_role;

    let our_args: Vec<String> = args().collect();
    let our_command = our_args[0].clone();

    let arguments = message.message_data.arguments;

    let _command = Command::new(our_command)
        .args(arguments.iter())
        .env("CRAYON_MERCY_ROLE_NAME", role)
        .env("CRAYON_MERCY_WORKER_ID", worker_id.to_string())
        .spawn()
        .map_err(|e| Error::CannotStartProcess { io_error: e })?;

    while WORKERS.lock().unwrap().get(&worker_id).is_none() {
        sleep(Duration::from_millis(100));
    }

    let data = NewWorkerReply { worker_id };
    let reply = Message::with_reply(message.id, message.id, MessageType::NewWorker, data);
    let mut bytes = serde_json::to_vec(&reply).unwrap();
    bytes.push(b'\n');
    stream.write_all(&bytes).unwrap();
    Ok(())
}

fn handle_send_worker_message(
    message: Message<SendWorkerMessage>,
    _stream: &mut UnixStream,
) -> Result<(), Error> {
    let worker_id = message.message_data.worker_id;
    let forwarded_message = Message {
        id: message.id,
        reply_id: message.reply_id,
        message_type: message.message_type,
        message_data: message.message_data.message_data,
    };

    if let Some(worker) = WORKERS.lock().unwrap().get(&worker_id) {
        let mut msg_str = serde_json::to_string(&forwarded_message).unwrap();
        msg_str.push('\n');
        let _ = worker.stream().write_all(msg_str.as_bytes());
    } else {
        eprintln!("[ERROR] [posix] [server] Worker not found: {}", worker_id);
    }
    Ok(())
}

fn handle_response_worker_message(
    message: Message<SendWorkerReply>,
    _stream: &mut UnixStream,
) -> Result<(), Error> {
    let response_message = Message {
        id: message.id,
        reply_id: message.reply_id, // This holds the original request's msg.id
        message_type: message.message_type,
        message_data: message.message_data.message_data,
    };

    let mut msg_str = serde_json::to_string(&response_message).unwrap();
    msg_str.push('\n');

    let mut workers = WORKERS.lock().unwrap();
    for (_, worker) in workers.iter_mut() {
        let _ = worker.stream().write_all(msg_str.as_bytes());
    }

    Ok(())
}

fn handle_new_mutex_message(
    message: Message<NewMutexData>,
    stream: &mut UnixStream,
) -> Result<(), Error> {
    static NEXT_MUTEX_ID: AtomicU64 = AtomicU64::new(1);

    let mutex = unsafe {
        let mut attr = std::mem::MaybeUninit::<pthread_mutexattr_t>::uninit();
        pthread_mutexattr_init(attr.as_mut_ptr());
        pthread_mutexattr_setpshared(attr.as_mut_ptr(), PTHREAD_PROCESS_SHARED);

        let mut mutex = std::mem::MaybeUninit::<pthread_mutex_t>::uninit();
        pthread_mutex_init(mutex.as_mut_ptr(), attr.as_ptr());
        mutex.assume_init()
    };

    let mut mutex_bytes = vec![0u8; std::mem::size_of::<pthread_mutex_t>()];
    unsafe {
        std::ptr::copy_nonoverlapping(
            &mutex as *const _ as *const u8,
            mutex_bytes.as_mut_ptr(),
            mutex_bytes.len(),
        );
    }

    let data = NewMutexReply {
        pthread_mutex: mutex_bytes,
        mutex_id: NEXT_MUTEX_ID.fetch_add(1, Ordering::AcqRel),
    };

    MUTEXES.lock().unwrap().insert(data.mutex_id, mutex);

    let reply = Message::with_reply(message.id, message.id, MessageType::NewMutex, data);
    let mut bytes = serde_json::to_vec(&reply).unwrap();
    bytes.push(b'\n');
    stream.write_all(&bytes).unwrap();
    Ok(())
}

fn handle_get_platform_mutex_message(
    message: Message<GetPlatformMutex>,
    stream: &mut UnixStream,
) -> Result<(), Error> {
    let mutex_id = message.message_data.mutex_id;
    let mutex = MUTEXES.lock().unwrap().get(&mutex_id).unwrap().clone();
    let mut mutex_bytes = vec![0u8; std::mem::size_of::<pthread_mutex_t>()];
    unsafe {
        std::ptr::copy_nonoverlapping(
            &mutex as *const _ as *const u8,
            mutex_bytes.as_mut_ptr(),
            mutex_bytes.len(),
        );
    }

    let data = GetPlatformMutexReply {
        pthread_mutex: mutex_bytes,
    };
    let reply = Message::with_reply(message.id, message.id, MessageType::GetPlatformMutex, data);
    let mut bytes = serde_json::to_vec(&reply).unwrap();
    bytes.push(b'\n');
    stream.write_all(&bytes).unwrap();
    Ok(())
}

fn handle_set_worker_state(
    worker_id: u64,
    message: Message<SetWorkerStateData>,
    _stream: &mut UnixStream,
) -> Result<(), Error> {
    let mut workers = WORKERS.lock().unwrap();
    if let Some(worker) = workers.get_mut(&worker_id) {
        let state_id = (message.message_data.state_id_high as u128) << 64
            | (message.message_data.state_id_low as u128);
        let old_state = worker.state.replace(state_id);
        if let Some(old_state_id) = old_state {
            decrement_rc(old_state_id);
        }
    }
    Ok(())
}

fn handle_get_worker_state(
    message: Message<GetWorkerStateData>,
    stream: &mut UnixStream,
) -> Result<(), Error> {
    let workers = WORKERS.lock().unwrap();
    let state_id = workers
        .get(&message.message_data.worker_id)
        .and_then(|w| w.state);

    let reply = Message::with_reply(
        message.id,
        message.id,
        MessageType::GetWorkerState,
        GetWorkerStateReply {
            state_id_high: state_id.map(|id| (id >> 64) as u64),
            state_id_low: state_id.map(|id| id as u64),
        },
    );

    let mut bytes = serde_json::to_vec(&reply).unwrap();
    bytes.push(b'\n');
    stream.write_all(&bytes).unwrap();
    Ok(())
}
