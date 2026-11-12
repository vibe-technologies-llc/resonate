use std::{ffi::c_void, ptr::NonNull};

use block2::{Block, RcBlock};
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::encode::{Encoding, RefEncode};

use crate::error::{Error, Result};

const THE_QUEUE_IS_CALLED: &str = "dev.resonate.metered";
const SATISFIED: i32 = 1;

#[repr(C)]
struct NwPathMonitor {
    opaque: [u8; 0],
}

#[repr(C)]
struct NwPath {
    opaque: [u8; 0],
}

unsafe impl RefEncode for NwPath {
    const ENCODING_REF: Encoding = Encoding::Object;
}

type Handler = Block<dyn Fn(NonNull<NwPath>)>;

#[link(name = "Network", kind = "framework")]
unsafe extern "C" {
    fn nw_path_monitor_create() -> *mut NwPathMonitor;
    fn nw_path_monitor_set_queue(monitor: NonNull<NwPathMonitor>, queue: NonNull<DispatchQueue>);
    fn nw_path_monitor_set_update_handler(monitor: NonNull<NwPathMonitor>, handler: &Handler);
    fn nw_path_monitor_start(monitor: NonNull<NwPathMonitor>);
    fn nw_path_monitor_cancel(monitor: NonNull<NwPathMonitor>);
    fn nw_path_get_status(path: NonNull<NwPath>) -> i32;
    fn nw_path_is_expensive(path: NonNull<NwPath>) -> bool;
    fn nw_path_is_constrained(path: NonNull<NwPath>) -> bool;
    fn nw_release(object: NonNull<c_void>);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub reachable: bool,
    pub expensive: bool,
    pub constrained: bool,
}

impl Route {
    fn of(path: NonNull<NwPath>) -> Self {
        Self {
            reachable: unsafe { nw_path_get_status(path) } == SATISFIED,
            expensive: unsafe { nw_path_is_expensive(path) },
            constrained: unsafe { nw_path_is_constrained(path) },
        }
    }
}

pub struct PathMonitor {
    monitor: NonNull<NwPathMonitor>,
    _queue: DispatchRetained<DispatchQueue>,
    _handler: RcBlock<dyn Fn(NonNull<NwPath>)>,
}

impl PathMonitor {
    pub fn following(told: impl Fn(Route) + Send + 'static) -> Result<Self> {
        let monitor =
            NonNull::new(unsafe { nw_path_monitor_create() }).ok_or(Error::NoPathMonitor)?;
        let queue = DispatchQueue::new(THE_QUEUE_IS_CALLED, None);
        let handler = RcBlock::new(move |path: NonNull<NwPath>| told(Route::of(path)));

        unsafe {
            nw_path_monitor_set_queue(monitor, DispatchRetained::as_ptr(&queue));
            nw_path_monitor_set_update_handler(monitor, &handler);
            nw_path_monitor_start(monitor);
        }

        Ok(Self {
            monitor,
            _queue: queue,
            _handler: handler,
        })
    }
}

impl Drop for PathMonitor {
    fn drop(&mut self) {
        unsafe {
            nw_path_monitor_cancel(self.monitor);
            nw_release(self.monitor.cast());
        }
    }
}
