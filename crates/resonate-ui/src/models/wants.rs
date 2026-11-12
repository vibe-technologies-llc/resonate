use std::{sync::Arc, time::SystemTime};

use gpui::{App, Context};
use resonate_core::{ArtistId, ReleaseTrackId, WantId};
use resonate_library::{Found, Issued, Mbid, Performer, RecordingRelease};

use super::{Change, LibraryModel, Wanted, watched::brought_back};
use crate::toast;

impl LibraryModel {
    pub fn want(&mut self, release_track: ReleaseTrackId, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        let wanting = cx.spawn(async move |this, cx| {
            let wanted = cx
                .background_executor()
                .spawn(async move { library.want_release_tracks(&[release_track]) })
                .await;

            let landed = this.update(cx, |this, cx| {
                this.wants_landed(wanted, Change::Want.doing(), Wanted::ThePlaylists, cx);
            });
            let _ = landed;
        });
        wanting.detach();
        cx.notify();
    }

    pub(super) fn wants_landed(
        &mut self,
        wanted: resonate_library::Result<Vec<(ReleaseTrackId, WantId)>>,
        doing: &str,
        reread: Wanted,
        cx: &mut Context<Self>,
    ) {
        match wanted {
            Ok(wanted) if !wanted.is_empty() => {
                self.list_as_downloads(&wanted, cx);
                self.fetch_or_say_nobody_can(cx);
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!(%error, "release tracks could not be wanted");
                toast::tell(toast::could_not(doing, &error), cx);
            }
        }
        self.read(reread, cx);
    }

    fn list_as_downloads(&mut self, wanted: &[(ReleaseTrackId, WantId)], cx: &App) {
        let fetched_by = self.fetched_by(cx);
        let now = SystemTime::now();
        let owner = self
            .detail
            .album
            .as_ref()
            .and_then(|album| album.artist.clone())
            .unwrap_or_default();
        let album_title = self
            .detail
            .album
            .as_ref()
            .map(|album| album.title.clone())
            .unwrap_or_default();
        let performer = self
            .detail
            .album
            .as_ref()
            .and_then(|album| album.artist_id)
            .map(Performer::Held);
        let pressing = self.detail.release.as_ref();

        for (release_track, want) in wanted {
            let Some(row) = self
                .detail
                .release_tracks
                .iter()
                .find(|row| row.id == *release_track)
            else {
                continue;
            };
            let Some(recording) = row.recording.clone() else {
                continue;
            };
            let release = pressing.and_then(|pressing| {
                Some(RecordingRelease {
                    id: pressing.mbid.clone()?,
                    title: album_title.clone(),
                    date: pressing.date.clone(),
                    disc: Some(row.disc),
                    position: Some(row.position),
                    issued: Issued {
                        kind: pressing.kind.clone(),
                        ..Issued::default()
                    },
                })
            });
            let found = Found {
                recording: recording.clone(),
                title: row.title.clone(),
                artist: row.artist.clone().unwrap_or_else(|| owner.clone()),
                length: row.length,
                release,
                releases: Vec::new(),
                performer: performer.clone(),
                isrcs: Vec::new(),
            };
            self.delivered.downloads.landing(found, now);
            self.delivered
                .downloads
                .wanted(&recording, *want, fetched_by);
        }
    }

    pub fn unwant(&mut self, want: WantId, cx: &mut Context<Self>) {
        self.edit(
            Change::Unwant,
            move |library| library.unwant(want).map(|_| None),
            cx,
        );
    }

    pub fn dismiss_missing(&mut self, release_track: ReleaseTrackId, cx: &mut Context<Self>) {
        self.edit(
            Change::Dismiss,
            move |library| library.dismiss_missing(release_track).map(|_| None),
            cx,
        );
    }

    pub fn dismiss_release(&mut self, artist: ArtistId, release: Mbid, cx: &mut Context<Self>) {
        self.edit(
            Change::Dismiss,
            move |library| library.dismiss_release(artist, &release).map(|_| None),
            cx,
        );
    }

    pub fn bring_back_dismissed(&mut self, cx: &mut Context<Self>) {
        self.edit(
            Change::BringBack,
            move |library| {
                library
                    .bring_back_dismissed()
                    .map(|brought| Some(brought_back(brought)))
            },
            cx,
        );
    }
}
