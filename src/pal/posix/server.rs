use std::{
    collections::{HashMap, LinkedList},
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    process::exit,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use serde_json::Value;
use shared_memory::{Shmem, ShmemConf};

use crate::{
    error::Error,
    message::{AllocData, AllocReply, FreeData, MapIdData, Message, MessageType},
    pal::posix::{MapIdReply, new_unix_socket_path},
};

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
            Ok(stream) => {
                println!(
                    "[DEBUG] [posix] [server] Accepted connection from {:?}",
                    stream.peer_addr().unwrap()
                );

                // Create a new thread to handle this client's messages.
                let family_id_clone = String::from(family_id);
                let is_running_clone = Arc::clone(&is_running);
                let socket_path_clone = socket_path.clone();
                let thread = std::thread::spawn(move || {
                    handle_client_messages(
                        stream,
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
    client_id: &str,
    is_running: Arc<AtomicBool>,
    socket_path: String,
) {
    let mut buffer = [0; 1024];
    let mut allocations = HashMap::new();

    let mut next_block_id = 0;
    let mut available_blocks = LinkedList::new();

    let mut pending = String::new();

    loop {
        let bytes_read = stream.read(&mut buffer).unwrap();
        if bytes_read == 0 {
            // We want to quit this thread because the connection terminated.
            allocations.clear();
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
            let value: Value = serde_json::from_str(line).unwrap();

            let message_type = value.get("message_type").unwrap().as_str().unwrap();
            match message_type {
                "Alloc" => handle_alloc_message(
                    client_id,
                    serde_json::from_value(value).unwrap(),
                    &mut stream,
                    &mut next_block_id,
                    &mut available_blocks,
                    &mut allocations,
                ),

                "Free" => handle_free_message(
                    serde_json::from_value(value).unwrap(),
                    &mut allocations,
                    &mut available_blocks,
                ),

                "MapId" => handle_map_id_message(
                    serde_json::from_value(value).unwrap(),
                    &mut stream,
                    &mut allocations,
                ),

                "Shutdown" => {
                    // set running to false and start a dummy connection
                    is_running.store(false, Ordering::Relaxed);
                    let _ = UnixStream::connect(&socket_path);
                    return;
                }

                _ => Err(Error::UnexpectedMessageType {
                    message_type: message_type.to_string(),
                }),
            }
            .unwrap()
        }
    }
}

fn handle_alloc_message(
    family_id: &str,
    message: Message<AllocData>,
    stream: &mut UnixStream,
    next_block_id: &mut u16,
    available_blocks: &mut LinkedList<u16>,
    allocations: &mut HashMap<u16, (String, Shmem)>,
) -> Result<(), Error> {
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

    allocations.insert(block_id, (os_id, shmem));

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

fn handle_free_message(
    message: Message<FreeData>,
    allocations: &mut HashMap<u16, (String, Shmem)>,
    available_blocks: &mut LinkedList<u16>,
) -> Result<(), Error> {
    let block_id = (message.message_data.alloc_id_low >> 16) as u16;
    allocations.remove(&block_id);
    available_blocks.push_back(block_id);

    Ok(())
}

fn handle_map_id_message(
    message: Message<MapIdData>,
    stream: &mut UnixStream,
    allocations: &HashMap<u16, (String, Shmem)>,
) -> Result<(), Error> {
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
