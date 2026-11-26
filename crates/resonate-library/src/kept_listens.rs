use std::{fs::File, io::BufReader, path::Path, vec};

use crate::{
    BroughtIn, Error, HeardElsewhere, HeardOnce, History, HistoryFrom, HistoryRead, Library,
    ListeningService, Result, line_of, listen_lines,
};

pub const LISTENS_READ_AT_ONCE: usize = 1_000;

pub const LINES_READ_AS: ListeningService = ListeningService::ListenBrainz;

pub trait HistoryPages {
    fn page(&mut self, from: HistoryFrom, most: usize) -> Result<HistoryRead>;
}

pub struct Unpinned<'a, K: ?Sized>(pub &'a K);

impl<K: KeptListens + ?Sized> HistoryPages for Unpinned<'_, K> {
    fn page(&mut self, from: HistoryFrom, most: usize) -> Result<HistoryRead> {
        self.0.history_from(from, most)
    }
}

pub trait KeptListens {
    fn history_from(&self, from: HistoryFrom, most: usize) -> Result<HistoryRead>;

    fn history_pages(&self) -> Result<Box<dyn HistoryPages + '_>> {
        Ok(Box::new(Unpinned(self)))
    }

    fn bring_in(&self, service: ListeningService, heard: &[HeardElsewhere]) -> Result<BroughtIn>;
}

impl KeptListens for Library {
    fn history_from(&self, from: HistoryFrom, most: usize) -> Result<HistoryRead> {
        Self::history_from(self, from, most)
    }

    fn history_pages(&self) -> Result<Box<dyn HistoryPages + '_>> {
        Ok(Box::new(Self::history_pages(self)?))
    }

    fn bring_in(&self, service: ListeningService, heard: &[HeardElsewhere]) -> Result<BroughtIn> {
        Self::bring_in(self, service, heard)
    }
}

pub struct HistoryLines<'a> {
    kept: &'a dyn KeptListens,
    pages: Option<Box<dyn HistoryPages + 'a>>,
    from: Option<HistoryFrom>,
    page: vec::IntoIter<HeardOnce>,
}

impl Iterator for HistoryLines<'_> {
    type Item = Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(heard) = self.page.next() {
                return Some(Ok(line_of(&heard)));
            }

            let from = self.from.take()?;

            if self.pages.is_none() {
                match self.kept.history_pages() {
                    Ok(pages) => self.pages = Some(pages),
                    Err(error) => return Some(Err(error)),
                }
            }

            let pages = self.pages.as_mut()?;

            match pages.page(from, LISTENS_READ_AT_ONCE) {
                Ok(read) => {
                    self.from = read.next;
                    self.page = read.heard.into_iter();
                }
                Err(error) => return Some(Err(error)),
            }
        }
    }
}

pub fn history_lines(kept: &dyn KeptListens) -> HistoryLines<'_> {
    HistoryLines {
        kept,
        pages: None,
        from: Some(HistoryFrom::START),
        page: Vec::new().into_iter(),
    }
}

pub fn bring_in_lines(
    kept: &dyn KeptListens,
    bytes: &[u8],
    so_far: &mut dyn FnMut(BroughtIn),
) -> Result<BroughtIn> {
    brought_in_pages(
        kept,
        |each| listen_lines::each_listen_in_bytes(bytes, each),
        so_far,
    )
}

pub fn bring_in_file(
    kept: &dyn KeptListens,
    path: &Path,
    so_far: &mut dyn FnMut(BroughtIn),
) -> Result<BroughtIn> {
    let unread = |source| Error::Io {
        path: path.to_path_buf(),
        source,
    };
    brought_in_pages(
        kept,
        |each| {
            let file = File::open(path).map_err(unread)?;
            listen_lines::each_listen_read(BufReader::new(file), &unread, each)
        },
        so_far,
    )
}

fn brought_in_pages(
    kept: &dyn KeptListens,
    mut read: impl FnMut(&mut dyn FnMut(HeardElsewhere) -> Result<()>) -> Result<()>,
    so_far: &mut dyn FnMut(BroughtIn),
) -> Result<BroughtIn> {
    read(&mut |_| Ok(()))?;

    let mut brought = BroughtIn::default();
    let mut page = Vec::with_capacity(LISTENS_READ_AT_ONCE);
    let mut landed = |page: &mut Vec<HeardElsewhere>| -> Result<()> {
        brought = brought.and(kept.bring_in(LINES_READ_AS, page)?);
        page.clear();
        so_far(brought);
        Ok(())
    };

    read(&mut |heard| {
        page.push(heard);
        match page.len() {
            LISTENS_READ_AT_ONCE => landed(&mut page),
            _ => Ok(()),
        }
    })?;
    if !page.is_empty() {
        landed(&mut page)?;
    }
    Ok(brought)
}

pub fn bring_in_history(
    kept: &dyn KeptListens,
    history: &dyn History,
    so_far: &mut dyn FnMut(BroughtIn),
) -> Result<BroughtIn> {
    let service = history.service();

    let mut brought = BroughtIn::default();
    let mut from = None;
    loop {
        let page = history.page(from)?;

        brought = brought.and(kept.bring_in(service, &page.heard)?);
        so_far(brought);
        match page.next {
            Some(next) if Some(next) != from => from = Some(next),
            Some(_) | None => return Ok(brought),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        io::{self, BufRead, Read},
        rc::Rc,
        time::{Duration, UNIX_EPOCH},
    };

    use resonate_core::Recording;

    use super::*;
    use crate::HeardPage;

    #[derive(Default)]
    struct Kept {
        held: Vec<HeardOnce>,
        brought: RefCell<Vec<(ListeningService, usize)>>,
    }

    impl KeptListens for Kept {
        fn history_from(&self, from: HistoryFrom, most: usize) -> Result<HistoryRead> {
            let HistoryFrom::Held(after) = from else {
                return Ok(HistoryRead {
                    heard: Vec::new(),
                    next: None,
                });
            };
            let after = usize::try_from(after).expect("a place");
            let heard = self
                .held
                .iter()
                .skip(after)
                .take(most)
                .cloned()
                .collect::<Vec<_>>();

            let next = (after + heard.len() < self.held.len())
                .then(|| HistoryFrom::Held((after + heard.len()) as u64));
            Ok(HistoryRead { heard, next })
        }

        fn bring_in(
            &self,
            service: ListeningService,
            heard: &[HeardElsewhere],
        ) -> Result<BroughtIn> {
            self.brought.borrow_mut().push((service, heard.len()));
            Ok(BroughtIn {
                credited: heard.len() as u64,
                held_already: 0,
                unmatched: 0,
            })
        }
    }

    fn once(second: u64) -> HeardOnce {
        HeardOnce {
            began: UNIX_EPOCH + Duration::from_secs(second),
            heard: Duration::from_secs(1),
            elsewhere: false,
            billed: None,
            location: format!("file:///{second}.flac"),
        }
    }

    struct Paged(Vec<HeardPage>);

    impl History for Paged {
        fn service(&self) -> ListeningService {
            ListeningService::LastFm
        }

        fn page(&self, from: Option<u64>) -> Result<HeardPage> {
            let at = usize::try_from(from.unwrap_or(0)).expect("a page");
            Ok(self.0[at].clone())
        }
    }

    fn heard_at(second: u64) -> HeardElsewhere {
        HeardElsewhere {
            at: UNIX_EPOCH + Duration::from_secs(second),
            recording: Recording {
                mbid: None,
                title: "Words".to_owned(),
                artist: "Low".to_owned(),
                album: String::new(),
                length: None,
            },
        }
    }

    #[test]
    fn every_listen_kept_is_one_line_across_the_pages_it_is_read_in() {
        let kept = Kept {
            held: (0..2_500).map(once).collect(),
            ..Kept::default()
        };

        let lines = history_lines(&kept)
            .collect::<Result<Vec<_>>>()
            .expect("the lines");

        assert_eq!(lines.len(), 2_500);
        assert!(lines[2_499].starts_with("{\"listened_at\":2499,"));
    }

    #[test]
    fn a_services_pages_are_brought_in_until_it_names_no_next_or_the_same_one_again() {
        let kept = Kept::default();
        let history = Paged(vec![
            HeardPage {
                heard: vec![heard_at(1), heard_at(2)],
                next: Some(1),
            },
            HeardPage {
                heard: vec![heard_at(3)],
                next: Some(1),
            },
        ]);
        let mut told = Vec::new();

        let brought = bring_in_history(&kept, &history, &mut |so_far| told.push(so_far.credited))
            .expect("brought in");

        assert_eq!(brought.credited, 3);
        assert_eq!(told, [2, 3]);
        assert_eq!(
            *kept.brought.borrow(),
            [(ListeningService::LastFm, 2), (ListeningService::LastFm, 1)]
        );
    }

    #[test]
    fn a_file_is_brought_in_as_listenbrainz_a_thousand_listens_at_a_time() {
        let kept = Kept {
            held: (0..1_500).map(once).collect(),
            ..Kept::default()
        };
        let lines = history_lines(&kept)
            .map(|line| line.map(|line| format!("{line}\n")))
            .collect::<Result<String>>()
            .expect("the lines");
        let named = lines.replace(
            "\"track_metadata\":{",
            "\"track_metadata\":{\"artist_name\":\"Low\",\"track_name\":\"Words\",",
        );

        let brought = bring_in_lines(&kept, named.as_bytes(), &mut |_| {}).expect("brought in");

        assert_eq!(brought.credited, 1_500);
        assert_eq!(
            *kept.brought.borrow(),
            [(LINES_READ_AS, LISTENS_READ_AT_ONCE), (LINES_READ_AS, 500)]
        );
    }

    const LISTENED: &str =
        r#"{"listened_at":1700000000,"track_metadata":{"artist_name":"Low","track_name":"Words"}}"#;

    struct OneLineAtATime {
        lines: usize,
        handed: Rc<Cell<usize>>,
        line: Vec<u8>,
        at: usize,
    }

    impl OneLineAtATime {
        fn new(lines: usize, handed: &Rc<Cell<usize>>) -> Self {
            handed.set(0);
            Self {
                lines,
                handed: Rc::clone(handed),
                line: Vec::new(),
                at: 0,
            }
        }
    }

    impl Read for OneLineAtATime {
        fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
            let held = self.fill_buf()?;
            let length = held.len().min(into.len());

            into[..length].copy_from_slice(&held[..length]);
            self.consume(length);
            Ok(length)
        }
    }

    impl BufRead for OneLineAtATime {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            if self.at == self.line.len() && self.handed.get() < self.lines {
                self.line = format!("{LISTENED}\n").into_bytes();
                self.at = 0;
                self.handed.set(self.handed.get() + 1);
            }
            Ok(&self.line[self.at..])
        }

        fn consume(&mut self, amount: usize) {
            self.at += amount;
        }
    }

    struct Watched<'a> {
        handed: &'a Cell<usize>,
        brought: Cell<usize>,
        ahead: RefCell<Vec<usize>>,
    }

    impl KeptListens for Watched<'_> {
        fn history_from(&self, _: HistoryFrom, _: usize) -> Result<HistoryRead> {
            Ok(HistoryRead {
                heard: Vec::new(),
                next: None,
            })
        }

        fn bring_in(&self, _: ListeningService, heard: &[HeardElsewhere]) -> Result<BroughtIn> {
            self.ahead
                .borrow_mut()
                .push(self.handed.get() - self.brought.get());
            self.brought.set(self.brought.get() + heard.len());
            Ok(BroughtIn {
                credited: heard.len() as u64,
                held_already: 0,
                unmatched: 0,
            })
        }
    }

    #[test]
    fn a_file_read_as_it_is_brought_in_holds_no_more_than_a_page_of_listens_at_once() {
        const LINES: usize = 2_500;

        let handed = Rc::new(Cell::new(0));
        let kept = Watched {
            handed: &handed,
            brought: Cell::new(0),
            ahead: RefCell::new(Vec::new()),
        };
        let unread = |source| Error::Io {
            path: "listens.jsonl".into(),
            source,
        };
        let mut opened = 0;

        let brought = brought_in_pages(
            &kept,
            |each| {
                opened += 1;
                listen_lines::each_listen_read(OneLineAtATime::new(LINES, &handed), &unread, each)
            },
            &mut |_| {},
        )
        .expect("brought in");

        assert_eq!(opened, 2);
        assert_eq!(brought.credited, LINES as u64);
        assert_eq!(kept.brought.get(), LINES);
        assert_eq!(
            *kept.ahead.borrow(),
            [LISTENS_READ_AT_ONCE, LISTENS_READ_AT_ONCE, 500]
        );
    }

    #[test]
    fn a_file_with_a_bad_line_past_the_first_page_brings_nothing_in() {
        let kept = Kept::default();
        let mut lines = vec![LISTENED; LISTENS_READ_AT_ONCE * 2].join("\n");

        lines.push_str("\nnot a listen\n");

        let refused = bring_in_lines(&kept, lines.as_bytes(), &mut |_| {});

        assert!(matches!(
            refused,
            Err(Error::ListenLine { line, .. }) if line == LISTENS_READ_AT_ONCE * 2 + 1
        ));
        assert!(kept.brought.borrow().is_empty());
    }
}
