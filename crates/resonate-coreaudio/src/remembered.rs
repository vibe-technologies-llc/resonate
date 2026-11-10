use std::collections::BTreeMap;

use crate::formats::Physical;

#[derive(Debug, Default)]
pub struct Remembered {
    taken_while_free: BTreeMap<String, Vec<Physical>>,
}

impl Remembered {
    pub fn read_or_recalled(&mut self, uid: &str, read: Option<Vec<Physical>>) -> Vec<Physical> {
        match read {
            Some(offered) => {
                self.taken_while_free
                    .insert(uid.to_owned(), offered.clone());
                offered
            }
            None => self.taken_while_free.get(uid).cloned().unwrap_or_default(),
        }
    }

    pub fn keep_only(&mut self, present: &[&str]) {
        self.taken_while_free
            .retain(|uid, _| present.contains(&uid.as_str()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::Depth;

    fn offered() -> Vec<Physical> {
        vec![Physical {
            depth: Depth::TwentyFour,
            channels: 2,
            lowest_hz: 96_000.0,
            highest_hz: 96_000.0,
        }]
    }

    #[test]
    fn a_device_held_by_another_program_keeps_the_formats_it_took_while_free() {
        let mut remembered = Remembered::default();

        assert_eq!(
            remembered.read_or_recalled("dac", Some(offered())),
            offered()
        );
        assert_eq!(remembered.read_or_recalled("dac", None), offered());
    }

    #[test]
    fn a_device_never_read_while_free_offers_nothing() {
        let mut remembered = Remembered::default();

        assert!(remembered.read_or_recalled("dac", None).is_empty());
    }

    #[test]
    fn a_device_that_left_is_forgotten() {
        let mut remembered = Remembered::default();
        remembered.read_or_recalled("dac", Some(offered()));
        remembered.read_or_recalled("speakers", Some(offered()));

        remembered.keep_only(&["speakers"]);

        assert!(remembered.read_or_recalled("dac", None).is_empty());
        assert_eq!(remembered.read_or_recalled("speakers", None), offered());
    }
}
