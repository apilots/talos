//! Validated Worker trust configuration shared by local and Server products.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rustls::{
    pki_types::{pem::PemObject, CertificateDer},
    RootCertStore,
};
use std::path::Path;
use talos_error::{ErrorCode, TalosError, TalosResult};
/// Read only validated certificates, so an editable path cannot copy other file contents into Worker Home.
pub fn read_worker_trusted_ca(path: Option<&Path>) -> TalosResult<Option<Vec<u8>>> {
    let Some(path) = path else { return Ok(None) };
    let invalid = || {
        TalosError::new(
            ErrorCode::InvalidRequest,
            "The Worker CA file must contain usable PEM certificates.",
        )
    };
    let read_error = |error| {
        TalosError::from_source(
            ErrorCode::InvalidRequest,
            "Cannot read the Worker CA file.",
            error,
        )
    };
    if !path.metadata().map_err(read_error)?.is_file() {
        return Err(invalid());
    }
    let file = std::fs::File::open(path).map_err(read_error)?;
    let mut roots = RootCertStore::empty();
    let mut pem = String::new();
    for certificate in CertificateDer::pem_reader_iter(file) {
        let certificate = certificate.map_err(|_| invalid())?;
        if roots.add(certificate.clone()).is_err() {
            continue;
        }
        pem.push_str("-----BEGIN CERTIFICATE-----\n");
        for line in STANDARD.encode(&certificate).as_bytes().chunks(64) {
            pem.extend(line.iter().copied().map(char::from));
            pem.push('\n');
        }
        pem.push_str("-----END CERTIFICATE-----\n");
    }
    if roots.is_empty() {
        return Err(invalid());
    }
    Ok(Some(pem.into_bytes()))
}
