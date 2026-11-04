use std::{fs, net::SocketAddr, path::Path};

use toml_edit::DocumentMut;

use crate::{ConfigField, Error, Result, keys::address_of};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Web {
    pub listen: SocketAddr,
    pub forwarded_by: Option<String>,
    pub android_certificates: Vec<String>,
}

impl Web {
    pub fn read(path: &Path) -> Result<Option<Self>> {
        let text = fs::read_to_string(path).map_err(|source| Error::ConfigRead {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(path, &text)
    }

    pub fn parse(path: &Path, text: &str) -> Result<Option<Self>> {
        let document: DocumentMut = text.parse().map_err(|source| Error::ConfigSyntax {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
        let Some(item) = document.get("web") else {
            return Ok(None);
        };
        let table = item.as_table().ok_or(Error::ConfigInvalid {
            field: ConfigField::Web,
        })?;
        let forwarded_by = match table.get("forwarded_by") {
            None => None,
            Some(item) => Some(
                item.as_str()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .ok_or(Error::ConfigInvalid {
                        field: ConfigField::ForwardedBy,
                    })?
                    .to_owned(),
            ),
        };
        let android_certificates = match table.get("android_certificates") {
            None => Vec::new(),
            Some(item) => item
                .as_array()
                .ok_or(Error::ConfigInvalid {
                    field: ConfigField::AndroidCertificates,
                })?
                .iter()
                .map(|certificate| {
                    certificate
                        .as_str()
                        .filter(|certificate| is_a_fingerprint(certificate))
                        .map(str::to_owned)
                        .ok_or(Error::ConfigInvalid {
                            field: ConfigField::AndroidCertificates,
                        })
                })
                .collect::<Result<Vec<_>>>()?,
        };

        Ok(Some(Self {
            listen: address_of(table, "listen", ConfigField::Listen)?,
            forwarded_by,
            android_certificates,
        }))
    }
}

fn is_a_fingerprint(text: &str) -> bool {
    !text.is_empty()
        && text
            .split(':')
            .all(|pair| pair.len() == 2 && pair.bytes().all(|digit| digit.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_certificate_named_for_app_links_is_a_fingerprint_or_the_table_is_refused() {
        let named =
            parsed("[web]\nlisten = \"127.0.0.1:8080\"\nandroid_certificates = [\"AB:CD:01\"]\n")
                .expect("parsed")
                .expect("a table");

        assert_eq!(named.android_certificates, ["AB:CD:01"]);
        assert!(
            parsed("[web]\nlisten = \"127.0.0.1:8080\"\nandroid_certificates = [\"<x>\"]\n")
                .is_err()
        );
    }

    fn parsed(text: &str) -> Result<Option<Web>> {
        Web::parse(Path::new("web.toml"), text)
    }

    #[test]
    fn no_web_table_leaves_the_listener_off() {
        assert_eq!(parsed("[hifi]\n").expect("parsed"), None);
    }

    #[test]
    fn a_web_table_names_where_to_listen_and_which_header_carries_the_client() {
        let web =
            parsed("[web]\nlisten = \"127.0.0.1:8080\"\nforwarded_by = \"CF-Connecting-IP\"\n")
                .expect("parsed")
                .expect("a table");

        assert_eq!(web.listen, "127.0.0.1:8080".parse().expect("address"));
        assert_eq!(web.forwarded_by.as_deref(), Some("CF-Connecting-IP"));
    }

    #[test]
    fn a_header_may_be_left_out_but_not_left_blank() {
        let bare = parsed("[web]\nlisten = \"127.0.0.1:8080\"\n")
            .expect("parsed")
            .expect("a table");

        assert_eq!(bare.forwarded_by, None);
        assert!(matches!(
            parsed("[web]\nlisten = \"127.0.0.1:8080\"\nforwarded_by = \" \"\n"),
            Err(Error::ConfigInvalid {
                field: ConfigField::ForwardedBy
            })
        ));
    }

    #[test]
    fn a_web_table_without_an_address_is_refused() {
        assert!(matches!(
            parsed("[web]\n"),
            Err(Error::ConfigMissing {
                field: ConfigField::Listen
            })
        ));
    }
}
