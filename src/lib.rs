pub mod alloc;
pub mod context;
pub mod error;
pub mod boxed;

mod mapping;
mod header;

use header::Version;
static VERSION: Version = header::Version {
    major: 0,
    minor: 0,
    patch: 1,
};


#[cfg(test)]
mod tests {
    use crate::alloc::{Allocator, AllocatesTypes};
    use crate::context::ContextBuilder;

    #[test]
    fn hello_world() {
        println!("Hello, world!");
        tracing::debug!("Hello, world!");
    }

    #[test]
    fn create_context() {
        let id = String::from("crayon.mercy.test");

        println!("Creating context with id: {}", id);
        tracing::debug!("Creating context with id: {}", id);
        let context = ContextBuilder::new(&id)
            .build()
            .unwrap();
        println!("Context created successfully: {:?}", context);
    }

    // #[test]
    // fn open_context() {
    //     let id = String::from("crayon.mercy.test");

    //     println!("Opening context with id: {}", id);
    //     tracing::debug!("Opening context with id: {}", id);
    //     let context = ContextBuilder::new(&id)
    //         .open()
    //         .unwrap();
    //     println!("Context opened successfully: {:?}", context);
    // }

    #[test]
    fn manifest_context() {
        let id = String::from("crayon.mercy.test");

        println!("Opening context with id: {}", id);
        tracing::debug!("Opening context with id: {}", id);
        let context = ContextBuilder::new(&id)
            .build_or_open()
            .unwrap();
        println!("Context opened successfully: {:?}", context);
    }

    #[test]
    fn test_long_context_id() {
        let id = String::from("crayon.mercy.test.this_is_an_extremely_long_id_that_should_not_be_used_in_production_because_it_is_too_long");

        println!("Creating context with id: {}", id);
        tracing::debug!("Creating context with id: {}", id);
        let context = ContextBuilder::new(&id)
            .build_or_open()
            .unwrap();
        println!("Context created successfully: {:?}", context);
    }

    // #[test]
    // #[allow(unreachable_code)]
    // fn panic_test() {
    //     let id = String::from("crayon.mercy.test");

    //     println!("Opening context with id: {}", id);
    //     tracing::debug!("Opening context with id: {}", id);
    //     let context = ContextBuilder::new(&id)
    //         .build_or_open()
    //         .unwrap();
    //     println!("Context opened successfully: {:?}", context);

    //     let mut _context_inner = context.inner.lock().unwrap();

    //     assert_eq!(_context_inner.mappings.len(), 1);
    // }

    #[test]
    fn alloc_kb() {
        let id = String::from("crayon.mercy.test");

        println!("Opening context with id: {}", id);
        tracing::debug!("Opening context with id: {}", id);
        let mut context = ContextBuilder::new(&id)
            .build_or_open()
            .unwrap();

        let alloc_id_0 = context.alloc(1024).unwrap();
        let alloc_id_1 = context.alloc(2048).unwrap();
        let alloc_id_2 = context.alloc(4096).unwrap();

        context.free(alloc_id_0);
        context.free(alloc_id_1);
        context.free(alloc_id_2);
    }

    #[test]
    fn alloc_boxes() {
        let id = String::from("crayon.mercy.test");

        println!("Opening context with id: {}", id);
        tracing::debug!("Opening context with id: {}", id);
        let mut context = ContextBuilder::new(&id)
            .build_or_open()
            .unwrap();

        #[derive(Debug)]
        #[allow(unused)]
        struct Person {
            name: [u8; 10],
            age: u64,
            race: [u8; 3],
            dead: bool
        }

        let mark_sadiki = Person {
            name: b"MarkSadiki".clone(),
            age: 18,
            race: b"BLK".clone(),
            dead: false     // Not yet.
        };
        println!("Person: {:?}", mark_sadiki);

        let in_box = context.new_box(mark_sadiki).unwrap();
        println!("Person in box: {:?}", in_box.as_ref().unwrap());

    }

    /*
    #[test]
    fn create_allocator() {
        use std::fs::File;
        use std::io::prelude::*;

        let id = String::from("crayon.mercy.test");
        let size = 2048;

        println!("Creating context with id: {} and size: {}", id, size);
        tracing::debug!("Creating context with id: {} and size: {}", id, size);
        let context = ContextBuilder::new(&id)
            .size(size + std::mem::size_of::<alloc::Block>())
            .build()
            .unwrap();

        let mut inner = context.lock().unwrap();
        let _allocator = alloc::Allocator::new(&mut inner, 0, size as u64).unwrap();
        
        unsafe {
            // Write the memory to a file for debugging
            let mut file = File::create("shared_memory_dump0.bin").unwrap();
            let slice = std::slice::from_raw_parts(inner.mapping.ptr_mut(), inner.mapping.size());
            file.write_all(slice).unwrap();
        };
    }
    */
}
