use resonate_core::Meter;

const UNRESTRICTED: i32 = 1;
const FIXED: i32 = 2;
const VARIABLE: i32 = 3;

pub fn meter_of_network_cost(value: i32) -> Meter {
    match value {
        UNRESTRICTED => Meter::Unmetered,
        FIXED | VARIABLE => Meter::Metered,
        _ => Meter::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unrestricted_connection_is_unmetered() {
        assert_eq!(meter_of_network_cost(1), Meter::Unmetered);
    }

    #[test]
    fn a_fixed_or_variable_cost_is_a_metered_connection() {
        assert_eq!(meter_of_network_cost(2), Meter::Metered);
        assert_eq!(meter_of_network_cost(3), Meter::Metered);
    }

    #[test]
    fn what_windows_does_not_know_or_a_number_it_never_sends_is_unknown() {
        assert_eq!(meter_of_network_cost(0), Meter::Unknown);
        assert_eq!(meter_of_network_cost(4), Meter::Unknown);
        assert_eq!(meter_of_network_cost(-1), Meter::Unknown);
    }
}
