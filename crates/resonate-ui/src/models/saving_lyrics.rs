use std::sync::Arc;

use gpui::Context;
use resonate_core::TrackId;
use resonate_library::Library;
use resonate_lyrics::{Beside, KeptSheet, Standing, write_beside};

use super::{LibraryModel, Notice};
use crate::{toast, words};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Saved {
    Written,
    AlreadySaid,
    Stands,
    Outranked,
    NotAFileOfItsOwn,
    NothingKept,
    Unwritten,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SavedLyrics {
    written: usize,
    already: usize,
    stood: usize,
    outranked: usize,
    apart: usize,
    unkept: usize,
    unwritten: usize,
}

impl LibraryModel {
    pub fn save_the_lyrics(
        &mut self,
        tracks: Vec<TrackId>,
        standing: Standing,
        cx: &mut Context<Self>,
    ) {
        let library = Arc::clone(&self.library);
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_executor()
                .spawn(async move {
                    tracks
                        .into_iter()
                        .map(|track| saved(&library, track, standing))
                        .fold(SavedLyrics::default(), SavedLyrics::and)
                })
                .await;
            let _ = this.update(cx, |_, cx| {
                toast::tell(saved.told(), cx);
                cx.notify();
            });
        })
        .detach();
    }
}

fn saved(library: &Library, id: TrackId, standing: Standing) -> Saved {
    let track = match library.track(id) {
        Ok(Some(track)) => track,
        Ok(None) => return Saved::NotAFileOfItsOwn,
        Err(error) => {
            tracing::warn!(%error, track = id.get(), "a song whose lyrics were to be saved went unread");
            return Saved::Unwritten;
        }
    };
    let Some(path) = track
        .location
        .as_path()
        .filter(|_| track.span.is_none() && track.location.is_local())
    else {
        return Saved::NotAFileOfItsOwn;
    };
    let sung = match library.kept_lyrics(&track.location, None) {
        Ok(kept) => kept.and_then(|kept| kept.sung),
        Err(error) => {
            tracing::warn!(%error, track = id.get(), "the lyrics kept for a song went unread");
            return Saved::Unwritten;
        }
    };
    let Some(sung) = sung else {
        return Saved::NothingKept;
    };
    let sheet = KeptSheet {
        text: &sung.text,
        document: sung.lyricsfile.as_deref(),
    };

    match write_beside(path, sheet, standing) {
        Ok(Beside::Written(_)) => Saved::Written,
        Ok(Beside::AlreadySaid(_)) => Saved::AlreadySaid,
        Err(resonate_lyrics::Error::SidecarStands { .. }) => Saved::Stands,
        Err(resonate_lyrics::Error::SidecarOutranked { .. }) => Saved::Outranked,
        Err(error) => {
            tracing::warn!(%error, track = id.get(), "a song's lyrics could not be saved beside it");
            Saved::Unwritten
        }
    }
}

impl SavedLyrics {
    fn and(mut self, saved: Saved) -> Self {
        let counted = match saved {
            Saved::Written => &mut self.written,
            Saved::AlreadySaid => &mut self.already,
            Saved::Stands => &mut self.stood,
            Saved::Outranked => &mut self.outranked,
            Saved::NotAFileOfItsOwn => &mut self.apart,
            Saved::NothingKept => &mut self.unkept,
            Saved::Unwritten => &mut self.unwritten,
        };
        *counted += 1;
        self
    }

    fn told(self) -> Notice {
        if self.unwritten > 0 {
            return Notice::Trouble(words::saving_lyrics::not_written(self.unwritten));
        }
        let mut parts = Vec::new();
        if self.written > 0 {
            parts.push(words::saving_lyrics::written(self.written));
        }
        if self.already > 0 {
            parts.push(words::saving_lyrics::already_beside(self.already));
        }
        if self.stood > 0 {
            parts.push(words::saving_lyrics::left_alone(self.stood));
        }
        if self.outranked > 0 {
            parts.push(words::saving_lyrics::outranked(self.outranked));
        }
        if self.unkept > 0 {
            parts.push(words::saving_lyrics::none_kept(self.unkept));
        }
        if self.apart > 0 {
            parts.push(words::saving_lyrics::no_file_of_its_own(self.apart));
        }
        let told = parts.join(" · ");

        match self.written + self.already > 0 && self.stood + self.outranked == 0 {
            true => Notice::Done(told),
            false => Notice::Noted(told),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counted(saved: &[Saved]) -> SavedLyrics {
        saved
            .iter()
            .copied()
            .fold(SavedLyrics::default(), SavedLyrics::and)
    }

    #[test]
    fn a_sheet_left_alone_is_said_beside_what_was_written_and_says_how_to_replace_it() {
        let told = counted(&[Saved::Written, Saved::Written, Saved::Stands]).told();

        assert!(matches!(told, Notice::Noted(_)), "{told:?}");
        assert_eq!(
            told.text(),
            "Saved the lyrics of 2 songs beside their files · A lyric sheet already stood beside one \
             song, so it was left alone: choose Over a sheet already there to replace it"
        );
    }

    #[test]
    fn lyrics_written_beside_every_song_are_done() {
        let told = counted(&[Saved::Written]).told();

        assert!(matches!(told, Notice::Done(_)), "{told:?}");
        assert_eq!(told.text(), "Saved the lyrics of one song beside its file");
    }

    #[test]
    fn a_sheet_that_could_not_be_written_is_trouble_whatever_else_was_saved() {
        let told = counted(&[Saved::Written, Saved::Unwritten]).told();

        assert!(matches!(told, Notice::Trouble(_)), "{told:?}");
    }
}
