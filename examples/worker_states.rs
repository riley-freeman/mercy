use mercy::alloc::AllocatesTypes;
use mercy::context::ContextBuilder;
use mercy::sync::Arc;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread::sleep;
use std::time::Duration;

fn log(msg: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/mercy_worker_states.log")
        .unwrap();
    writeln!(file, "{}", msg).unwrap();
    println!("{}", msg);
}

fn main() {
    let family_id = "com.example.worker_states_v3";
    let _ = std::fs::remove_file("/tmp/mercy_worker_states.log");

    ContextBuilder::new(family_id)
        .main(|mut ctx| {
            log("[main] Starting worker states example...");

            // 1. Spawn the producer.
            let producer = ctx
                .new_worker("producer", vec![])
                .expect("Failed to spawn producer");
            log(&format!(
                "[main] Spawned producer worker (ID: {})",
                producer.id()
            ));

            // 2. Spawn the consumer, passing the producer's ID as an argument.
            ctx.new_worker("consumer", vec![producer.id().to_string()])
                .expect("Failed to spawn consumer");
            log("[main] Spawned consumer worker");

            // Give them time to run.
            sleep(Duration::from_secs(12));
            log("[main] Example finished.");
            ctx.close();
        })
        .add_role("producer", |mut ctx| {
            // Give the server a moment to finish registering everything.
            sleep(Duration::from_secs(1));
            log("[producer] Started");

            // Create a shared atomic integer.
            let initial_value = 10;
            log("[producer] Creating shared Arc...");
            let mutex = ctx.new_mutex(initial_value).unwrap();
            log("[producer] Shared Mutex created");
            let shared = ctx.new_arc(mutex).unwrap();
            log("[producer] Shared Arc created");

            // Publish the Arc as our worker state.
            log("[producer] Publishing state...");
            ctx.set_state(shared.clone()).unwrap();
            log(&format!(
                "[producer] Published state (AtomicI32, initial value: {})",
                initial_value
            ));

            // Increment the value over time.
            for i in 1..=10 {
                sleep(Duration::from_secs(1));
                let prev = *shared.lock();
                *shared.lock() += 1;
                log(&format!("[producer] Incremented: {} -> {}", prev, prev + 1));
            }
            log("[producer] Done");
        })
        .add_role("consumer", |mut ctx| {
            // Extract the producer ID from arguments.
            let args: Vec<String> = std::env::args().collect();
            let producer_id: u64 = args
                .iter()
                .find_map(|s| s.parse().ok())
                .expect("Producer ID not found in args");

            log(&format!(
                "[consumer] Started, looking for producer {} state...",
                producer_id
            ));

            // Poll until the producer has published its state.
            let mut shared_atomic: Option<Arc<mercy::sync::Mutex<i32>>> = None;
            for _ in 0..100 {
                shared_atomic = ctx.get_worker_state::<mercy::sync::Mutex<i32>>(producer_id);
                if shared_atomic.is_some() {
                    break;
                }
                sleep(Duration::from_millis(200));
            }

            let shared_atomic = shared_atomic.expect("Timed out waiting for producer state");
            log(&format!(
                "[consumer] Found state! Initial value seen: {}",
                *shared_atomic.lock()
            ));

            // Observe the changes made by the producer.
            for _ in 0..10 {
                sleep(Duration::from_secs(1));
                log(&format!(
                    "[consumer] Observed value: {}",
                    *shared_atomic.lock()
                ));
            }

            log("[consumer] Done");
        })
        .start();
}
