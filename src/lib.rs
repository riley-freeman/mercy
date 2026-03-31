pub mod alloc;
pub mod boxed;
pub mod context;
pub mod error;
pub mod macros;
pub mod message;
pub mod rec;
pub mod string;
pub mod sync;
pub mod vec;
pub mod worker;

mod header;
mod mapping;
mod pal;

use header::Version;
static VERSION: Version = header::Version {
    major: 0,
    minor: 0,
    patch: 1,
};

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::fmt::Debug;

    use crate::alloc::{AllocatesTypes, Allocator, HasAllocId};
    use crate::context::ContextBuilder;

    #[test]
    fn hello_world() {
        println!("Hello, world!");
        tracing::debug!("Hello, world!");
    }

    #[test]
    fn create_context() {
        let id = String::from("crayon.mercy.test.create");

        println!("Creating context with id: {}", id);
        tracing::debug!("Creating context with id: {}", id);
        ContextBuilder::new(&id).start();
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
        let id = String::from("crayon.mercy.test.manifest");

        println!("Opening context with id: {}", id);
        tracing::debug!("Opening context with id: {}", id);
        ContextBuilder::new(&id).start();
    }

    #[test]
    fn the_long_family_id_test() {
        let id = String::from(
            "crayon.mercy.test.this_is_an_extremely_long_id_that_should_not_be_used_in_production_because_it_is_too_long",
        );

        println!("Creating context with id: {}", id);
        tracing::debug!("Creating context with id: {}", id);
        let _context = ContextBuilder::new(&id)
            .main(|_context| println!("Successfully made it to main!"))
            .start();
    }

    #[test]
    fn the_alloc_test() {
        let id = String::from("crayon.mercy.test.alloc");

        println!("Opening context with id: {}", id);
        tracing::debug!("Opening context with id: {}", id);
        ContextBuilder::new(&id)
            .main(|res| {
                let mut context = res.unwrap();
                let alloc_id_0 = context.alloc(1024).unwrap();
                let alloc_id_1 = context.alloc(2048).unwrap();
                let alloc_id_2 = context.alloc(4096).unwrap();

                context.free(alloc_id_0);
                context.free(alloc_id_1);
                context.free(alloc_id_2);
            })
            .start();
    }

    #[test]
    fn the_box_test() {
        let id = String::from("crayon.mercy.test.box");

        println!("Opening context with id: {}", id);
        tracing::debug!("Opening context with id: {}", id);
        ContextBuilder::new(&id)
            .main(|res| {
                let mut context = res.unwrap();

                #[derive(Debug, Clone, PartialEq)]
                #[allow(unused)]
                struct Person {
                    name: [u8; 10],
                    age: u64,
                    race: [u8; 3],
                    dead: bool,
                }

                let mark_sadiki = Person {
                    name: b"MarkSadiki".clone(),
                    age: 19,
                    race: b"BLK".clone(),
                    dead: false, // Not yet.
                };

                let in_box =
                    crate::rec::State::new(context.new_box(mark_sadiki.clone()).unwrap()).unwrap();
                assert_eq!(mark_sadiki, in_box.watch().unwrap().clone());
            })
            .start();
    }

    #[test]
    fn the_arc_test() {
        let id = String::from("crayon.mercy.test.arc");

        println!("Opening context with id: {}", id);
        tracing::debug!("Opening context with id: {}", id);
        ContextBuilder::new(&id)
            .main(|res| {
                let mut context = res.unwrap();

                let i = context.new_arc(u16::MAX).unwrap();
                println!("Hello World: {}", &i);

                let j = i.clone();
                println!("Hello World: {}", &j);
                std::mem::drop(j);

                let k: crate::sync::Arc<dyn Any + Send + Sync> = i.clone().into();
                let k2: crate::sync::Arc<u16> = match k.downcast() {
                    Ok(r) => {
                        println!("SUCCESS... UP...? TRANSFORMING ANY TO I32");
                        r
                    }
                    Err(_) => {
                        println!("MESSED UP TRANSFORMING ANY TO I32");
                        i.clone()
                    }
                };

                println!("Hello World: {}", &k2);
            })
            .start();
    }

    #[test]
    fn the_weak_test() {
        let id = String::from("crayon.mercy.test.weak");

        println!("Opening context with id: {}", id);
        tracing::debug!("Opening context with id: {}", id);
        ContextBuilder::new(&id)
            .main(|res| {
                let mut context = res.unwrap();
                let i = context.new_arc(u16::MAX).unwrap();

                let weak = crate::sync::Arc::downgrade(&i).unwrap();
                assert_eq!(*weak.upgrade().unwrap().as_ref(), u16::MAX);

                std::mem::drop(i);
                assert!(weak.upgrade().is_err());
            })
            .start();
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
