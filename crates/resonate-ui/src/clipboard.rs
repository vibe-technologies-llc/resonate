use std::{
    fs::File,
    io::Write,
    sync::mpsc::{self, SyncSender},
    thread,
    time::Duration,
};

use gpui::{App, ClipboardItem};
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

pub(crate) fn copy(text: String, cx: &mut App) {
    if !offered_by_the_compositor(&text) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }
}

fn offered_by_the_compositor(text: &str) -> bool {
    let (told, heard) = mpsc::sync_channel(1);
    let served = text.as_bytes().to_vec();
    let spawned = thread::Builder::new()
        .name("resonate-clipboard".to_owned())
        .spawn(move || serve(served, &told));

    spawned.is_ok() && heard.recv_timeout(ANSWERED_WITHIN) == Ok(true)
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

fn serve(text: Vec<u8>, told: &SyncSender<bool>) {
    let mut offering = Offering { text, taken: false };
    let Some(mut offered) = offer(&mut offering) else {
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

fn offer(offering: &mut Offering) -> Option<Offered> {
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
