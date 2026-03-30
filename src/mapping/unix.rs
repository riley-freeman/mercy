use std::{fmt::Display, os::unix::io::RawFd};
use num_traits::{Zero, PrimInt};
use tracing::debug;

use crate::error::Error;

#[derive(Debug)]
pub struct UnixMapping {
    ptr: usize,
    size: usize,
    fd: RawFd,

    os_id: String,

    owned: bool,
}


pub fn new_mapping(id: &str, size: usize) -> Result<UnixMapping, Error> {
    let os_id = format!("/{}", id); // shm_open requires a leading slash
    let os_id_c = std::ffi::CString::new(os_id.clone()).unwrap();
    debug!("MERCY: attempting to create new shared memory with id: {}", id);

    // Size must be greater than 0
    if size == 0 {
        return Err(Error::InvalidSize{ size });
    }

    // TODO: Check if id is too long
    println!("OS ID: {}", id);
    let fd = unsafe { conv_unix_code(libc::shm_open(
        os_id_c.as_ptr(),
        libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,    // EXCL means fail if it already exists
        0o600                                           // allow user read/write
    ), id) }?;

    // Enlarge the segment to the requested size
    debug!("MERCY: enlarging shared memory segment to size: {}", size);
    unsafe { conv_unix_code(libc::ftruncate(fd, size as i64), id) }?;

    // Map the segment into our process's address space
    // Actual question btw, how much do address spaces cost? like without them how much faster
    // would everything be?
    debug!("MERCY: mapping shared memory segment into process's address space");
    let ptr_raw = unsafe { libc::mmap(
        std::ptr::null_mut(),
        size,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_SHARED,
        fd,
        0
    ) };
    if ptr_raw == libc::MAP_FAILED {
        return Err(Error::MiscellaneousOSError { code: errno::errno().0 });
    }
    let ptr = ptr_raw as usize;

    Ok(UnixMapping {  ptr, size, fd, os_id, owned: true })
}

pub fn open_mapping(id: &str) -> Result<UnixMapping, Error> {
    let os_id = format!("/{}", id); // shm_open requires a leading slash
    let os_id_c = std::ffi::CString::new(os_id.clone()).unwrap();

    // Opening a shared memory fd
    debug!("MERCY: attempting to open shared memory with ID: {}", id);
    let fd = unsafe { conv_unix_code(libc::shm_open(
        os_id_c.as_ptr(),
        libc::O_RDWR,
        0o600
    ), id) }?;

    // Get the block size
    let size = unsafe {
        let mut file_stat: libc::stat = std::mem::zeroed();
        conv_unix_code(libc::fstat(fd, &mut file_stat as *mut libc::stat), id)?;
        file_stat.st_size as usize
    };

    // Check if the size is valid
    if size == 0 {
        return Err(Error::InvalidSize { size });
    }

    // Map the memroy into our process's address space
    let ptr_raw = unsafe { libc::mmap(
        std::ptr::null_mut(),
        size,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_SHARED,
        fd,
        0
    ) };
    if ptr_raw == libc::MAP_FAILED {
        return Err(Error::MiscellaneousOSError { code: errno::errno().0 });
    }
    let ptr = ptr_raw as usize;

    Ok(UnixMapping { ptr, size, fd, os_id, owned: false })
}

fn conv_unix_code<T: PrimInt + Zero + Display>(res: T, id: &str) -> Result<T, Error> {
    // get the error code
    let errno = errno::errno();

    if res >= T::zero() { Ok(res) }
    else {
        match errno.0 {
            libc::EINVAL => Err(Error::InvalidSize { size: 0 }),
            libc::EACCES => Err(Error::InvalidPermissions { id: String::from(id) }),
            libc::EEXIST => Err(Error::IdAlreadyExists { id: String::from(id) }),
            libc::EMFILE => Err(Error::ProcessLimitReached),
            _ => Err(Error::MiscellaneousOSError { code: errno.0 }),
        }
    }

}

impl Drop for UnixMapping {
    fn drop(&mut self) {
        debug!("MERCY: dropping shared memory segment");


        debug!("MERCY: unmapping shared memory segment: {:?}", self.ptr);
        if (unsafe { libc::munmap(self.ptr as _, self.size) }) != 0 {
            debug!("MERCY: failed to unmap shared memory segment: {:?}", self.ptr); // Do we really
                                                                                    // care though?
        };

        // We don't want to unlink the segment if we're panicking, because that would
        // cause the segment to be deleted. We could possibly recover some data.
        if !std::thread::panicking() && self.owned {
            let os_id_c = std::ffi::CString::new(self.os_id.clone()).unwrap();
            if (unsafe { libc::shm_unlink(os_id_c.as_ptr()) }) != 0 {
                debug!("MERCY: failed to unlink shared memory segment: {:?}", self.fd);     // We_care_but_honestly_can't_do_anything
                                                                                            // Underscored_because_by_IDE_automatically
                                                                                            // makes_new_lines_and_i'm_too_lazy_to_figure
                                                                                            // out_how_to_fix_it
            };

            if (unsafe { libc::close(self.fd) }) != 0 {
                debug!("MERCY: failed to close shared memory segment: {:?}", self.fd);
            };
        }
    }
}

impl super::Mapping for UnixMapping {
    fn size(&self) -> usize {
        self.size
    }

    fn ptr(&self) -> *const u8 {
        self.ptr as *const u8
    }

    fn ptr_mut(&mut self) -> *mut u8 {
        self.ptr as *mut u8
    }

    fn is_owner(&self) -> bool {
        self.owned
    }

    unsafe fn set_ownership(&mut self, status: bool) {
        self.owned = status;
    }
}
