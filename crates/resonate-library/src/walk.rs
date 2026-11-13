use std::{
    collections::VecDeque,
    hash::Hash,
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
};

use ahash::AHashSet;
use crossbeam_channel::{Sender, bounded};
use parking_lot::Mutex;

pub const PICTURES_WAITING_AT_MOST: usize = 64;

pub const PERFORMERS_WEIGHED_AT_MOST: u16 = 32;

#[derive(Debug)]
pub struct Seeks<S>(Mutex<Vec<S>>);

impl<S> Default for Seeks<S> {
    fn default() -> Self {
        Self(Mutex::new(Vec::new()))
    }
}

impl<S: PartialEq> Seeks<S> {
    pub fn seek(&self, seek: S) {
        let mut asked = self.0.lock();
        asked.retain(|held| *held != seek);
        asked.push(seek);
    }

    pub fn taken(&self) -> Vec<S> {
        std::mem::take(&mut *self.0.lock())
    }
}

pub struct Queue<K, A> {
    waiting: VecDeque<(K, A)>,
    spent: AHashSet<K>,
}

impl<K: Clone + Eq + Hash, A> Default for Queue<K, A> {
    fn default() -> Self {
        Self {
            waiting: VecDeque::new(),
            spent: AHashSet::new(),
        }
    }
}

impl<K: Clone + Eq + Hash, A> Queue<K, A> {
    pub fn of(asks: impl IntoIterator<Item = (K, A)>) -> Self {
        let mut queue = Self::default();
        queue.extend(asks);
        queue
    }

    pub fn extend(&mut self, asks: impl IntoIterator<Item = (K, A)>) {
        self.waiting.extend(asks);
    }

    pub fn len(&self) -> usize {
        self.waiting.len()
    }

    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    pub fn is_spent(&self, key: &K) -> bool {
        self.spent.contains(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.waiting.iter().map(|(key, _)| key)
    }

    fn taken_out(&mut self, key: &K) -> Option<A> {
        let at = self.waiting.iter().position(|(queued, _)| queued == key)?;
        self.waiting.remove(at).map(|(_, ask)| ask)
    }

    fn next(&mut self) -> Option<(K, A)> {
        while let Some((key, ask)) = self.waiting.pop_front() {
            if self.spent.insert(key.clone()) {
                return Some((key, ask));
            }
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifted {
    Queued,
    Due,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    On,
    Stop,
}

pub trait Walker {
    type Key: Clone + Eq + Hash;
    type Ask;
    type Seek: PartialEq;
    type Error;

    fn goes_on(&self) -> Result<Step, Self::Error>;

    fn named_by(&self, seek: Self::Seek) -> Result<Vec<Self::Key>, Self::Error>;

    fn still_due(&self, key: &Self::Key) -> Result<Option<Self::Ask>, Self::Error>;

    fn lifted(&self, _key: &Self::Key, _lifted: Lifted) {}

    fn ask(&self, key: Self::Key, ask: Self::Ask) -> Result<Step, Self::Error>;
}

pub fn walk<W: Walker>(
    walker: &W,
    queue: &mut Queue<W::Key, W::Ask>,
    seeks: &Seeks<W::Seek>,
) -> Result<Step, W::Error> {
    loop {
        if walker.goes_on()? == Step::Stop {
            return Ok(Step::Stop);
        }
        lift(walker, queue, seeks)?;
        let Some((key, ask)) = queue.next() else {
            return Ok(Step::On);
        };
        if walker.ask(key, ask)? == Step::Stop {
            return Ok(Step::Stop);
        }
    }
}

fn lift<W: Walker>(
    walker: &W,
    queue: &mut Queue<W::Key, W::Ask>,
    seeks: &Seeks<W::Seek>,
) -> Result<(), W::Error> {
    for seek in seeks.taken() {
        for key in walker.named_by(seek)? {
            if queue.is_spent(&key) {
                continue;
            }
            let (ask, lifted) = match queue.taken_out(&key) {
                Some(ask) => (ask, Lifted::Queued),
                None => match walker.still_due(&key)? {
                    Some(ask) => (ask, Lifted::Due),
                    None => continue,
                },
            };
            walker.lifted(&key, lifted);
            queue.waiting.push_front((key, ask));
        }
    }
    Ok(())
}

pub trait Reads<P>: Send + Sync + 'static {
    fn read(&self, picture: P);

    fn after(&self) {}
}

pub struct Readers<P> {
    wanted: Option<Sender<P>>,
    readers: Vec<JoinHandle<()>>,
    waiting: Arc<AtomicU32>,
}

struct ReaderEnded<R: ?Sized> {
    living: Arc<AtomicUsize>,
    waiting: Arc<AtomicU32>,
    reads: Arc<R>,
}

impl<R: ?Sized> Drop for ReaderEnded<R> {
    fn drop(&mut self) {
        if self.living.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.waiting.store(0, Ordering::Release);
        }
    }
}

impl<P: Send + 'static> Readers<P> {
    pub fn none() -> Self {
        Self {
            wanted: None,
            readers: Vec::new(),
            waiting: Arc::default(),
        }
    }

    pub fn start<R: Reads<P>>(named: &str, count: usize, reads: Arc<R>) -> Self {
        let (wanted, asked) = bounded::<P>(PICTURES_WAITING_AT_MOST);
        let waiting = Arc::new(AtomicU32::new(0));
        let living = Arc::new(AtomicUsize::new(count));
        let readers: Vec<JoinHandle<()>> = (0..count)
            .filter_map(|nth| {
                let asked = asked.clone();
                let ended = ReaderEnded {
                    living: Arc::clone(&living),
                    waiting: Arc::clone(&waiting),
                    reads: Arc::clone(&reads),
                };
                thread::Builder::new()
                    .name(format!("{named}-{nth}"))
                    .spawn(move || {
                        for picture in asked {
                            ended.reads.read(picture);
                            ended.waiting.fetch_sub(1, Ordering::AcqRel);
                            ended.reads.after();
                        }
                        let reads = Arc::clone(&ended.reads);
                        drop(ended);
                        reads.after();
                    })
                    .map_err(|error| {
                        living.fetch_sub(1, Ordering::AcqRel);
                        tracing::warn!(%error, "a picture reader did not start");
                    })
                    .ok()
            })
            .collect();

        Self {
            wanted: (!readers.is_empty()).then_some(wanted),
            readers,
            waiting,
        }
    }

    pub fn hand(&self, picture: P) -> Option<P> {
        let Some(wanted) = &self.wanted else {
            return Some(picture);
        };
        self.waiting.fetch_add(1, Ordering::AcqRel);
        wanted.send(picture).err().map(|unsent| {
            self.waiting.fetch_sub(1, Ordering::AcqRel);
            unsent.0
        })
    }

    pub fn waiting(&self) -> u32 {
        self.waiting.load(Ordering::Acquire)
    }

    pub fn rest(mut self) {
        drop(self.wanted.take());
        for reader in self.readers.drain(..) {
            if reader.join().is_err() {
                tracing::warn!("a picture reader ended in a panic");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        convert::Infallible,
        sync::atomic::{AtomicU32, Ordering},
    };

    use parking_lot::Mutex;

    use super::{Lifted, Queue, Readers, Reads, Seeks, Step, Walker, walk};

    struct Asked {
        due: Vec<u64>,
        asked: RefCell<Vec<u64>>,
        lifted: RefCell<Vec<(u64, Lifted)>>,
        seeks_while_asking: Option<(u64, u64)>,
        sought: Seeks<u64>,
    }

    impl Asked {
        fn of(due: &[u64]) -> Self {
            Self {
                due: due.to_vec(),
                asked: RefCell::default(),
                lifted: RefCell::default(),
                seeks_while_asking: None,
                sought: Seeks::default(),
            }
        }
    }

    impl Walker for Asked {
        type Key = u64;
        type Ask = ();
        type Seek = u64;
        type Error = Infallible;

        fn goes_on(&self) -> Result<Step, Infallible> {
            Ok(Step::On)
        }

        fn named_by(&self, seek: u64) -> Result<Vec<u64>, Infallible> {
            Ok(vec![seek])
        }

        fn still_due(&self, key: &u64) -> Result<Option<()>, Infallible> {
            Ok(self.due.contains(key).then_some(()))
        }

        fn lifted(&self, key: &u64, lifted: Lifted) {
            self.lifted.borrow_mut().push((*key, lifted));
        }

        fn ask(&self, key: u64, (): ()) -> Result<Step, Infallible> {
            self.asked.borrow_mut().push(key);
            if let Some((after, sought)) = self.seeks_while_asking
                && after == key
            {
                self.sought.seek(sought);
            }
            Ok(Step::On)
        }
    }

    fn queued(keys: &[u64]) -> Queue<u64, ()> {
        Queue::of(keys.iter().map(|key| (*key, ())))
    }

    #[test]
    fn what_was_sought_is_lifted_to_the_front_and_the_newest_leads() {
        let walker = Asked::of(&[]);
        walker.sought.seek(3);
        walker.sought.seek(8);

        let mut queue = queued(&[1, 2, 3, 7, 8]);
        let walked = walk(&walker, &mut queue, &walker.sought);

        assert_eq!(walked, Ok(Step::On));
        assert_eq!(*walker.asked.borrow(), [8, 3, 1, 2, 7]);
        assert_eq!(
            *walker.lifted.borrow(),
            [(3, Lifted::Queued), (8, Lifted::Queued)]
        );
    }

    #[test]
    fn a_seek_naming_nothing_queued_is_brought_in_only_while_it_is_due() {
        let walker = Asked::of(&[50]);
        walker.sought.seek(99);
        walker.sought.seek(50);

        let mut queue = queued(&[1, 2]);
        walk(&walker, &mut queue, &walker.sought).expect("a walk that cannot fail");

        assert_eq!(*walker.asked.borrow(), [50, 1, 2]);
        assert_eq!(*walker.lifted.borrow(), [(50, Lifted::Due)]);
    }

    #[test]
    fn a_seek_asked_for_twice_is_held_once() {
        let walker = Asked::of(&[]);
        walker.sought.seek(3);
        walker.sought.seek(2);
        walker.sought.seek(3);

        let mut queue = queued(&[1, 2, 3, 7, 8]);
        walk(&walker, &mut queue, &walker.sought).expect("a walk that cannot fail");

        assert_eq!(*walker.asked.borrow(), [3, 2, 1, 7, 8]);
    }

    #[test]
    fn what_has_been_asked_already_is_left_where_it_is_and_never_asked_twice() {
        let walker = Asked {
            seeks_while_asking: Some((2, 1)),
            ..Asked::of(&[1])
        };

        let mut queue = queued(&[1, 2, 3, 2]);
        walk(&walker, &mut queue, &walker.sought).expect("a walk that cannot fail");

        assert_eq!(*walker.asked.borrow(), [1, 2, 3]);
        assert!(walker.lifted.borrow().is_empty());
        assert!(queue.is_spent(&1));
    }

    struct Counted {
        read: AtomicU32,
        heard: Mutex<Vec<u32>>,
    }

    impl Reads<u32> for Counted {
        fn read(&self, picture: u32) {
            self.heard.lock().push(picture);
            self.read.fetch_add(1, Ordering::AcqRel);
        }
    }

    #[test]
    fn every_picture_handed_over_is_read_once_and_none_waits_once_the_readers_rest() {
        let counted = std::sync::Arc::new(Counted {
            read: AtomicU32::new(0),
            heard: Mutex::new(Vec::new()),
        });
        let readers = Readers::start("resonate-test-pictures", 3, std::sync::Arc::clone(&counted));

        for picture in 0..200 {
            assert_eq!(readers.hand(picture), None);
        }
        let waiting = std::sync::Arc::clone(&readers.waiting);
        readers.rest();

        let mut heard = counted.heard.lock().clone();
        heard.sort_unstable();

        assert_eq!(counted.read.load(Ordering::Acquire), 200);
        assert_eq!(heard, (0..200).collect::<Vec<_>>());
        assert_eq!(waiting.load(Ordering::Acquire), 0);
    }

    #[test]
    fn with_no_reader_a_picture_is_handed_back_to_be_read_where_it_was_asked() {
        let readers = Readers::<u32>::none();

        assert_eq!(readers.hand(7), Some(7));
        assert_eq!(readers.waiting(), 0);
    }
}
