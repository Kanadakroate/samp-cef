use quinn::{Endpoint, ServerConfig, TransportConfig};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use std::sync::Arc;
use std::time::Duration;

pub fn make_from_files(bind_addr: std::net::SocketAddr) -> anyhow::Result<Endpoint> {
    let cert_path = std::env::var_os("SAMP_CEF_SERVER_CERT")
        .ok_or_else(|| anyhow::anyhow!("SAMP_CEF_SERVER_CERT must name the persistent DER certificate"))?;
    let key_path = std::env::var_os("SAMP_CEF_SERVER_KEY")
        .ok_or_else(|| anyhow::anyhow!("SAMP_CEF_SERVER_KEY must name the local PKCS#8 DER private key"))?;
    let certificate = CertificateDer::from(std::fs::read(cert_path)?);
    let private_key = PrivatePkcs8KeyDer::from(std::fs::read(key_path)?);
    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(1)));
    transport.max_concurrent_uni_streams(16u32.into());
    transport.max_concurrent_bidi_streams(0u32.into());
    transport.stream_receive_window((64 * 1024u32).into());
    transport.receive_window((256 * 1024u32).into());
    let mut config = ServerConfig::with_single_cert(vec![certificate], private_key.into())?;
    config.transport = Arc::new(transport);
    Ok(Endpoint::server(config, bind_addr)?)
}
