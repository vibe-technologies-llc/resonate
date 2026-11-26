pub(crate) trait Restorable {
    type Format;
    type Error;

    fn set_mixing(&self, mixes: bool) -> Result<(), Self::Error>;
    fn set_physical_format(&self, format: &Self::Format) -> Result<(), Self::Error>;
    fn runs_at(&self, hz: f64) -> bool;
    fn set_nominal_rate(&self, hz: f64) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Setting {
    Mixing,
    PhysicalFormat,
    NominalRate,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Unrestored<E> {
    pub(crate) setting: Setting,
    pub(crate) error: E,
}

pub(crate) struct Prior<R: Restorable> {
    pub(crate) output: R,
    pub(crate) rate: Option<f64>,
    pub(crate) format: Option<R::Format>,
    pub(crate) mixed: Option<bool>,
}

impl<R: Restorable> Prior<R> {
    pub(crate) fn put_back(&self) -> Vec<Unrestored<R::Error>> {
        let mut unrestored = Vec::new();

        let mut attempt = |setting, result: Result<(), R::Error>| {
            if let Err(error) = result {
                unrestored.push(Unrestored { setting, error });
            }
        };

        if let Some(mixed) = self.mixed {
            attempt(Setting::Mixing, self.output.set_mixing(mixed));
        }
        if let Some(format) = &self.format {
            attempt(
                Setting::PhysicalFormat,
                self.output.set_physical_format(format),
            );
        }
        if let Some(rate) = self.rate.filter(|rate| !self.output.runs_at(*rate)) {
            attempt(Setting::NominalRate, self.output.set_nominal_rate(rate));
        }
        unrestored
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Mixing(bool),
        Format(u8),
        Rate(u32),
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Refused(Setting);

    #[derive(Default)]
    struct Output {
        calls: Rc<RefCell<Vec<Call>>>,
        refusing: Vec<Setting>,
        runs_at: Option<f64>,
    }

    impl Output {
        fn answer(&self, call: Call, setting: Setting) -> Result<(), Refused> {
            self.calls.borrow_mut().push(call);
            if self.refusing.contains(&setting) {
                Err(Refused(setting))
            } else {
                Ok(())
            }
        }
    }

    impl Restorable for Output {
        type Format = u8;
        type Error = Refused;

        fn set_mixing(&self, mixes: bool) -> Result<(), Refused> {
            self.answer(Call::Mixing(mixes), Setting::Mixing)
        }

        fn set_physical_format(&self, format: &u8) -> Result<(), Refused> {
            self.answer(Call::Format(*format), Setting::PhysicalFormat)
        }

        fn runs_at(&self, hz: f64) -> bool {
            self.runs_at == Some(hz)
        }

        fn set_nominal_rate(&self, hz: f64) -> Result<(), Refused> {
            self.answer(Call::Rate(hz as u32), Setting::NominalRate)
        }
    }

    fn everything_changed(output: Output) -> Prior<Output> {
        Prior {
            output,
            rate: Some(44_100.0),
            format: Some(7),
            mixed: Some(true),
        }
    }

    #[test]
    fn what_was_changed_is_put_back_mixing_then_format_then_rate() {
        let output = Output::default();
        let calls = Rc::clone(&output.calls);

        let unrestored = everything_changed(output).put_back();

        assert!(unrestored.is_empty());
        assert_eq!(
            *calls.borrow(),
            [Call::Mixing(true), Call::Format(7), Call::Rate(44_100)]
        );
    }

    #[test]
    fn a_rate_the_device_already_runs_at_is_left_alone() {
        let output = Output {
            runs_at: Some(44_100.0),
            ..Output::default()
        };
        let calls = Rc::clone(&output.calls);

        let unrestored = everything_changed(output).put_back();

        assert!(unrestored.is_empty());
        assert_eq!(*calls.borrow(), [Call::Mixing(true), Call::Format(7)]);
    }

    #[test]
    fn what_was_never_changed_is_never_asked() {
        let output = Output::default();
        let calls = Rc::clone(&output.calls);
        let prior = Prior {
            output,
            rate: None,
            format: None,
            mixed: None,
        };

        assert!(prior.put_back().is_empty());
        assert!(calls.borrow().is_empty());
    }

    #[test]
    fn every_refusal_is_reported_and_none_stops_the_rest_from_being_tried() {
        let output = Output {
            refusing: vec![
                Setting::Mixing,
                Setting::PhysicalFormat,
                Setting::NominalRate,
            ],
            ..Output::default()
        };
        let calls = Rc::clone(&output.calls);

        let unrestored = everything_changed(output).put_back();

        assert_eq!(
            unrestored,
            [
                Unrestored {
                    setting: Setting::Mixing,
                    error: Refused(Setting::Mixing)
                },
                Unrestored {
                    setting: Setting::PhysicalFormat,
                    error: Refused(Setting::PhysicalFormat)
                },
                Unrestored {
                    setting: Setting::NominalRate,
                    error: Refused(Setting::NominalRate)
                },
            ]
        );
        assert_eq!(calls.borrow().len(), 3);
    }

    #[test]
    fn a_refusal_in_the_middle_is_the_only_one_reported() {
        let output = Output {
            refusing: vec![Setting::PhysicalFormat],
            ..Output::default()
        };

        let unrestored = everything_changed(output).put_back();

        assert_eq!(
            unrestored
                .iter()
                .map(|each| each.setting)
                .collect::<Vec<_>>(),
            [Setting::PhysicalFormat]
        );
    }
}
