use std::num::NonZeroUsize;

pub struct Recall {
    samples: Vec<f64>,
    channels: NonZeroUsize,
    frames: usize,
    entered: u64,
}

impl Recall {
    pub fn new(channels: NonZeroUsize, frames: usize) -> Self {
        Self {
            samples: vec![0.0; frames.saturating_mul(channels.get())],
            channels,
            frames,
            entered: 0,
        }
    }

    pub const fn entered(&self) -> u64 {
        self.entered
    }

    pub const fn oldest(&self) -> u64 {
        self.entered.saturating_sub(self.frames as u64)
    }

    pub const fn forget(&mut self) {
        self.entered = 0;
    }

    pub fn enter(&mut self, block: &[f64]) {
        let channels = self.channels.get();
        let given = block.len() / channels;
        if self.frames == 0 {
            self.entered = self.entered.saturating_add(given as u64);
            return;
        }
        let passed_over = given.saturating_sub(self.frames);
        self.entered = self.entered.saturating_add(passed_over as u64);
        let mut from = block
            .get(passed_over * channels..given * channels)
            .unwrap_or_default();
        let mut remaining = given - passed_over;
        while remaining > 0 {
            let slot = (self.entered % self.frames as u64) as usize;
            let run = remaining.min(self.frames - slot);
            let samples = run * channels;
            if let (Some(into), Some(taken)) = (
                self.samples
                    .get_mut(slot * channels..slot * channels + samples),
                from.get(..samples),
            ) {
                into.copy_from_slice(taken);
            }
            from = from.get(samples..).unwrap_or_default();
            self.entered = self.entered.saturating_add(run as u64);
            remaining -= run;
        }
    }

    pub fn recalled(&self, from: u64, into: &mut [f64]) -> usize {
        if from < self.oldest() || from >= self.entered || self.frames == 0 {
            return 0;
        }
        let channels = self.channels.get();
        let frames = (into.len() / channels).min((self.entered - from) as usize);
        let mut copied = 0;
        while copied < frames {
            let slot = ((from + copied as u64) % self.frames as u64) as usize;
            let run = (frames - copied).min(self.frames - slot);
            let samples = run * channels;
            if let (Some(taken), Some(laid)) = (
                self.samples.get(slot * channels..slot * channels + samples),
                into.get_mut(copied * channels..copied * channels + samples),
            ) {
                laid.copy_from_slice(taken);
            }
            copied += run;
        }
        frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo() -> NonZeroUsize {
        NonZeroUsize::new(2).expect("two channels")
    }

    fn counting(from: usize, frames: usize) -> Vec<f64> {
        (from * 2..(from + frames) * 2)
            .map(|sample| sample as f64)
            .collect()
    }

    #[test]
    fn what_was_entered_is_recalled_from_any_frame_it_still_holds() {
        let mut recall = Recall::new(stereo(), 8);
        recall.enter(&counting(0, 5));
        recall.enter(&counting(5, 6));

        let mut into = vec![0.0; 2 * 8];
        assert_eq!(recall.oldest(), 3);
        assert_eq!(recall.entered(), 11);
        assert_eq!(recall.recalled(3, &mut into), 8);
        assert_eq!(into, counting(3, 8));
        assert_eq!(recall.recalled(9, &mut into), 2);
        assert_eq!(&into[..4], &counting(9, 2)[..]);
    }

    #[test]
    fn a_frame_older_than_it_holds_is_not_recalled() {
        let mut recall = Recall::new(stereo(), 4);
        recall.enter(&counting(0, 10));

        let mut into = vec![0.0; 8];
        assert_eq!(recall.recalled(5, &mut into), 0);
        assert_eq!(recall.recalled(6, &mut into), 4);
        assert_eq!(into, counting(6, 4));
    }

    #[test]
    fn forgetting_starts_the_count_over() {
        let mut recall = Recall::new(stereo(), 4);
        recall.enter(&counting(0, 3));
        recall.forget();
        recall.enter(&counting(7, 2));

        let mut into = vec![0.0; 4];
        assert_eq!(recall.entered(), 2);
        assert_eq!(recall.recalled(0, &mut into), 2);
        assert_eq!(into, counting(7, 2));
    }
}
