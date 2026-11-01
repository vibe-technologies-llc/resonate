use std::marker::PhantomData;

use windows::Win32::{
    Foundation::HANDLE,
    System::Threading::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW},
};
use windows_core::w;

use crate::error::{Op, Result, refused};

type BoundToItsThread = PhantomData<*const ()>;

pub struct ProAudioClass {
    task: HANDLE,
    thread: BoundToItsThread,
}

impl ProAudioClass {
    pub fn join() -> Result<Self> {
        let mut task_index = 0_u32;
        let task = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &raw mut task_index) }
            .map_err(refused(Op::JoinProAudio))?;
        Ok(Self {
            task,
            thread: PhantomData,
        })
    }

    pub fn leave(self) -> Result<()> {
        let task = self.task;
        std::mem::forget(self);
        left(task)
    }
}

fn left(task: HANDLE) -> Result<()> {
    unsafe { AvRevertMmThreadCharacteristics(task) }.map_err(refused(Op::LeaveProAudio))
}

impl Drop for ProAudioClass {
    fn drop(&mut self) {
        let _ = left(self.task);
    }
}
