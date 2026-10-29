use crate::Stamp;

pub trait AudioSource: Send {
    fn fill(&mut self, dst: &mut [u8]) -> usize;

    fn fill_stamped(&mut self, dst: &mut [u8], _stamp: Stamp) -> usize {
        self.fill(dst)
    }
}
