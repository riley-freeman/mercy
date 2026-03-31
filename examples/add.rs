use mercy::context::ContextBuilder;
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Write},
    thread::sleep,
    time::Duration,
};

#[derive(Serialize, Deserialize)]
struct Payload {
    a: i64,
    b: i64,
}

fn ask(prompt: &str) -> i64 {
    print!("[main] {}: ", prompt);
    io::stdout().flush().ok();
    let mut s = String::new();
    io::stdin().read_line(&mut s).ok();
    s.trim().parse().unwrap_or(0)
}

fn main() {
    ContextBuilder::new("com.example.add")
        .main(|ctx| {
            let mut ctx = ctx.unwrap();
            let (a, b) = (ask("A"), ask("B"));
            let (tx, rx) = std::sync::mpsc::channel();

            ctx.new_worker("calculator", vec![])
                .unwrap()
                .send_message(Payload { a, b }, move |v| {
                    tx.send(v).ok();
                })
                .unwrap();

            let res: i64 = rx.recv().unwrap().deserialize_into().unwrap();
            println!("[main] {a} + {b} = {res}");
            ctx.close();
        })
        .add_role("calculator", |ctx| {
            let (tx, rx) = std::sync::mpsc::channel();
            ctx.unwrap().set_message_callback(move |v| {
                let p: Payload = v.deserialize_into().unwrap();
                tx.send(()).unwrap();
                Some(serde_value::Value::I64(p.a + p.b))
            });
            rx.recv().ok();
            sleep(Duration::from_millis(100));
        })
        .start();
}
