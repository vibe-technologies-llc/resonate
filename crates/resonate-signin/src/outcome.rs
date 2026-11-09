use std::process::ExitStatus;

pub const SIGNED_IN: i32 = 0;

pub const CLOSED: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    SignedIn,
    Closed,
    Failed { code: Option<i32> },
}

impl Outcome {
    pub fn of(status: ExitStatus) -> Self {
        match status.code() {
            Some(SIGNED_IN) => Self::SignedIn,
            Some(CLOSED) => Self::Closed,
            code => Self::Failed { code },
        }
    }

    pub const fn code(self) -> i32 {
        match self {
            Self::SignedIn => SIGNED_IN,
            Self::Closed => CLOSED,
            Self::Failed { code: Some(code) } => code,
            Self::Failed { code: None } => FAILED_WITH_NO_CODE,
        }
    }
}

const FAILED_WITH_NO_CODE: i32 = 1;

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    #[cfg(unix)]
    fn exiting_with(code: i32) -> std::process::ExitStatus {
        Command::new("sh")
            .arg("-c")
            .arg(format!("exit {code}"))
            .status()
            .expect("a shell")
    }

    #[cfg(windows)]
    fn exiting_with(code: i32) -> std::process::ExitStatus {
        Command::new("cmd")
            .args(["/C", &format!("exit {code}")])
            .status()
            .expect("a shell")
    }

    #[test]
    fn each_exit_status_reads_as_its_outcome() {
        assert_eq!(Outcome::of(exiting_with(SIGNED_IN)), Outcome::SignedIn);
        assert_eq!(Outcome::of(exiting_with(CLOSED)), Outcome::Closed);
        assert_eq!(
            Outcome::of(exiting_with(1)),
            Outcome::Failed { code: Some(1) }
        );
        assert_eq!(Outcome::SignedIn.code(), SIGNED_IN);
        assert_eq!(Outcome::Closed.code(), CLOSED);
    }
}
