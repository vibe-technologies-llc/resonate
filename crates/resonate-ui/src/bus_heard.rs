use std::thread;

use crossbeam_channel::{Receiver, RecvError, Select};
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use resonate_core::DesktopLook;

use crate::settings::Followed;

const HEARD_BY: &str = "resonate-bus-heard";

#[derive(Debug, PartialEq)]
pub(crate) enum Told {
    Quit,
    Raise,
    Followed(Followed),
    Desktop(DesktopLook),
}

pub(crate) struct Heard {
    pub(crate) quit: Receiver<()>,
    pub(crate) raise: Receiver<()>,
    pub(crate) followed: Receiver<Followed>,
    pub(crate) desktop: Receiver<DesktopLook>,
}

impl Heard {
    pub(crate) fn on_a_thread_of_its_own(self) -> Option<UnboundedReceiver<Told>> {
        let (told, heard) = unbounded();

        thread::Builder::new()
            .name(HEARD_BY.to_owned())
            .spawn(move || self.pass_on(&told))
            .inspect_err(|error| {
                tracing::warn!(%error, "the window cannot hear the session bus");
            })
            .ok()?;

        Some(heard)
    }

    fn pass_on(self, told: &UnboundedSender<Told>) {
        let mut waiting = Select::new();

        let quit = waiting.recv(&self.quit);
        let raise = waiting.recv(&self.raise);
        let followed = waiting.recv(&self.followed);
        let desktop = waiting.recv(&self.desktop);
        let mut open = 4;

        while open > 0 {
            let ready = waiting.select();
            let at = ready.index();

            let heard: Result<Told, RecvError> = match at {
                _ if at == quit => ready.recv(&self.quit).map(|()| Told::Quit),
                _ if at == raise => ready.recv(&self.raise).map(|()| Told::Raise),
                _ if at == followed => ready.recv(&self.followed).map(Told::Followed),
                _ => {
                    debug_assert_eq!(at, desktop);
                    ready.recv(&self.desktop).map(Told::Desktop)
                }
            };
            match heard {
                Ok(heard) => {
                    if told.unbounded_send(heard).is_err() {
                        return;
                    }
                }
                Err(RecvError) => {
                    waiting.remove(at);
                    open -= 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        thread,
        time::{Duration, Instant},
    };

    use crossbeam_channel::unbounded;
    use futures_channel::mpsc::UnboundedReceiver;
    use resonate_core::DesktopLook;

    use super::{Heard, Told};
    use crate::settings::Followed;

    const WAITED_AT_MOST: Duration = Duration::from_secs(5);

    fn next(heard: &mut UnboundedReceiver<Told>) -> Option<Told> {
        let began = Instant::now();
        loop {
            match heard.try_recv() {
                Ok(told) => return Some(told),
                Err(error) if error.is_closed() => return None,
                Err(_) => {
                    assert!(began.elapsed() < WAITED_AT_MOST, "nothing was passed on");

                    thread::yield_now();
                }
            }
        }
    }

    #[test]
    fn what_each_channel_says_is_passed_on_and_the_hearing_ends_once_every_one_is_closed() {
        let (quit, quit_heard) = unbounded();
        let (raise, raise_heard) = unbounded();
        let (followed, followed_heard) = unbounded();
        let (desktop, desktop_heard) = unbounded();

        let mut heard = Heard {
            quit: quit_heard,
            raise: raise_heard,
            followed: followed_heard,
            desktop: desktop_heard,
        }
        .on_a_thread_of_its_own()
        .expect("a thread to hear on");

        raise.send(()).expect("raised");

        let raised = next(&mut heard);

        desktop.send(DesktopLook::default()).expect("a look");

        let looked = next(&mut heard);

        followed
            .send(Followed::PartyOffered("resonate://party/ABCDE".to_owned()))
            .expect("followed");

        let follows = next(&mut heard);

        drop((raise, desktop, followed));
        quit.send(()).expect("quit");

        let quits = next(&mut heard);

        drop(quit);

        let ended = next(&mut heard);

        assert_eq!(raised, Some(Told::Raise));
        assert_eq!(looked, Some(Told::Desktop(DesktopLook::default())));
        assert_eq!(
            follows,
            Some(Told::Followed(Followed::PartyOffered(
                "resonate://party/ABCDE".to_owned()
            )))
        );
        assert_eq!(quits, Some(Told::Quit));
        assert_eq!(ended, None);
    }
}
