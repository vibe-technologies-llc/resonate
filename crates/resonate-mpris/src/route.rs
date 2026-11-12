use resonate_core::Meter;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Path {
    pub reachable: bool,
    pub expensive: bool,
    pub constrained: bool,
}

pub fn meter_of_the_path(path: Path) -> Meter {
    match path {
        Path {
            reachable: false, ..
        } => Meter::Unknown,
        Path {
            expensive: false,
            constrained: false,
            ..
        } => Meter::Unmetered,
        Path { .. } => Meter::Metered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REACHED: Path = Path {
        reachable: true,
        expensive: false,
        constrained: false,
    };

    #[test]
    fn a_path_neither_expensive_nor_constrained_is_unmetered() {
        assert_eq!(meter_of_the_path(REACHED), Meter::Unmetered);
    }

    #[test]
    fn a_cellular_or_hotspot_path_and_low_data_mode_are_metered() {
        let expensive = Path {
            expensive: true,
            ..REACHED
        };
        let constrained = Path {
            constrained: true,
            ..REACHED
        };

        assert_eq!(meter_of_the_path(expensive), Meter::Metered);
        assert_eq!(meter_of_the_path(constrained), Meter::Metered);
    }

    #[test]
    fn no_path_out_says_nothing_of_the_meter() {
        let unreached = Path {
            reachable: false,
            expensive: true,
            constrained: true,
        };

        assert_eq!(meter_of_the_path(unreached), Meter::Unknown);
    }
}
