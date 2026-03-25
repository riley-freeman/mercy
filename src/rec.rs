use core::time;
use std::{
    collections::{HashMap, LinkedList},
    marker::PhantomData,
    ops::{Deref, DerefMut},
    slice,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::AtomicBool,
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, SystemTime},
};

use crate::{
    alloc::{self, HasAllocId},
    error::Error,
};

use similar::DiffOp;

static PROCESS_RECORDER: LazyLock<Mutex<WeakRecorder>> = LazyLock::new(|| Mutex::new(Weak::new()));

const SLEEP_DURATION: Duration = time::Duration::from_secs(0);

#[derive(Debug, Clone)]
pub struct StateSnapshot {
    alloc_id: u128,
    original_data: Vec<u8>,
    modified_data: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct RecorderInner {
    _begin_time: SystemTime,
    updates: HashMap<u128, LinkedList<Update>>,
    recording: AtomicBool,
    queue_thread: Option<std::thread::JoinHandle<()>>,
    queue_sender: Sender<StateSnapshot>,
    queue_receiver: Receiver<StateSnapshot>,
}

#[derive(Debug, Clone)]
pub struct Recorder {
    inner: Arc<Mutex<RecorderInner>>,
}

type WeakRecorder = std::sync::Weak<std::sync::Mutex<RecorderInner>>;

impl Default for RecorderInner {
    fn default() -> Self {
        let channel = mpsc::channel();
        RecorderInner {
            _begin_time: SystemTime::now(),
            recording: AtomicBool::new(false),
            queue_thread: None,
            queue_sender: channel.0,
            queue_receiver: channel.1,
            updates: HashMap::new(),
        }
    }
}

impl Recorder {
    pub fn new() -> Result<Recorder, Error> {
        let mut lock = PROCESS_RECORDER.lock().unwrap();
        match lock.upgrade() {
            Some(r) => Ok(Recorder { inner: r }),
            None => {
                let rec = Recorder {
                    inner: Arc::new(Mutex::new(RecorderInner::default())),
                };
                *lock = Arc::downgrade(&rec.inner);
                Ok(rec)
            }
        }
    }

    pub fn begin_recording(&mut self) {
        let clone = self.clone();
        let mut lock = self.inner.lock().unwrap();

        lock.recording = AtomicBool::new(true);
        lock.queue_thread = Some(std::thread::spawn(move || {
            let ordering = std::sync::atomic::Ordering::Relaxed;
            loop {
                // Check if we should continue recording
                if !clone.inner.lock().unwrap().recording.load(ordering) {
                    return;
                }

                process_queue(clone.clone());

                std::thread::sleep(SLEEP_DURATION);
            }
        }));
    }

    pub fn end_recording(&mut self) {
        let mut lock = self.inner.lock().unwrap();
        lock.recording
            .store(false, std::sync::atomic::Ordering::Relaxed); // i THINK relaxed is fine...

        let thread = std::mem::replace(&mut lock.queue_thread, None);
        std::mem::drop(lock);

        match thread {
            Some(res) => {
                let _ = res.join();
            }
            None => {}
        }

        // Flush the queue (might as well on this thread...)
        process_queue(self.clone());
    }
}

fn process_queue(recorder: Recorder) {
    // Extract the receiver outside the lock scope to avoid borrow conflicts
    let rs: Vec<StateSnapshot> = {
        let lock = recorder.inner.lock().unwrap();
        lock.queue_receiver.try_iter().collect()
    };

    for r in rs {
        process_reference(recorder.clone(), r);
    }

    std::thread::sleep(SLEEP_DURATION);
}

fn process_reference(recorder: Recorder, r: StateSnapshot) {
    if let Some(modified_data) = r.modified_data {
        let update = Update::new(r.alloc_id, &r.original_data, &modified_data);

        // Lock mutably only when updating the updates map
        let mut lock = recorder.inner.lock().unwrap();
        lock.updates
            .entry(r.alloc_id)
            .or_insert_with(LinkedList::new)
            .push_back(update);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    update_time: SystemTime,
    alloc_id: u128,
    changes: Vec<DiffOp>,
}

impl Update {
    pub fn new(alloc_id: u128, original: &[u8], modified: &[u8]) -> Update {
        let changes = similar::capture_diff_slices(similar::Algorithm::Myers, original, modified);
        Update {
            update_time: SystemTime::now(),
            alloc_id,
            changes,
        }
    }
}

pub struct State<T: HasAllocId + Clone> {
    object: T,
    ptr: usize,
    original_data: Vec<u8>,
    modified_data: Option<Vec<u8>>,
    listeners: Vec<Box<dyn FnMut(&T::Inner)>>,
}

impl<T: HasAllocId + Clone> Drop for State<T> {
    fn drop(&mut self) {
        // Tell the recorder to record whatever the fuck
        if let Some(recorder) = PROCESS_RECORDER.lock().unwrap().upgrade() {
            // Get the ptr to the data
            let data = match &self.modified_data {
                Some(_) => unsafe {
                    Some(
                        slice::from_raw_parts(self.ptr as *const u8, self.original_data.len())
                            .to_vec(),
                    )
                },
                None => None,
            };

            let snapshot = StateSnapshot {
                alloc_id: self.object.alloc_id(),
                original_data: self.original_data.clone(),
                modified_data: data,
            };
            recorder
                .lock()
                .unwrap()
                .queue_sender
                .send(snapshot)
                .unwrap();
        }
    }
}

impl<T: HasAllocId + Clone> State<T> {
    pub fn new(object: T) -> Result<State<T>, Error> {
        // Get the ptr to the data
        let ptr = alloc::map_id(&object.alloc_id())? as *const u8;
        let len = alloc::len(&object.alloc_id())? as usize;

        let original_data = unsafe { slice::from_raw_parts(ptr, len).to_vec() };

        Ok(State {
            object,
            ptr: ptr as usize,
            original_data,
            modified_data: None,
            listeners: Vec::new(),
        })
    }

    /// Returns a `WatchGuard` that provides mutable access to the underlying data.
    /// When the `WatchGuard` is dropped, any modifications are sent to the recorder.
    pub fn watch(&self) -> Result<WatchGuard<T::Inner>, Error> {
        WatchGuard::new(self.object.alloc_id())
    }

    /// Returns a clone of the object handle stored in this state.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.object.clone()
    }

    /// Sets the value stored in the allocation and sends changes to the recorder if recording.
    pub fn set(&mut self, value: T::Inner) {
        let len = self.original_data.len();
        let original = unsafe { slice::from_raw_parts(self.ptr as *const u8, len).to_vec() };

        unsafe {
            *(self.ptr as *mut T::Inner) = value;
        }

        let modified = unsafe { slice::from_raw_parts(self.ptr as *const u8, len).to_vec() };

        if let Some(recorder) = PROCESS_RECORDER.lock().unwrap().upgrade() {
            let snapshot = StateSnapshot {
                alloc_id: self.object.alloc_id(),
                original_data: original,
                modified_data: Some(modified),
            };
            recorder
                .lock()
                .unwrap()
                .queue_sender
                .send(snapshot)
                .unwrap();
        }

        // Notify all listener callbacks of the new value.
        let new_value = unsafe { &*(self.ptr as *const T::Inner) };
        for listener in &mut self.listeners {
            listener(new_value);
        }
    }

    /// Adds a callback that will be invoked whenever `set` is called, receiving
    /// a reference to the new value.
    pub fn add_listener_callback(&mut self, callback: impl FnMut(&T::Inner) + 'static) {
        self.listeners.push(Box::new(callback));
    }
}

/// A scoped mutable reference to allocated data. When dropped, captures the
/// current state of the data and sends it to the recorder for diff tracking.
pub struct WatchGuard<T> {
    _phantom: PhantomData<T>,
    alloc_id: u128,
    ptr: usize,
    len: usize,
    original_data: Vec<u8>,
    modified_data: Option<Vec<u8>>,
}

impl<T> WatchGuard<T> {
    pub fn new(alloc_id: u128) -> Result<WatchGuard<T>, Error> {
        let ptr = alloc::map_id(&alloc_id)? as *const u8;
        let len = alloc::len(&alloc_id)? as usize;

        let original_data = unsafe { slice::from_raw_parts(ptr, len).to_vec() };

        Ok(WatchGuard {
            _phantom: PhantomData,
            alloc_id,
            ptr: ptr as usize,
            len,
            original_data,
            modified_data: None,
        })
    }
}

impl<T> Drop for WatchGuard<T> {
    fn drop(&mut self) {
        if let Some(recorder) = PROCESS_RECORDER.lock().unwrap().upgrade() {
            let data = match &self.modified_data {
                Some(_) => unsafe {
                    Some(slice::from_raw_parts(self.ptr as *const u8, self.len).to_vec())
                },
                None => None,
            };

            let snapshot = StateSnapshot {
                alloc_id: self.alloc_id,
                original_data: self.original_data.clone(),
                modified_data: data,
            };
            recorder
                .lock()
                .unwrap()
                .queue_sender
                .send(snapshot)
                .unwrap();
        }
    }
}

impl<T> Deref for WatchGuard<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*(self.ptr as *const T) }
    }
}

impl<T> DerefMut for WatchGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.modified_data = Some(Vec::new());
        unsafe { &mut *(self.ptr as *mut T) }
    }
}

#[test]
fn the_first_recording_test() {
    use crate::alloc::AllocatesTypes;
    use crate::context::ContextBuilder;

    let id = String::from("crayon.mercy.test.rec");
    ContextBuilder::new(&id)
        .main(|res| {
            let mut context = res.unwrap();
            let mut recording = Recorder::new().unwrap();
            recording.begin_recording();

            let mut b = State::new(context.new_box(0x99_u8).unwrap()).unwrap();

            {
                let mut r = b.watch().unwrap();
                *r = 0x1;
            }
            {
                b.set(0x2);
            }
            {
                let mut r = b.watch().unwrap();
                *r = 0x0;
                *r = 0x3;
            }
            recording.end_recording();

            println!("RECORDED DATA: {:?}", recording);
        })
        .start();
}
