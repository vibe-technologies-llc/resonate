use resonate_core::SourceId;
use resonate_fetch::is_a_document;
use resonate_providers::{
    AlbumSongs, Artist, ArtistAlbums, Error, Found, Id, Mix, MixSongs, Named, PlaysInPlace,
    ProviderOp, Rating, Result, Song, Streamed, not_offered,
};
use ureq::http::Uri;

use crate::{
    YtMusic,
    playlists::{LIKED_SONGS, LIKED_SONGS_ID},
    search::Row,
};

const THE_TITLE: &str = "YouTube Music";
const PICTURES_ARE_ON: [&str; 3] = [
    "lh3.googleusercontent.com",
    "yt3.googleusercontent.com",
    "i.ytimg.com",
];
const LARGEST_PICTURE: u64 = 8 * 1024 * 1024;

fn is_on_youtubes_image_hosts(address: &str) -> bool {
    let Ok(uri) = address.parse::<Uri>() else {
        return false;
    };
    uri.scheme_str() == Some("https")
        && uri
            .authority()
            .is_some_and(|authority| PICTURES_ARE_ON.contains(&authority.as_str()))
}

fn song_of(row: Row) -> Option<Song> {
    let id = Id::new(row.video.as_str())?;
    Some(Song {
        rating: Rating::of(Rating::flagged(Some(row.explicit)), &row.title, None),
        cover: row.thumbnail.as_deref().and_then(Id::new),
        title: row.title,
        artist: row.artist,
        album: row.album,
        length: row.length,
        ..Song::only(id)
    })
}

impl YtMusic {
    fn picture(&self, address: &str) -> Result<Vec<u8>> {
        let op = ProviderOp::Cover;
        let response = self
            .asking
            .get(address)
            .config()
            .max_redirects(0)
            .build()
            .call()
            .map_err(|error| self.unreachable(op, error))?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Refused {
                provider: self.source.clone(),
                op,
                status: status.as_u16(),
            });
        }
        if is_a_document(response.body().mime_type()) {
            return Err(Error::Unreadable {
                provider: self.source.clone(),
                op,
            });
        }
        response
            .into_body()
            .with_config()
            .limit(LARGEST_PICTURE)
            .read_to_vec()
            .map_err(|error| self.unreachable(op, error))
    }
}

impl PlaysInPlace for YtMusic {
    fn scheme(&self) -> &SourceId {
        &self.source
    }

    fn title(&self) -> &'static str {
        THE_TITLE
    }

    fn artists(&self) -> Result<Vec<Artist>> {
        Ok(Vec::new())
    }

    fn artist(&self, _id: &Id) -> Result<ArtistAlbums> {
        Err(not_offered(&self.source, ProviderOp::Browse))
    }

    fn album(&self, _id: &Id) -> Result<AlbumSongs> {
        Err(not_offered(&self.source, ProviderOp::Browse))
    }

    fn search(&self, words: &str) -> Result<Found> {
        let songs = self.songs(words)?.into_iter().filter_map(song_of).collect();
        Ok(Found {
            songs,
            ..Found::default()
        }
        .explicit_first())
    }

    fn first_page(&self) -> Result<Found> {
        if !self.tool.holds_cookies() {
            return Ok(Found::default());
        }
        let mut mixes = Vec::from_iter(liked_songs());
        mixes.extend(self.tool.playlists()?);
        Ok(Found {
            mixes,
            ..Found::default()
        })
    }

    fn mix(&self, id: &Id) -> Result<MixSongs> {
        let listed = self.tool.playlist(id)?;
        let name = if id.as_str() == LIKED_SONGS_ID {
            LIKED_SONGS.to_owned()
        } else {
            listed.name.unwrap_or_else(|| id.to_string())
        };
        Ok(MixSongs {
            songs: listed.songs,
            mix: Mix {
                id: id.clone(),
                name,
                subtitle: None,
            },
        })
    }

    fn streamed(&self, _song: &Id) -> Result<Streamed> {
        Err(not_offered(&self.source, ProviderOp::Stream))
    }

    fn plays_songs(&self) -> bool {
        false
    }

    fn keeps_songs(&self) -> bool {
        true
    }

    fn cover(&self, cover: &Id, _side: u32) -> Result<Vec<u8>> {
        if !is_on_youtubes_image_hosts(cover.as_str()) {
            tracing::warn!("a thumbnail was outside YouTube's image hosts");
            return Err(Error::OffItsHosts {
                provider: self.source.clone(),
                op: ProviderOp::Cover,
            });
        }
        self.picture(cover.as_str())
    }

    fn cover_of(&self, _named: &Named) -> Result<Id> {
        Err(not_offered(&self.source, ProviderOp::Cover))
    }
}

fn liked_songs() -> Option<Mix> {
    Some(Mix {
        id: Id::new(LIKED_SONGS_ID)?,
        name: LIKED_SONGS.to_owned(),
        subtitle: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_on_an_image_host_with_nothing_beside_the_host_is_a_picture() {
        for fetched in [
            "https://lh3.googleusercontent.com/a=w120-h120",
            "https://yt3.googleusercontent.com/a=w120-h120",
            "https://i.ytimg.com/vi/9RfVp-GhKfs/hqdefault.jpg?sqp=a",
        ] {
            assert!(is_on_youtubes_image_hosts(fetched), "{fetched}");
        }
        for refused in [
            "http://i.ytimg.com/vi/a/hqdefault.jpg",
            "https://i.ytimg.com:8443/vi/a/hqdefault.jpg",
            "https://i.ytimg.com@elsewhere.example/a.jpg",
            "https://someone@i.ytimg.com/a.jpg",
            "https://i.ytimg.com.elsewhere.example/a.jpg",
            "https://elsewhere.example/i.ytimg.com/a.jpg",
            "file:///etc/passwd",
            "not an address",
        ] {
            assert!(!is_on_youtubes_image_hosts(refused), "{refused}");
        }
    }
}
