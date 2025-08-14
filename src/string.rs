use core::{fmt, slice};
use std::{fmt::Display, ops::{Add, AddAssign, Deref}};

use libc::strlen;

use crate::{alloc::{self, Allocator}, error::Error};


pub struct String {
    id: u128,
}

impl Drop for String {
    fn drop(&mut self) {
        alloc::free(&self.id);
    }
}

impl String {
    pub fn new(allocator: &mut dyn Allocator, value: &str) -> Result<Self, Error> {
        let len = value.len();
        let id = allocator.alloc(len as u32 + 1)?;

        // Write the (only byte) data into the buffer
        let ptr = match allocator.map_id(id) {
            Some(ptr) => ptr,
            // Ok mf... im drunk as fuck... I KNOW. there is a better way to do this shit... one that looks better... but take that up with the judge bitch.
            None => return Err(Error::BlockNotFound { allocation_id: id })
        };

        unsafe {
            libc::memcpy(ptr as _, value.as_ptr() as _, len);
            libc::memcpy(ptr.byte_add(len) as _ , &0 as *const _ as _, 1);
        }

        Ok(String {id})
    }

    pub fn push(&mut self, c: char) -> Result<(), Error> {
        self.push_str(&c.to_string())
    }

    pub fn push_str(&mut self, string: &str)  -> Result<(), Error> {
        // Create the old string
        let og_str = unsafe {
            let ptr = alloc::map_id(&self.id).unwrap();
            let len = libc::strlen(ptr as _);
            str::from_utf8_unchecked(slice::from_raw_parts(ptr, len))
        };

        let new_str = format!("{}{}", og_str, string);
        let new_id = alloc::realloc(&self.id, new_str.len() as u32 + 1)?;

        // Write the new data
        unsafe {
            let ptr = alloc::map_id(&new_id).unwrap();
            let len = new_str.len();
            libc::memcpy(ptr as _, new_str.as_ptr() as _, len);
            libc::memcpy(ptr.byte_add(len)as _, &0 as *const _ as _, 1);
        }

        // Free that old shit lowkey
        alloc::free(&self.id);
        self.id = new_id;

        Ok(())
    }

    pub fn try_clone(&self) -> Result<Self, Error> {
        let ptr = alloc::map_id(&self.id).ok_or(Error::BlockNotFound { allocation_id: self.id })?;
        let len = unsafe { strlen(ptr as _) } + 1;
        
        let new_id = alloc::realloc(&self.id, len as _)?;
        let new_ptr = alloc::map_id(&new_id).ok_or(Error::BlockNotFound { allocation_id: self.id })?;

        unsafe { libc::memcpy(new_ptr as _, ptr as _, len) };

        Ok(Self { id: new_id })
    }
}

impl AsRef<str>  for String {
    fn as_ref(&self) -> &str {
        unsafe {
            let ptr = alloc::map_id(&self.id).unwrap();
            let len = strlen(ptr as _);
            let slice = slice::from_raw_parts(ptr, len);
            str::from_utf8_unchecked(slice)
        }
    }
}

impl Deref for String {
    type Target = str; 
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl Display for String {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}

impl Clone for String {
    fn clone(&self) -> Self {
        self.try_clone().unwrap()
    }
}

impl Add<&str> for String {
    type Output = String;
    fn add(self, rhs: &str) -> Self::Output {
        let mut s = self;
        s.push_str(rhs).unwrap();
        s
    }
}

impl AddAssign<&str> for String {
    fn add_assign(&mut self, rhs: &str) {
        self.push_str(rhs).unwrap();
    }
}

impl Extend<char> for String {
    fn extend<T: IntoIterator<Item = char>>(&mut self, iter: T) {
        let string: std::string::String = iter.into_iter().collect();
        self.push_str(&string).unwrap();
    }
}

impl<'a> Extend<&'a char> for String {
    fn extend<T: IntoIterator<Item = &'a char>>(&mut self, iter: T) {
        self.extend(iter.into_iter().cloned());
    }
}

impl<'a> Extend<&'a str> for String {
    fn extend<T: IntoIterator<Item = &'a str>>(&mut self, iter: T) {
        iter.into_iter().for_each(move |s| self.push_str(s).unwrap());
    }
}

impl<'a> Extend<String> for String {
    fn extend<T: IntoIterator<Item = String>>(&mut self, iter: T) {
        iter.into_iter().for_each(move |s| self.push_str(&s).unwrap());
    }
}

impl<'a> Extend<std::string::String> for String {
    fn extend<T: IntoIterator<Item = std::string::String>>(&mut self, iter: T) {
        iter.into_iter().for_each(move |s| self.push_str(&s).unwrap());
    }
}


#[test]
fn the_string_test() {
    use crate::context::ContextBuilder;
    use crate::alloc::AllocatesTypes;

    let id = std::string::String::from("crayon.mercy.test.string");

    println!("Opening context with id: {}", id);
    tracing::debug!("Opening context with id: {}", id);
    let mut context = ContextBuilder::new(&id)
        .build_or_open()
        .unwrap(); 

    let mut string = context.new_string("Hello, World").unwrap();
    assert_eq!(string.as_ref(), "Hello, World");

    string.push_str(", from a concatenated x99 STRING").unwrap();
    assert_eq!(string.as_ref(), "Hello, World, from a concatenated x99 STRING");

    let string = string + " - x99 MERCY!";
    assert_eq!(string.as_ref(), "Hello, World, from a concatenated x99 STRING - x99 MERCY!");


    let mut string = context.new_string("August").unwrap();
    string += " 13";
    string += ", 2025";

    assert_eq!(string.as_ref(), "August 13, 2025");
}

#[test]
fn the_extend_test() {
    use crate::context::ContextBuilder;
    use crate::alloc::AllocatesTypes;

    let id = std::string::String::from("crayon.mercy.test.string.extend");

    println!("Opening context with id: {}", id);
    tracing::debug!("Opening context with id: {}", id);
    let mut context = ContextBuilder::new(&id)
        .build_or_open()
        .unwrap();

    let mut string = context.new_string("").unwrap();

    let chars = ['A', 'B', 'C'];
    string.extend(chars);


    let strs = ["DEF", "GHI", "JK"];
    string.extend(strs);

    let chars = ['L', 'M', 'N', 'O', 'P'];
    let chars_ref = chars.iter().collect::<Vec<&char>>();
    string.extend(chars_ref);

    let strs = ["QRS", "TUV"];
    let strings: Vec<String> = strs.iter().map(|s| context.new_string(s).unwrap()).collect();
    string.extend(strings);

    let strs = ["WXY"];
    let strings: Vec<std::string::String> = strs.iter().map(|s| s.to_string()).collect();
    string.extend(strings);

    string.extend(['Z']);

    assert_eq!(string.as_ref(), "ABCDEFGHIJKLMNOPQRSTUVWXYZ");
}

