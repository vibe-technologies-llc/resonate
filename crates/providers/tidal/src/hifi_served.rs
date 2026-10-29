use resonate_core::SourceId;
use resonate_fetch::{Patience, Seeking, escaped, is_a_document, unreached};
use resonate_providers::{
    Album, AlbumSongs, Artist, ArtistAlbums, Error, Found, Id, Named, PlaysInPlace, ProviderOp,
    Result, Song, Streamed, not_offered,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use ureq::http::header::AUTHORIZATION;

use crate::{
    catalog::{
        AsksV1, BROWSED_AT_MOST, Entry, Items, V1Album, V1Found, V1Track, album_of,
        songs_of_entries, track_id,
    },
    hifi::{HifiApi, Hosting, Keyed, TokenService, Wrapped},
    played::TrackId,
};

const RELAYED_AS: &str = "audio/flac";
const WHOLE: u16 = 200;
const HOSTED_COUNTRY: &str = "US";
const HIFI_API_TITLE: &str = "hifi-api";
const RESONATE_TITLE: &str = "Resonate server";

struct Hosted<'a> {
    hifi: &'a HifiApi,
    hosting: &'a Hosting,
}

impl AsksV1 for Hosted<'_> {
    fn provider(&self) -> &SourceId {
        &self.hifi.asker.source
    }

    fn v1(&self, op: ProviderOp, path: &str) -> Result<Option<Vec<u8>>> {
        let bearer = self
            .hifi
            .token(op, &self.hosting.tidal_token, TokenService::Tidal)?;
        let joined = if path.contains('?') { '&' } else { '?' };
        let url = format!(
            "{}{path}{joined}countryCode={HOSTED_COUNTRY}",
            self.hosting.tidal_api
        );
        let asker = &self.hifi.asker;
        match asker.sent(op, || {
            asker
                .agent
                .get(&url)
                .header(AUTHORIZATION, bearer.as_str())
                .call()
        })? {
            crate::asker::Sent::Answered(response) => asker.read_whole(op, response).map(Some),
            crate::asker::Sent::Refused {
                status: 403 | 404, ..
            } => Ok(None),
            crate::asker::Sent::Refused { status, .. } => Err(self.hifi.refusal(op, status)),
        }
    }

    fn read<T: DeserializeOwned>(&self, op: ProviderOp, bytes: &[u8]) -> Result<T> {
        self.hifi.asker.parsed(op, bytes)
    }
}

#[derive(Default, Deserialize)]
struct Searched {
    #[serde(default)]
    data: Value,
}

fn part<T: DeserializeOwned + Default>(value: Option<&Value>) -> T {
    value
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

impl HifiApi {
    const fn is_keyed(&self) -> bool {
        self.secret.is_some()
    }

    fn hosted_v1(&self) -> Option<Hosted<'_>> {
        self.hosted.as_ref().map(|hosting| Hosted {
            hifi: self,
            hosting,
        })
    }

    fn not_browsed(&self, op: ProviderOp) -> Error {
        not_offered(&self.asker.source, op)
    }

    fn routed(&self, route: &str) -> Result<Option<Value>> {
        let op = ProviderOp::Browse;
        let url = format!("{}{route}", self.server);
        let asked = self.asked(op, &url)?;
        let Some(bytes) = self.waited_out(op, asked)? else {
            return Ok(None);
        };
        self.asker.parsed(op, &bytes).map(Some)
    }

    fn searched_on_the_server(&self, words: &str) -> Result<Found> {
        let words = escaped(words);
        let songs: Option<Wrapped<Items<V1Track>>> = self
            .routed(&format!("/search/?s={words}&limit={BROWSED_AT_MOST}"))?
            .and_then(|value| serde_json::from_value(value).ok());
        let mut found = Found {
            songs: songs
                .map(|songs| songs.data.items)
                .unwrap_or_default()
                .into_iter()
                .filter_map(V1Track::song)
                .collect(),
            ..Found::default()
        };
        if self.is_keyed() {
            return Ok(found);
        }
        for kind in ["a", "al"] {
            let answered: Searched = part(
                self.routed(&format!("/search/?{kind}={words}&limit={BROWSED_AT_MOST}"))?
                    .as_ref(),
            );
            let held: V1Found = part(Some(&answered.data));
            let held = held.found();
            found.artists.extend(held.artists);
            found.albums.extend(held.albums);
        }
        Ok(found)
    }

    fn album_on_the_server(&self, id: &Id) -> Result<AlbumSongs> {
        let album = track_id(&self.asker.source, ProviderOp::Browse, id)?;
        let answered = self.routed(&format!("/album/?id={album}"))?;
        let data = answered.as_ref().and_then(|answered| answered.get("data"));
        let entries: Items<Entry> = part(data);
        let mut held = album_of(id, songs_of_entries(entries.items));
        let named: Option<V1Album> =
            data.and_then(|data| serde_json::from_value(data.clone()).ok());
        if let Some(named) = named.and_then(V1Album::album) {
            held.album = Album {
                id: id.clone(),
                songs: held.album.songs,
                ..named
            };
        }
        Ok(held)
    }

    fn artist_on_the_server(&self, id: &Id) -> Result<ArtistAlbums> {
        let artist = track_id(&self.asker.source, ProviderOp::Browse, id)?;
        let answered = self.routed(&format!("/artist/?f={artist}"))?;
        let albums = answered.as_ref().and_then(|answered| {
            answered
                .get("albums")
                .or_else(|| answered.get("data").and_then(|data| data.get("albums")))
        });
        let albums: Items<V1Album> = part(albums);
        let albums: Vec<Album> = albums
            .items
            .into_iter()
            .filter_map(V1Album::album)
            .collect();
        Ok(ArtistAlbums {
            artist: Artist {
                id: id.clone(),
                name: albums
                    .iter()
                    .find_map(|album| album.artist.clone())
                    .unwrap_or_default(),
                albums: u32::try_from(albums.len()).ok(),
                cover: None,
            },
            albums,
        })
    }

    fn song_cover_on_the_server(&self, id: &Id) -> Result<Id> {
        let op = ProviderOp::Cover;
        let track = track_id(&self.asker.source, op, id)?;
        let answered = self.routed(&format!("/info/?id={track}"))?;
        let track: Option<V1Track> = answered
            .as_ref()
            .and_then(|answered| answered.get("data"))
            .and_then(|data| serde_json::from_value(data.clone()).ok());
        track
            .and_then(|track| track.album_cover())
            .ok_or_else(|| self.asker.unreadable(op))
    }

    fn relayed_stream(&self, keyed: &Keyed, track: TrackId) -> Result<Streamed> {
        let op = ProviderOp::Stream;
        let url = format!("{}/file/?id={}", self.server, track.0);
        let bearer = keyed.bearer();
        let response = keyed
            .relay
            .get(&url)
            .header(AUTHORIZATION, bearer.as_str())
            .call()
            .map_err(|error| unreached(self.asker.source.clone(), op, error))?;
        let status = response.status().as_u16();
        if status != WHOLE {
            return Err(self.refusal(op, status));
        }
        if is_a_document(response.body().mime_type()) {
            return Err(self.asker.unreadable(op));
        }
        let reader = Seeking::continuing(
            keyed.relay.clone(),
            url,
            response,
            Patience::default().resumed_after,
        )
        .bearing(bearer);
        let length = reader.length();
        Ok(Streamed::seeking(
            reader,
            length,
            Some(RELAYED_AS.to_owned()),
        ))
    }
}

impl PlaysInPlace for HifiApi {
    fn scheme(&self) -> &SourceId {
        &self.asker.source
    }

    fn title(&self) -> &'static str {
        if self.is_keyed() {
            RESONATE_TITLE
        } else {
            HIFI_API_TITLE
        }
    }

    fn artists(&self) -> Result<Vec<Artist>> {
        Ok(Vec::new())
    }

    fn artist(&self, id: &Id) -> Result<ArtistAlbums> {
        if let Some(hosted) = self.hosted_v1() {
            return hosted.artist_albums(id);
        }
        if self.is_keyed() {
            return Err(self.not_browsed(ProviderOp::Browse));
        }
        self.artist_on_the_server(id)
    }

    fn album(&self, id: &Id) -> Result<AlbumSongs> {
        if let Some(hosted) = self.hosted_v1() {
            return hosted.album_songs(id);
        }
        if self.is_keyed() {
            return Err(self.not_browsed(ProviderOp::Browse));
        }
        self.album_on_the_server(id)
    }

    fn search(&self, words: &str) -> Result<Found> {
        match self.hosted_v1() {
            Some(hosted) => hosted.searched(words),
            None => self.searched_on_the_server(words),
        }
    }

    fn streamed(&self, song: &Id) -> Result<Streamed> {
        let track = TrackId(track_id(&self.asker.source, ProviderOp::Stream, song)?);
        if let Some(keyed) = &self.secret {
            return self.relayed_stream(keyed, track);
        }
        let media = self.media(track)?;
        self.player().streamed(media)
    }

    fn cover(&self, cover: &Id, side: u32) -> Result<Vec<u8>> {
        self.asker.picture(&self.images, cover, side)
    }

    fn cover_of(&self, named: &Named) -> Result<Id> {
        if let Some(hosted) = self.hosted_v1() {
            return hosted.cover_named(named);
        }
        match named {
            Named::Song(id) if !self.is_keyed() => self.song_cover_on_the_server(id),
            Named::Album(id) if !self.is_keyed() => self
                .album_on_the_server(id)?
                .album
                .cover
                .ok_or_else(|| self.asker.unreadable(ProviderOp::Cover)),
            Named::Song(_) | Named::Album(_) | Named::Artist(_) => {
                Err(self.not_browsed(ProviderOp::Cover))
            }
        }
    }

    fn songs_of(&self, named: &Named) -> Result<Vec<Song>> {
        match named {
            Named::Song(id) => Ok(vec![Song::only(id.clone())]),
            Named::Album(id) => Ok(self.album(id)?.songs),
            Named::Artist(id) => {
                let mut songs = Vec::new();
                for album in self.artist(id)?.albums {
                    songs.extend(self.album(&album.id)?.songs);
                }
                Ok(songs)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_album_the_server_answers_is_read_from_its_entries_and_its_own_name() {
        let answered: Value = serde_json::from_str(
            r#"{"version":"2.2","data":{"id":7,"title":"Meddle","cover":"aa-bb","releaseDate":"1971-10-30",
                "items":[{"item":{"id":11,"title":"One of These Days","trackNumber":1,"duration":357},"type":"track"},
                         {"item":null,"type":"video"},
                         {"item":{"id":12,"title":"A Pillow of Winds","trackNumber":2},"type":"track"}]}}"#,
        )
        .expect("an album");
        let data = answered.get("data");

        let entries: Items<Entry> = part(data);
        let songs = songs_of_entries(entries.items);
        let named: Option<V1Album> =
            data.and_then(|data| serde_json::from_value(data.clone()).ok());

        assert_eq!(songs.len(), 2);
        assert_eq!(
            named.and_then(V1Album::album).map(|album| album.year),
            Some(Some(1971))
        );
    }
}
