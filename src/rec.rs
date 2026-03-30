use core::time;
use std::{
    collections::{HashMap, LinkedList},
    fmt::{self, Debug, Display},
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
    alloc::{self, HasAllocId, HasInner},
    context::ContextBuilder,
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

static ALLOCATION_CALLBACKS: LazyLock<Mutex<HashMap<u128, Vec<Callback>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static STATE_POINTERS: LazyLock<Mutex<HashMap<u128, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone)]
struct Callback(Arc<Mutex<dyn FnMut(&dyn std::any::Any) + Send + 'static>>);
unsafe impl Send for Callback {}
unsafe impl Sync for Callback {}

#[derive(Clone)]
pub struct State<T: HasAllocId + Clone> {
    alloc_id: u128,
    object: Arc<Mutex<T>>,
    original_data: Vec<u8>,
}

impl<T: HasAllocId + Clone> State<T> {
    pub fn new(object: T) -> Result<State<T>, Error> {
        // Get the ptr to the data
        let ptr = alloc::map_id(&object.alloc_id())? as *const u8;
        let len = alloc::len(&object.alloc_id())? as usize;

        let original_data = unsafe { slice::from_raw_parts(ptr, len).to_vec() };

        // Remove any existing callbacks for this allocation ID
        ALLOCATION_CALLBACKS
            .lock()
            .unwrap()
            .insert(object.alloc_id(), Vec::new());

        STATE_POINTERS
            .lock()
            .unwrap()
            .insert(object.alloc_id(), ptr as usize);

        Ok(State {
            alloc_id: object.alloc_id(),
            object: Arc::new(Mutex::new(object)),
            original_data,
        })
    }

    /// Returns a `WatchGuard` that provides mutable access to the underlying data.
    /// When the `WatchGuard` is dropped, any modifications are sent to the recorder.
    pub fn watch(&self) -> Result<WatchGuard<T>, Error> {
        let lock = self.object.lock().unwrap();
        WatchGuard::new(lock.alloc_id())
    }

    pub fn get(&self) -> T {
        self.object.lock().unwrap().clone()
    }

    /// Sets the value stored in the allocation and sends changes to the recorder if recording.
    pub fn set(&mut self, value: T) {
        let len = self.original_data.len();
        let mut lock = self.object.lock().unwrap();

        // Pre-map the new value's allocation BEFORE the assignment drops the old one,
        // so we don't hit the Unix socket after the old allocation is freed.
        let old_alloc_id = self.alloc_id;
        let new_alloc_id = value.alloc_id();
        let new_ptr = alloc::map_id(&new_alloc_id).unwrap() as *const u8;
        let new_len = alloc::len(&new_alloc_id).unwrap() as usize;

        let (original, modified) = unsafe {
            let ptr = STATE_POINTERS.lock().unwrap()[&old_alloc_id];
            let original = slice::from_raw_parts(ptr as *const u8, len).to_vec();
            *lock = value;
            let modified = slice::from_raw_parts(new_ptr, new_len).to_vec();
            (original, modified)
        };

        drop(lock);

        STATE_POINTERS
            .lock()
            .unwrap()
            .insert(new_alloc_id, new_ptr as usize);
        self.alloc_id = new_alloc_id;

        Self::record_change(old_alloc_id, original, modified.clone());

        // Notify all listener callbacks of the new value.
        let mut callbacks = ALLOCATION_CALLBACKS.lock().unwrap();

        // Move around the info relating to alloc IDs
        if old_alloc_id != new_alloc_id {
            let old_callbacks = callbacks.remove(&old_alloc_id);
            if let Some(cbs) = old_callbacks {
                callbacks.insert(new_alloc_id, cbs);
            } else {
                callbacks.remove(&new_alloc_id);
            }
        }

        if let Some(recorder) = PROCESS_RECORDER.lock().unwrap().upgrade() {
            let mut lock = recorder.lock().unwrap();
            let updates = lock.updates.remove(&old_alloc_id).unwrap_or_default();
            lock.updates.insert(new_alloc_id, updates);
        }

        let listeners = callbacks.get(&self.alloc_id).cloned().unwrap_or_default();
        drop(callbacks);

        for listener in listeners {
            listener.0.lock().unwrap()(&() as &dyn std::any::Any);
        }
    }



    fn record_change(alloc_id: u128, original: Vec<u8>, modified: Vec<u8>) {
        if let Some(recorder) = PROCESS_RECORDER.lock().unwrap().upgrade() {
            println!("[DEBUG] Sending snapshot for alloc_id: {}", alloc_id);
            let snapshot = StateSnapshot {
                alloc_id,
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
    }
}

impl<T: HasInner + HasAllocId + Clone> State<T> {
    pub fn value(&self) -> T::Inner {
        self.object.lock().unwrap().clone_inner()
    }

    pub fn set_value(&mut self, value: T::Inner)
    where
        T::Inner: Send + 'static,
    {
        let len = self.original_data.len();
        let old_alloc_id = self.alloc_id;
        let mut lock = self.object.lock().unwrap();

        // Capture original bytes before mutation.
        let original = unsafe {
            let ptr = STATE_POINTERS.lock().unwrap()[&old_alloc_id];
            slice::from_raw_parts(ptr as *const u8, len).to_vec()
        };

        // set_inner may reallocate (e.g. String does realloc+free),
        // so the alloc_id and pointer can change.
        lock.set_inner(value);

        // Re-read the (possibly new) alloc_id after set_inner.
        let new_alloc_id = lock.alloc_id();
        let new_ptr = alloc::map_id(&new_alloc_id).unwrap() as *const u8;
        let new_len = alloc::len(&new_alloc_id).unwrap() as usize;

        let modified = unsafe { slice::from_raw_parts(new_ptr, new_len).to_vec() };

        let inner = lock.clone_inner();
        drop(lock);

        // Update tracked pointer.
        STATE_POINTERS
            .lock()
            .unwrap()
            .insert(new_alloc_id, new_ptr as usize);
        self.alloc_id = new_alloc_id;

        Self::record_change(old_alloc_id, original, modified);

        // Move callbacks if alloc ID changed.
        let mut callbacks = ALLOCATION_CALLBACKS.lock().unwrap();
        if old_alloc_id != new_alloc_id {
            let old_callbacks = callbacks.remove(&old_alloc_id);
            if let Some(cbs) = old_callbacks {
                callbacks.insert(new_alloc_id, cbs);
            }
        }

        if let Some(recorder) = PROCESS_RECORDER.lock().unwrap().upgrade() {
            let mut rec_lock = recorder.lock().unwrap();
            let updates = rec_lock.updates.remove(&old_alloc_id).unwrap_or_default();
            rec_lock.updates.insert(new_alloc_id, updates);
        }

        let listeners = callbacks.get(&new_alloc_id).cloned().unwrap_or_default();
        drop(callbacks);

        for listener in listeners {
            listener.0.lock().unwrap()(&inner as &dyn std::any::Any);
        }
    }

    /// Adds a callback that will be invoked whenever `set` or `set_value` is called,
    /// receiving a reference to the new inner value.
    pub fn add_listener_callback(&self, mut callback: impl FnMut(&T::Inner) + Send + 'static)
    where
        T::Inner: Send + 'static,
    {
        let callback = Callback(Arc::new(Mutex::new(move |val: &dyn std::any::Any| {
            if let Some(inner) = val.downcast_ref::<T::Inner>() {
                callback(inner);
            }
        })));
        ALLOCATION_CALLBACKS
            .lock()
            .unwrap()
            .entry(self.alloc_id)
            .or_insert_with(Vec::new)
            .push(callback);
    }
}

impl<T: Debug + HasAllocId + Clone> Debug for State<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.get())
    }
}

impl<T: Display + HasAllocId + Clone> Display for State<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.get())
    }
}

impl<T: HasInner + HasAllocId + Clone> From<T> for State<T> {
    fn from(value: T) -> Self {
        Self::new(value).unwrap()
    }
}

impl<T: HasAllocId + Clone> PartialEq for State<T> {
    fn eq(&self, other: &Self) -> bool {
        self.alloc_id == other.alloc_id
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

        if self.modified_data.is_some() {
            let listeners = ALLOCATION_CALLBACKS
                .lock()
                .unwrap()
                .get(&self.alloc_id)
                .cloned()
                .unwrap_or_default();

            for listener in listeners {
                listener.0.lock().unwrap()(&() as &dyn std::any::Any);
            }
        }
    }
}

impl<T: HasInner> Deref for WatchGuard<T> {
    type Target = T::Inner;
    fn deref(&self) -> &Self::Target {
        unsafe { &*(self.ptr as *const T::Inner) }
    }
}

impl<T: HasInner> DerefMut for WatchGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.modified_data = Some(Vec::new());
        unsafe { &mut *(self.ptr as *mut T::Inner) }
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
                b.set_value(0x2);
            }
            {
                let mut r = b.watch().unwrap();
                *r = 0x0;
                *r = 0x3;
            }
            {
                b.set_value(0x4);
            }

            recording.end_recording();
            let lock = recording.inner.lock().unwrap();
            assert_eq!(lock.updates.len(), 1);
            assert_eq!(lock.updates.get(&b.alloc_id).unwrap().len(), 4);
        })
        .start();
}

#[test]
fn state_callback_test() {
    use crate::alloc::AllocatesTypes;

    let id = String::from("crayon.mercy.test.rec.state_callback");
    ContextBuilder::new(&id)
        .main(|res| {
            let mut context = res.unwrap();
            let mut state = State::new(context.new_box(0x99_u8).unwrap()).unwrap();

            let changed = Arc::new(AtomicBool::new(false));
            let changed_clone = Arc::clone(&changed);
            state.add_listener_callback(move |_| {
                changed_clone.store(true, std::sync::atomic::Ordering::Relaxed);
            });

            assert!(!changed.load(std::sync::atomic::Ordering::Relaxed));
            assert_eq!(*state.get().as_ref(), 0x99);

            state.set(context.new_box(0x1).unwrap());
            assert!(changed.load(std::sync::atomic::Ordering::Relaxed));
            assert_eq!(state.value(), 0x1);
        })
        .start();
}

#[test]
fn test_callback_moving_logic() {
    use std::sync::atomic::Ordering;

    let old_id = 1u128;
    let new_id = 2u128;

    let mut callbacks = ALLOCATION_CALLBACKS.lock().unwrap();
    callbacks.clear();

    let called = Arc::new(AtomicBool::new(false));
    let called_clone = Arc::clone(&called);
    let cb = Callback(Arc::new(Mutex::new(move |_: &dyn std::any::Any| {
        called_clone.store(true, Ordering::Relaxed);
    })));

    // Register old callback
    callbacks.insert(old_id, vec![cb]);

    // Register a stale callback on the new ID that should be cleared
    let stale_called = Arc::new(AtomicBool::new(false));
    let stale_called_clone = Arc::clone(&stale_called);
    let stale_cb = Callback(Arc::new(Mutex::new(move |_: &dyn std::any::Any| {
        stale_called_clone.store(true, Ordering::Relaxed);
    })));
    callbacks.insert(new_id, vec![stale_cb]);

    // Simulate what's in State::set
    if old_id != new_id {
        let old_callbacks = callbacks.remove(&old_id);
        if let Some(cbs) = old_callbacks {
            callbacks.insert(new_id, cbs);
        } else {
            callbacks.remove(&new_id);
        }
    }

    for listener in callbacks.get(&new_id).unwrap() {
        listener.0.lock().unwrap()(&() as &dyn std::any::Any);
    }

    assert!(called.load(Ordering::Relaxed));
    assert!(!stale_called.load(Ordering::Relaxed));
    assert!(callbacks.get(&old_id).is_none());
    assert_eq!(callbacks.get(&new_id).unwrap().len(), 1);

    callbacks.clear();
}
