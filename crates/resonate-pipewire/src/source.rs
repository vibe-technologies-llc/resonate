pub trait AudioSource: Send {
    fn fill(&mut self, dst: &mut [u8]) -> usize;

    fn on_underrun(&mut self, _missing_bytes: usize) {}
}
