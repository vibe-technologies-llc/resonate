use std::sync::Arc;

use crossbeam_channel::Sender;
use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, Error, Reshaped, Reshaper, Result, StreamCommand, StreamRequest, Unreshapable,
};
use resonate_core::StreamSpec;

use super::{handover::Handing, oboe_offers::Way, oboe_state::Routing};

pub(crate) enum Order {
    Command(StreamCommand),
    Reshaped,
}

#[derive(Clone, Copy)]
pub(crate) struct Opened {
    pub(crate) spec: StreamSpec,
    pub(crate) way: Way,
    pub(crate) requested: i32,
}

impl Opened {
    fn refuses(&self, request: &StreamRequest, routing: &Routing) -> Option<Unreshapable> {
        if request.spec != self.spec {
            Some(Unreshapable::AnotherFormat)
        } else if routing.requested() != self.requested {
            Some(Unreshapable::AnotherTarget)
        } else if routing.way_for(request.spec) != self.way {
            Some(Unreshapable::AnotherPath)
        } else {
            None
        }
    }
}

pub(crate) fn controlled_by(
    orders: Sender<Order>,
) -> Box<dyn Fn(StreamCommand) -> Result<()> + Send + Sync> {
    Box::new(move |command| {
        orders
            .send(Order::Command(command))
            .map_err(|_| Error::Disconnected)
    })
}

pub(crate) fn reshaper(
    opened: Opened,
    routing: Arc<Routing>,
    handing: Handing,
    orders: Sender<Order>,
) -> Reshaper {
    let handing = Mutex::new(handing);
    Box::new(move |request, source| {
        let mut handing = handing.lock();
        handing.let_go_of_the_spent();
        if let Some(because) = opened.refuses(request, &routing) {
            return Reshaped::Refused { source, because };
        }
        refused_or_kept(&mut handing, &orders, source)
    })
}

fn refused_or_kept(
    handing: &mut Handing,
    orders: &Sender<Order>,
    source: Box<dyn AudioSource>,
) -> Reshaped {
    if !handing.has_room() {
        return Reshaped::Refused {
            source,
            because: Unreshapable::StillHandingOver,
        };
    }
    if orders.send(Order::Reshaped).is_err() {
        return Reshaped::Refused {
            source,
            because: Unreshapable::NoStream,
        };
    }
    match handing.hand(source) {
        Ok(()) => Reshaped::Kept,
        Err(source) => Reshaped::Refused {
            source,
            because: Unreshapable::StillHandingOver,
        },
    }
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::{Receiver, unbounded};
    use resonate_audio::{LatencyRequest, MediaRole};
    use resonate_core::{ChannelLayout, SampleFormat, SampleRate};

    use super::*;
    use crate::output::{
        formats::{OutputEncoding, OutputFormat},
        handover::{Fed, fed_from},
        oboe_offers::MIXED,
    };

    const USB: i32 = 31;
    const CD: StreamSpec = StreamSpec::new(
        SampleRate::HZ_44100,
        ChannelLayout::Stereo,
        SampleFormat::S16,
    );

    struct Marked(u8);

    impl AudioSource for Marked {
        fn fill(&mut self, dst: &mut [u8]) -> usize {
            dst.fill(self.0);
            dst.len()
        }
    }

    struct Held {
        routing: Arc<Routing>,
        fed: Fed,
        orders: Receiver<Order>,
        reshaper: Reshaper,
    }

    fn held(opened: Opened) -> Held {
        let routing = Arc::new(Routing::new());
        let (fed, handing) = fed_from(Box::new(Marked(1)));
        let (sender, orders) = unbounded();
        let reshaper = reshaper(opened, Arc::clone(&routing), handing, sender);
        Held {
            routing,
            fed,
            orders,
            reshaper,
        }
    }

    fn mixed() -> Opened {
        Opened {
            spec: MIXED,
            way: Way::Mixed,
            requested: 0,
        }
    }

    fn request(spec: StreamSpec) -> StreamRequest {
        StreamRequest {
            target: None,
            spec,
            latency: LatencyRequest::Auto,
            role: MediaRole::Music,
            media_name: String::new(),
            force_graph_rate: false,
            no_convert: false,
            realtime: true,
        }
    }

    fn mark_read(fed: &mut Fed) -> u8 {
        let mut bytes = [0_u8; 2];
        fed.source().fill(&mut bytes);
        bytes.first().copied().unwrap_or_default()
    }

    fn refused_for(reshaped: Reshaped) -> Option<Unreshapable> {
        match reshaped {
            Reshaped::Refused { because, .. } => Some(because),
            Reshaped::Kept | Reshaped::Lost(_) => None,
        }
    }

    #[test]
    fn a_command_is_sent_to_the_output_thread_and_a_thread_that_has_ended_is_disconnected() {
        let (sender, orders) = unbounded();
        let control = controlled_by(sender);

        assert!(control(StreamCommand::Drain).is_ok());
        assert!(matches!(
            orders.try_recv(),
            Ok(Order::Command(StreamCommand::Drain))
        ));

        drop(orders);
        assert!(matches!(
            control(StreamCommand::Close),
            Err(Error::Disconnected)
        ));
    }

    #[test]
    fn a_stream_asked_for_the_shape_it_holds_is_given_the_new_source_and_kept() {
        let mut held = held(mixed());

        let reshaped = (held.reshaper)(&request(MIXED), Box::new(Marked(2)));

        assert!(matches!(reshaped, Reshaped::Kept));
        assert!(matches!(held.orders.try_recv(), Ok(Order::Reshaped)));
        assert_eq!(mark_read(&mut held.fed), 2);
    }

    #[test]
    fn a_stream_is_reshaped_again_once_its_callback_has_taken_the_last_source() {
        let mut held = held(mixed());

        for mark in 2..=4 {
            let reshaped = (held.reshaper)(&request(MIXED), Box::new(Marked(mark)));
            assert!(matches!(reshaped, Reshaped::Kept), "source {mark}");
            assert_eq!(mark_read(&mut held.fed), mark);
        }
    }

    #[test]
    fn a_source_not_yet_taken_keeps_the_stream_from_taking_another() {
        let mut held = held(mixed());
        assert!(matches!(
            (held.reshaper)(&request(MIXED), Box::new(Marked(2))),
            Reshaped::Kept
        ));

        let reshaped = (held.reshaper)(&request(MIXED), Box::new(Marked(3)));

        assert_eq!(refused_for(reshaped), Some(Unreshapable::StillHandingOver));
        assert_eq!(mark_read(&mut held.fed), 2);
    }

    #[test]
    fn another_rate_word_or_channel_count_is_refused_for_a_stream_opened_in_one_shape() {
        let held = held(mixed());
        let mono = StreamSpec::new(SampleRate::HZ_48000, ChannelLayout::Mono, SampleFormat::F32);

        for spec in [CD, mono] {
            let reshaped = (held.reshaper)(&request(spec), Box::new(Marked(2)));
            assert_eq!(refused_for(reshaped), Some(Unreshapable::AnotherFormat));
        }
        assert!(held.orders.try_recv().is_err());
    }

    #[test]
    fn the_source_of_a_refusal_is_handed_back_unread() {
        let held = held(mixed());

        let reshaped = (held.reshaper)(&request(CD), Box::new(Marked(9)));

        let Reshaped::Refused { mut source, .. } = reshaped else {
            panic!("a stream held in another shape was reshaped");
        };
        let mut bytes = [0_u8; 2];
        source.fill(&mut bytes);
        assert_eq!(bytes, [9, 9]);
    }

    #[test]
    fn a_device_chosen_since_the_stream_opened_is_refused_so_the_stream_opens_on_it() {
        let held = held(mixed());
        held.routing.select(USB);

        let reshaped = (held.reshaper)(&request(MIXED), Box::new(Marked(2)));

        assert_eq!(refused_for(reshaped), Some(Unreshapable::AnotherTarget));
    }

    #[test]
    fn a_route_that_now_offers_the_song_its_own_way_is_refused_so_the_stream_opens_that_way() {
        let held = held(Opened {
            spec: CD,
            way: Way::Mixed,
            requested: 0,
        });
        held.routing.offer(
            USB,
            &[OutputFormat {
                rate: 44_100,
                encoding: OutputEncoding::Pcm16,
                channels: 2,
            }],
        );
        held.routing.follow_song_rate(true);

        let reshaped = (held.reshaper)(&request(CD), Box::new(Marked(2)));

        assert_eq!(refused_for(reshaped), Some(Unreshapable::AnotherPath));
    }

    #[test]
    fn a_stream_whose_thread_has_ended_is_refused_with_the_source_handed_back() {
        let held = held(mixed());
        let Held {
            reshaper, orders, ..
        } = held;
        drop(orders);

        let reshaped = reshaper(&request(MIXED), Box::new(Marked(2)));

        assert_eq!(refused_for(reshaped), Some(Unreshapable::NoStream));
    }
}
