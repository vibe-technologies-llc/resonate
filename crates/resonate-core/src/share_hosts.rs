use std::{collections::BTreeSet, path::Path};

use crate::os_bytes;

pub(crate) const VERBATIM_UNC: &[u8] = br"\\?\UNC\";
pub(crate) const NETWORK: &[u8] = br"\\";
pub(crate) const NOT_A_SHARE: &[u8] = b"?.";
const SLASH: u8 = b'/';
const BACKSLASH: u8 = b'\\';

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShareHosts {
    anyone: bool,
    named: BTreeSet<Vec<u8>>,
}

impl ShareHosts {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn anyone() -> Self {
        Self {
            anyone: true,
            named: BTreeSet::new(),
        }
    }

    pub fn rooting<P: AsRef<Path>>(roots: impl IntoIterator<Item = P>) -> Self {
        Self {
            anyone: false,
            named: roots
                .into_iter()
                .filter_map(|root| host_of_path(&os_bytes::bytes_of(root.as_ref().as_os_str())))
                .collect(),
        }
    }

    pub fn admits(&self, host: &[u8]) -> bool {
        self.anyone || self.named.contains(&host.to_ascii_lowercase())
    }
}

pub(crate) fn host_of_uri_rest(decoded: &[u8]) -> &[u8] {
    decoded
        .split(|byte| *byte == SLASH || *byte == BACKSLASH)
        .next()
        .unwrap_or_default()
}

fn host_of_path(path: &[u8]) -> Option<Vec<u8>> {
    let share = path
        .strip_prefix(VERBATIM_UNC)
        .or_else(|| path.strip_prefix(NETWORK))?;

    if share
        .first()
        .is_none_or(|first| NOT_A_SHARE.contains(first))
    {
        return None;
    }

    let host = host_of_uri_rest(share);

    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_admitted_where_a_root_is_a_share_on_it_in_any_case() {
        let hosts = ShareHosts::rooting([
            Path::new(r"\\NAS\music"),
            Path::new(r"\\?\UNC\vault\songs"),
            Path::new(r"C:\Music"),
            Path::new(r"\\?\D:\Music"),
            Path::new(r"\\.\pipe\resonate"),
        ]);

        assert!(hosts.admits(b"nas"));
        assert!(hosts.admits(b"Vault"));
        assert!(!hosts.admits(b"elsewhere"));
        assert!(!hosts.admits(b"C:"));
        assert!(!hosts.admits(b"?"));
        assert!(!hosts.admits(b"."));
    }

    #[test]
    fn no_hosts_admit_nothing_and_anyone_admits_everything() {
        assert!(!ShareHosts::none().admits(b"nas"));
        assert!(!ShareHosts::rooting(Vec::<&Path>::new()).admits(b"nas"));
        assert!(ShareHosts::anyone().admits(b"nas"));
    }
}
