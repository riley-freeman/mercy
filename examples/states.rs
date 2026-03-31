use mercy::alloc::AllocatesTypes;
use mercy::context::ContextBuilder;
use mercy::rec::State;
use std::sync::{
    Arc,
    atomic::{AtomicI32, Ordering},
};

fn main() {
    ContextBuilder::new("com.example.states")
        .main(|mut ctx| {
            println!("--- Mercy States Example ---");

            let initial_value = 42;
            let boxed = ctx.new_box(initial_value).expect("Failed to create box");
            let mut state = State::new(boxed).expect("Failed to create state");

            println!("[main] Initial state value: {}", state.value());

            let shared_counter = Arc::new(AtomicI32::new(initial_value));
            let counter_clone = Arc::clone(&shared_counter);

            state.add_listener_callback(move |&new_val: &i32| {
                println!("[listener] Callback triggered! New value is: {}", new_val);
                counter_clone.store(new_val, Ordering::Relaxed);
            });

            println!("[main] Updating state to 100 via set_value()...");
            state.set_value(100);

            assert_eq!(state.value(), 100);
            assert_eq!(shared_counter.load(Ordering::Relaxed), 100);
            println!("[main] State value after set_value: {}", state.value());

            println!("[main] Updating state to 200 via watch() guard...");
            {
                let mut guard = state.watch().expect("Failed to watch state");
                *guard = 200;
                println!("[main]  -> Value modified inside guard scope: {}", *guard);
                println!("[main]  -> Listener hasn't fired yet...");
            } // Guard dropped here, triggering the listener.

            assert_eq!(state.value(), 200);
            assert_eq!(shared_counter.load(Ordering::Relaxed), 200);
            println!(
                "[main] State value after watch guard dropped: {}",
                state.value()
            );

            println!("\n[main] Example completed successfully!");

            // Clean up the context before exiting.
            ctx.close();
        })
        .start();
}
