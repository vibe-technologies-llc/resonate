use std::{
    fs::File,
    future::{Future as _, poll_fn},
    io::Write,
    pin::{Pin, pin},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    task::Poll,
    thread,
    time::Duration,
};

use futures_channel::oneshot;
use gpui::{App, ClipboardItem, Task};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, delegate_noop, event_created_child,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_registry::WlRegistry, wl_seat::WlSeat},
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::ExtDataControlOfferV1,
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};

const TEXT_TYPES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "STRING",
    "TEXT",
];

const ANSWERED_WITHIN: Duration = Duration::from_millis(250);

const THE_FIRST_VERSION: u32 = 1;

const WAITING: u8 = 0;

const TAKEN_BY_THE_COMPOSITOR: u8 = 1;

const GIVEN_UP: u8 = 2;

#[derive(Debug, Default)]
struct Claim(AtomicU8);

impl Claim {
    fn for_the_compositor(&self) -> bool {
        self.settle(TAKEN_BY_THE_COMPOSITOR)
    }

    fn given_up(&self) -> bool {
        self.settle(GIVEN_UP)
    }

    fn settle(&self, on: u8) -> bool {
        self.0
            .compare_exchange(WAITING, on, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

type Told = oneshot::Sender<bool>;

enum Answer {
    InTime(bool),
    Late(oneshot::Receiver<bool>),
}

pub(crate) fn copy(text: String, cx: &mut App) {
    copy_through(text, serve, cx);
}

fn copy_through(
    text: String,
    offer: impl FnOnce(Vec<u8>, &Claim, Told) + Send + 'static,
    cx: &mut App,
) {
    let claim = Arc::new(Claim::default());
    let (told, heard) = oneshot::channel();
    let served = text.as_bytes().to_vec();
    let serving = Arc::clone(&claim);
    let spawned = thread::Builder::new()
        .name("resonate-clipboard".to_owned())
        .spawn(move || offer(served, &serving, told));
    if spawned.is_err() {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        return;
    }

    let deadline = cx.background_executor().timer(ANSWERED_WITHIN);
    cx.spawn(async move |cx| {
        let taken = match answered_by(heard, deadline).await {
            Answer::InTime(taken) => taken,
            Answer::Late(_) if claim.given_up() => false,
            Answer::Late(heard) => heard.await.unwrap_or(false),
        };
        if !taken {
            let written = cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(text)));
            let _ = written;
        }
    })
    .detach();
}

async fn answered_by(mut heard: oneshot::Receiver<bool>, deadline: Task<()>) -> Answer {
    let mut deadline = pin!(deadline);
    let in_time = poll_fn(|waker| {
        if let Poll::Ready(answer) = Pin::new(&mut heard).poll(waker) {
            return Poll::Ready(Some(answer.unwrap_or(false)));
        }
        deadline.as_mut().poll(waker).map(|()| None)
    })
    .await;

    match in_time {
        Some(taken) => Answer::InTime(taken),
        None => Answer::Late(heard),
    }
}

struct Offering {
    text: Vec<u8>,
    taken: bool,
}

struct Offered {
    queue: EventQueue<Offering>,
    source: ExtDataControlSourceV1,
    device: ExtDataControlDeviceV1,
}

fn serve(text: Vec<u8>, claim: &Claim, told: Told) {
    let mut offering = Offering { text, taken: false };
    let Some(mut offered) = offer(&mut offering, claim) else {
        let _ = told.send(false);
        return;
    };
    let _ = told.send(true);

    while !offering.taken {
        if let Err(error) = offered.queue.blocking_dispatch(&mut offering) {
            tracing::debug!(%error, "the compositor stopped asking for the copy");
            break;
        }
    }
    offered.source.destroy();
    offered.device.destroy();
    let _ = offered.queue.flush();
}

fn offer(offering: &mut Offering, claim: &Claim) -> Option<Offered> {
    let connection = Connection::connect_to_env()
        .map_err(|error| tracing::debug!(%error, "no compositor to hand a copy to"))
        .ok()?;
    let (globals, mut queue) = registry_queue_init::<Offering>(&connection)
        .map_err(|error| tracing::debug!(%error, "the compositor's globals could not be read"))
        .ok()?;
    let handle = queue.handle();

    let manager: ExtDataControlManagerV1 = globals
        .bind(&handle, THE_FIRST_VERSION..=THE_FIRST_VERSION, ())
        .map_err(|error| tracing::debug!(%error, "the compositor offers no data control"))
        .ok()?;
    let seat: WlSeat = globals
        .bind(&handle, THE_FIRST_VERSION..=THE_FIRST_VERSION, ())
        .map_err(|error| tracing::debug!(%error, "the compositor offers no seat"))
        .ok()?;

    let device = manager.get_data_device(&seat, &handle, ());
    let source = manager.create_data_source(&handle, ());
    for kind in TEXT_TYPES {
        source.offer(kind.to_owned());
    }
    if !claim.for_the_compositor() {
        tracing::debug!("the window gave up on the compositor before it could be handed the copy");
        source.destroy();
        device.destroy();
        let _ = queue.flush();
        return None;
    }
    device.set_selection(Some(&source));

    queue
        .roundtrip(offering)
        .map_err(|error| tracing::debug!(%error, "the compositor did not take the copy"))
        .ok()?;

    Some(Offered {
        queue,
        source,
        device,
    })
}

impl Dispatch<WlRegistry, GlobalListContents> for Offering {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtDataControlSourceV1, ()> for Offering {
    fn event(
        state: &mut Self,
        _: &ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_data_control_source_v1::Event::Send { fd, .. } => {
                if let Err(error) = File::from(fd).write_all(&state.text) {
                    tracing::debug!(%error, "a reader went before the copy was written to it");
                }
            }
            ext_data_control_source_v1::Event::Cancelled => state.taken = true,
            _ => {}
        }
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for Offering {
    fn event(
        state: &mut Self,
        _: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_data_control_device_v1::Event::Finished = event {
            state.taken = true;
        }
    }

    event_created_child!(Offering, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

delegate_noop!(Offering: ignore WlSeat);
delegate_noop!(Offering: ExtDataControlManagerV1);
delegate_noop!(Offering: ignore ExtDataControlOfferV1);

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use gpui::TestAppContext;

    use super::*;

    fn held(cx: &TestAppContext) -> Option<String> {
        cx.read_from_clipboard().and_then(|item| item.text())
    }

    #[gpui::test]
    fn a_copy_hands_the_thread_its_text_and_waits_on_nothing(cx: &mut TestAppContext) {
        let (release, released) = mpsc::channel::<()>();
        let (heard, hearing) = mpsc::channel();

        cx.update(|cx| {
            copy_through(
                "Echoes".to_owned(),
                move |text, _: &Claim, _: Told| {
                    let _ = released.recv();
                    let _ = heard.send(text);
                },
                cx,
            );
        });
        let _ = release.send(());

        assert_eq!(hearing.recv().as_deref(), Ok(&b"Echoes"[..]));
    }

    #[gpui::test]
    fn a_compositor_answering_late_is_refused_and_the_fallback_keeps_the_clipboard(
        cx: &mut TestAppContext,
    ) {
        let (release, released) = mpsc::channel::<()>();
        let (claimed, claiming) = mpsc::channel();

        cx.update(|cx| {
            copy_through(
                "Time".to_owned(),
                move |_, claim: &Claim, told: Told| {
                    let _ = released.recv();
                    let taken = claim.for_the_compositor();
                    let _ = told.send(taken);
                    let _ = claimed.send(taken);
                },
                cx,
            );
        });
        cx.run_until_parked();
        assert_eq!(held(cx), None);

        cx.executor().advance_clock(ANSWERED_WITHIN);
        cx.run_until_parked();
        assert_eq!(held(cx).as_deref(), Some("Time"));

        let _ = release.send(());
        assert_eq!(claiming.recv(), Ok(false));
    }

    #[gpui::test]
    fn a_compositor_taking_the_copy_in_time_leaves_the_fallback_unwritten(cx: &mut TestAppContext) {
        let (answered, answering) = mpsc::channel();

        cx.write_to_clipboard(ClipboardItem::new_string("Money".to_owned()));
        cx.update(|cx| {
            copy_through(
                "Us and Them".to_owned(),
                move |_, claim: &Claim, told: Told| {
                    let taken = claim.for_the_compositor();
                    let _ = told.send(taken);
                    let _ = answered.send(());
                },
                cx,
            );
        });
        let _ = answering.recv();
        cx.run_until_parked();
        cx.executor().advance_clock(ANSWERED_WITHIN);
        cx.run_until_parked();

        assert_eq!(held(cx).as_deref(), Some("Money"));
    }

    #[gpui::test]
    fn a_compositor_refusing_the_copy_is_answered_by_the_fallback_at_once(cx: &mut TestAppContext) {
        let (answered, answering) = mpsc::channel();

        cx.update(|cx| {
            copy_through(
                "Brain Damage".to_owned(),
                move |_, _: &Claim, told: Told| {
                    let _ = told.send(false);
                    let _ = answered.send(());
                },
                cx,
            );
        });
        let _ = answering.recv();
        cx.run_until_parked();

        assert_eq!(held(cx).as_deref(), Some("Brain Damage"));
    }

    #[test]
    fn a_claim_settles_once_whichever_side_reaches_it_first() {
        let given_up_first = Claim::default();
        let claimed_first = Claim::default();

        assert!(given_up_first.given_up());
        assert!(!given_up_first.for_the_compositor());
        assert!(claimed_first.for_the_compositor());
        assert!(!claimed_first.given_up());
    }
}
