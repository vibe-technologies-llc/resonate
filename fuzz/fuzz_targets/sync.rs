#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_sync::wire::{
    Beacon, Candidates, FromPeer, FromServer, SentBeacon, ToPeer, ToServer, decoded_bundle,
    decoded_recordings, encoded_bundle, encoded_recordings,
};

fuzz_target!(|bytes: &[u8]| {
    if let Ok(frame) = ToServer::decoded(bytes) {
        assert_eq!(ToServer::decoded(&frame.encoded()).ok(), Some(frame));
    }
    if let Ok(frame) = FromServer::decoded(bytes) {
        assert_eq!(FromServer::decoded(&frame.encoded()).ok(), Some(frame));
    }
    if let Ok(frame) = ToPeer::decoded(bytes) {
        assert_eq!(ToPeer::decoded(&frame.encoded()).ok(), Some(frame));
    }
    if let Ok(frame) = FromPeer::decoded(bytes) {
        assert_eq!(FromPeer::decoded(&frame.encoded()).ok(), Some(frame));
    }
    if let Ok(beacon) = Beacon::decoded(bytes) {
        assert_eq!(Beacon::decoded(&beacon.encoded()).ok(), Some(beacon));
    }
    if let Ok(sent) = SentBeacon::decoded(bytes) {
        assert_eq!(SentBeacon::decoded(&sent.encoded()).ok(), Some(sent));
    }
    if let Ok(changes) = decoded_bundle(bytes) {
        assert_eq!(
            decoded_bundle(&encoded_bundle(&changes)).ok(),
            Some(changes)
        );
    }
    if let Ok(candidates) = Candidates::decoded(bytes) {
        assert_eq!(
            Candidates::decoded(&candidates.encoded()).ok(),
            Some(candidates)
        );
    }
    if let Ok(rows) = decoded_recordings(bytes) {
        assert_eq!(
            decoded_recordings(&encoded_recordings(&rows)).ok(),
            Some(rows)
        );
    }
});
