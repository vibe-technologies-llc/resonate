#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Serial(u64);

impl Serial {
    pub(crate) const fn moved(&mut self) -> Self {
        self.0 += 1;
        *self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_serial_taken_before_a_move_is_not_the_serial_after_it() {
        let mut serial = Serial::default();

        let before = serial;
        let after = serial.moved();

        assert_ne!(before, after);
        assert_eq!(after, serial);
        assert_ne!(serial.moved(), after);
    }
}
