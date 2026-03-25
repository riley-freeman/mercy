use std::ffi::os_str::Display;

use mercy::{alloc::AllocatesTypes, boxed::Box, context::ContextBuilder, string::String};
use similar::DiffableStr;

struct Person {
    name: String,
    age: u8,
    height: f32,
}

impl std::fmt::Display for Person {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Person {{ name: {}, age: {}, height: {} }}",
            self.name, self.age, self.height
        )
    }
}

pub fn main() {
    let id = std::string::String::from("crayon.mercy.test.example.create");
    println!("[DEBUG] Creating context with id: {}", id);

    ContextBuilder::new(&id)
        .main(|ctx| {
            let mut context = ctx.unwrap();

            let contents = "HI MY NAME IS CARMEN WINSON!";
            let name = context.new_string(contents).unwrap();
            let person = context
                .new_box(Person {
                    name,
                    age: 19,
                    height: 5.8,
                })
                .unwrap();

            println!("[DEBUG] [client] Running main!");
            println!("[DEBUG] [client] Hello World!");
            println!("[DEBUG] [client] Person: {}", &person);
        })
        .build();
}
