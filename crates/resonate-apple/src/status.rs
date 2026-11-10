use std::{error, fmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OsStatus(i32);

impl OsStatus {
    pub const fn get(self) -> i32 {
        self.0
    }

    pub(crate) const fn of(raw: i32) -> Self {
        Self(raw)
    }

    pub(crate) fn checked(raw: i32) -> Result<(), Self> {
        match raw {
            0 => Ok(()),
            refused => Err(Self(refused)),
        }
    }

    fn four_characters(self) -> Option<[char; 4]> {
        let letters = self.0.to_be_bytes().map(char::from);
        letters
            .iter()
            .all(|letter| letter.is_ascii_graphic() || *letter == ' ')
            .then_some(letters)
    }
}

impl fmt::Display for OsStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.four_characters() {
            Some([first, second, third, fourth]) => {
                write!(formatter, "OSStatus '{first}{second}{third}{fourth}'")
            }
            None => write!(formatter, "OSStatus {}", self.0),
        }
    }
}

impl error::Error for OsStatus {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_spelled_in_four_letters_is_shown_as_them_and_any_other_as_its_number() {
        let hogged = OsStatus::of(i32::from_be_bytes(*b"!hog"));
        let parameter = OsStatus::of(-50);

        assert_eq!(hogged.to_string(), "OSStatus '!hog'");
        assert_eq!(parameter.to_string(), "OSStatus -50");
    }

    #[test]
    fn nothing_but_zero_is_taken_as_done() {
        assert_eq!(OsStatus::checked(0), Ok(()));
        assert_eq!(OsStatus::checked(-50).map_err(OsStatus::get), Err(-50));
    }
}
