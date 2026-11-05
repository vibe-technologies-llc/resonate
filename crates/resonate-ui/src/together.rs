use std::time::Duration;

use resonate_core::{PlaylistId, ZoneId};

const SHARED_LINK: &str = "resonate://shared/";

const PARTY_LINK: &str = "resonate://party/";

const SHARED_PAGE: &str = "/s/#";

const PARTY_PAGE: &str = "/p/#";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedPerson {
    pub id: [u8; 16],
    pub name: String,
    pub me: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedReach {
    Waiting,
    Reached,
    Unreached,
    ServerTooOld,
    TurnedAway,
    NoServer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedSeen {
    pub name: String,
    pub may_edit: bool,
    pub edit_link: Option<String>,
    pub view_link: String,
    pub people: Vec<SharedPerson>,
    pub unheld: Vec<String>,
    pub reach: SharedReach,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartyReached {
    Connecting,
    Here,
    Reconnecting,
    Ended,
    NoSuchParty,
    TurnedAway,
    ServerTooOld,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartyStanding {
    WaitingForTheHost,
    Finding,
    NotHeld,
    CannotStartMidSong,
    SatOut,
    HostPaused,
    InStep,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartySeen {
    pub zone: ZoneId,
    pub hosting: bool,
    pub link: String,
    pub reach: PartyReached,
    pub people: Vec<SharedPerson>,
    pub standing: PartyStanding,
    pub song: Option<(String, String)>,
    pub asked_for: Vec<(String, String)>,
    pub round_trip: Option<Duration>,
}

pub trait Together: Send + Sync {
    fn seen(&self, playlist: PlaylistId) -> Option<SharedSeen>;

    fn shared_playlists(&self) -> Vec<(PlaylistId, bool)>;

    fn share(&self, playlist: PlaylistId) -> bool;

    fn watch(&self, playlist: Option<PlaylistId>);

    fn leave(&self, playlist: PlaylistId) -> bool;

    fn reset(&self, playlist: PlaylistId, removing: Option<[u8; 16]>) -> bool;

    fn accept(&self, invitation: &str) -> bool;

    fn join_party(&self, invitation: &str, zone: ZoneId) -> bool;

    fn named(&self, name: &str);

    fn party(&self) -> Option<PartySeen>;

    fn begin_party(&self, zone: ZoneId) -> bool;

    fn leave_party(&self);

    fn turn_out(&self, person: [u8; 16]);

    fn play_request_next(&self, at: usize) -> bool;

    fn dismiss_request(&self, at: usize);

    fn following(&self, zone: ZoneId) -> bool;
}

pub(crate) fn is_an_invitation(text: &str) -> bool {
    let text = text.trim();
    let lowered = text.to_ascii_lowercase();

    lowered.starts_with(SHARED_LINK)
        || lowered.starts_with(PARTY_LINK)
        || (lowered.starts_with("http")
            && (lowered.contains(SHARED_PAGE) || lowered.contains(PARTY_PAGE)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_invitation_is_told_from_a_search_and_from_another_services_link() {
        assert!(is_an_invitation("resonate://shared/E-ABCDE"));
        assert!(is_an_invitation(" RESONATE://PARTY/ABCDE "));
        assert!(is_an_invitation("https://resonate.tecnio.dev/s/#E-ABCDE"));
        assert!(is_an_invitation("https://resonate.tecnio.dev/p/#ABCDE"));

        assert!(!is_an_invitation("resonate://pair/ABCDE"));
        assert!(!is_an_invitation("https://tidal.com/browse/playlist/abc"));
        assert!(!is_an_invitation("love song"));
    }
}
