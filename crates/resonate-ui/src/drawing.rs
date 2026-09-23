use std::{future::Future, thread};

use crossbeam_channel::{Receiver, SendError, Sender, unbounded};
use futures_channel::oneshot;
use gpui::Global;

const DRAWING_THREADS: usize = 2;

const DRAWING_THREAD: &str = "resonate-drawing";

type Job = Box<dyn FnOnce() + Send>;

pub(crate) struct Drawer {
    jobs: Sender<Job>,
}

impl Global for Drawer {}

impl Drawer {
    pub(crate) fn new() -> Self {
        let (jobs, waiting) = unbounded::<Job>();
        for _ in 0..DRAWING_THREADS {
            let taken = waiting.clone();
            let spawned = thread::Builder::new()
                .name(DRAWING_THREAD.to_owned())
                .spawn(move || draw_until_dropped(&taken));
            if let Err(error) = spawned {
                tracing::warn!(%error, "a drawing thread could not be started");
            }
        }

        Self { jobs }
    }

    pub(crate) fn draw<T, W>(&self, work: W) -> impl Future<Output = Option<T>> + use<T, W>
    where
        T: Send + 'static,
        W: FnOnce() -> T + Send + 'static,
    {
        let (drawn, landed) = oneshot::channel();
        let job: Job = Box::new(move || {
            let _ = drawn.send(work());
        });
        if let Err(SendError(job)) = self.jobs.send(job) {
            job();
        }

        async move { landed.await.ok() }
    }
}

fn draw_until_dropped(jobs: &Receiver<Job>) {
    for job in jobs {
        job();
    }
}
