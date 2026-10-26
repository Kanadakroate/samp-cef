use quinn::crypto::rustls::QuicClientConfig;
use quinn::{ClientConfig, Endpoint};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as RustlsError, RootCertStore, SignatureScheme};
use std::sync::Arc;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Provisioned release certificate; no fallback. WebPKI still checks validity,
/// DNS name and actual TLS handshake signatures. Zero-RTT remains disabled.
#[derive(Debug)]
struct PinnedServerVerification {
    certificate: CertificateDer<'static>,
    verifier: Arc<WebPkiServerVerifier>,
}

impl ServerCertVerifier for PinnedServerVerification {
    fn verify_server_cert(
        &self, end_entity: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>, ocsp_response: &[u8], now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        if end_entity.as_ref() != self.certificate.as_ref() {
            return Err(RustlsError::General("CEF release certificate pin mismatch".into()));
        }
        self.verifier.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
    }
    fn verify_tls12_signature(
        &self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.verifier.verify_tls12_signature(message, cert, dss)
    }
    fn verify_tls13_signature(
        &self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.verifier.verify_tls13_signature(message, cert, dss)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.verifier.supported_verify_schemes()
    }
}

fn release_pin_path(
    override_path: Option<&OsStr>,
    current_exe: impl FnOnce() -> std::io::Result<PathBuf>,
) -> anyhow::Result<PathBuf> {
    if let Some(value) = override_path {
        let path = PathBuf::from(value);
        if value.is_empty() || !path.is_absolute() {
            return Err(anyhow::anyhow!("SAMP_CEF_PIN_CERT must be a nonempty absolute DER certificate path"));
        }
        return Ok(path);
    }
    let executable = current_exe()?;
    if !executable.is_absolute() {
        return Err(anyhow::anyhow!("The current executable path must be absolute"));
    }
    let directory = executable.parent().ok_or_else(|| anyhow::anyhow!("The current executable has no parent directory"))?;
    Ok(directory.join("cef").join("server.der"))
}

fn pinned_client_config(pin_path: &Path) -> anyhow::Result<ClientConfig> {
    let certificate = CertificateDer::from(std::fs::read(pin_path)?);
    let mut roots = RootCertStore::empty();
    roots.add(certificate.clone())?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone()).build()?;
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedServerVerification { certificate, verifier }))
        .with_no_client_auth();
    tls.enable_early_data = false;
    Ok(ClientConfig::new(Arc::new(QuicClientConfig::try_from(tls)?)))
}

pub fn make_pinned_client(bind_addr: std::net::SocketAddr) -> anyhow::Result<Endpoint> {
    let override_path = std::env::var_os("SAMP_CEF_PIN_CERT");
    let pin_path = release_pin_path(override_path.as_deref(), std::env::current_exe)?;
    let config = pinned_client_config(&pin_path)?;
    let mut endpoint = Endpoint::client(bind_addr)?;
    endpoint.set_default_client_config(config);
    Ok(endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn verifier(cert: CertificateDer<'static>) -> PinnedServerVerification {
        let mut roots = RootCertStore::empty();
        roots.add(cert.clone()).unwrap();
        let verifier = WebPkiServerVerifier::builder_with_provider(
            Arc::new(roots), Arc::new(rustls::crypto::ring::default_provider()),
        ).build().unwrap();
        PinnedServerVerification { certificate: cert, verifier }
    }
    #[test]
    fn exact_certificate_and_dns_name_required() {
        let good = rcgen::generate_simple_self_signed(vec!["samp.cef".into()]).unwrap();
        let good = CertificateDer::from(good.cert);
        let other = rcgen::generate_simple_self_signed(vec!["samp.cef".into()]).unwrap();
        let other = CertificateDer::from(other.cert);
        let verifier = verifier(good.clone());
        let name = ServerName::try_from("samp.cef").unwrap();
        assert!(verifier.verify_server_cert(&good, &[], &name, &[], UnixTime::now()).is_ok());
        assert!(verifier.verify_server_cert(&other, &[], &name, &[], UnixTime::now()).is_err());
        let wrong_name = ServerName::try_from("attacker.example").unwrap();
        assert!(verifier.verify_server_cert(&good, &[], &wrong_name, &[], UnixTime::now()).is_err());
    }

    #[test]
    fn packaged_pin_uses_executable_parent_and_explicit_override_is_absolute() {
        let root = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let executable = root.join("game").join("gta_sa.exe");
        assert_eq!(release_pin_path(None, || Ok(executable.clone())).unwrap(),
            root.join("game").join("cef").join("server.der"));
        let explicit = root.join("lab").join("pin.der");
        assert_eq!(release_pin_path(Some(explicit.as_os_str()), || panic!("override must not depend on executable lookup")).unwrap(), explicit);
        assert!(release_pin_path(None, || Ok(PathBuf::from("relative-game.exe"))).is_err());
        assert!(release_pin_path(None, || Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))).is_err());
        for value in ["", "server.der", "./cef/server.der", "../server.der"] {
            assert!(release_pin_path(Some(OsStr::new(value)), || panic!("invalid override must not fall back")).is_err());
        }
    }

    #[test]
    fn actual_pin_read_errors_and_invalid_der_fail_closed() {
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                for file in ["empty.der", "broken.der", "valid.der"] {
                    let _ = std::fs::remove_file(self.0.join(file));
                }
                let _ = std::fs::remove_dir(&self.0);
            }
        }
        let unique = format!("lr-cef-pin-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let base = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let scratch = Scratch(base.join(unique));
        std::fs::create_dir(&scratch.0).unwrap();
        assert!(pinned_client_config(&scratch.0.join("missing.der")).is_err());
        assert!(pinned_client_config(&scratch.0).is_err());
        std::fs::write(scratch.0.join("empty.der"), []).unwrap();
        std::fs::write(scratch.0.join("broken.der"), b"not a DER certificate").unwrap();
        assert!(pinned_client_config(&scratch.0.join("empty.der")).is_err());
        assert!(pinned_client_config(&scratch.0.join("broken.der")).is_err());
        let cert = rcgen::generate_simple_self_signed(vec!["samp.cef".into()]).unwrap();
        std::fs::write(scratch.0.join("valid.der"), cert.cert.der()).unwrap();
        assert!(pinned_client_config(&scratch.0.join("valid.der")).is_ok());
        let missing_override = release_pin_path(Some(scratch.0.join("missing.der").as_os_str()), || panic!("no fallback")).unwrap();
        assert!(pinned_client_config(&missing_override).is_err());
    }
}
