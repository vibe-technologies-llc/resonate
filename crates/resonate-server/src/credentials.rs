use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use parking_lot::RwLock;
use rustls::{
    crypto::ring::default_provider,
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};

use crate::{Error, PemOp, Result};

#[derive(Debug)]
pub struct Presented {
    pair: RwLock<Arc<CertifiedKey>>,
}

pub struct Credentials {
    certificate: PathBuf,
    key: PathBuf,
    presented: Arc<Presented>,
}

impl Presented {
    pub fn of(
        chain: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> Result<Arc<Self>> {
        let pair = certified(chain, key).map_err(|source| Error::Tls(Box::new(source)))?;

        Ok(Self::holding(pair))
    }

    fn holding(pair: CertifiedKey) -> Arc<Self> {
        Arc::new(Self {
            pair: RwLock::new(Arc::new(pair)),
        })
    }
}

impl ResolvesServerCert for Presented {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(Arc::clone(&self.pair.read()))
    }
}

impl Credentials {
    pub fn read(certificate: &Path, key: &Path) -> Result<Self> {
        let pair = paired(certificate, key)?;

        Ok(Self {
            certificate: certificate.to_path_buf(),
            key: key.to_path_buf(),
            presented: Presented::holding(pair),
        })
    }

    pub fn presented(&self) -> Arc<Presented> {
        Arc::clone(&self.presented)
    }

    pub fn reload(&self) -> Result<()> {
        let pair = paired(&self.certificate, &self.key)?;

        *self.presented.pair.write() = Arc::new(pair);
        Ok(())
    }
}

fn paired(certificate: &Path, key: &Path) -> Result<CertifiedKey> {
    let (chain, private) = pem(certificate, key)?;

    certified(chain, private).map_err(|source| Error::Unpaired {
        certificate: certificate.to_path_buf(),
        key: key.to_path_buf(),
        source: Box::new(source),
    })
}

fn certified(
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> std::result::Result<CertifiedKey, rustls::Error> {
    CertifiedKey::from_der(chain, key, &default_provider())
}

fn pem(
    certificate: &Path,
    key: &Path,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let chain = CertificateDer::pem_file_iter(certificate)
        .and_then(Iterator::collect::<std::result::Result<Vec<_>, _>>)
        .map_err(|source| Error::Pem {
            op: PemOp::Certificate,
            path: certificate.to_path_buf(),
            source,
        })?;

    if chain.is_empty() {
        return Err(Error::NoCertificate {
            path: certificate.to_path_buf(),
        });
    }

    let key = PrivateKeyDer::from_pem_file(key).map_err(|source| Error::Pem {
        op: PemOp::Key,
        path: key.to_path_buf(),
        source,
    })?;
    Ok((chain, key))
}
