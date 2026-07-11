//! [go-plugin] automatic mTLS support.
//!
//! When Grafana serves a backend plugin with AutoMTLS it generates an ephemeral
//! client certificate and passes it to the plugin process in the
//! `PLUGIN_CLIENT_CERT` environment variable (PEM). The plugin must, in response:
//!
//! - generate its own ephemeral self-signed server certificate,
//! - serve gRPC over TLS, requiring and verifying the host's client certificate,
//! - advertise its server certificate as the sixth `|`-separated field of the
//!   go-plugin handshake line, as base64 (raw standard, no padding) of the DER.
//!
//! Grafana's client certificate is ECDSA P-521 and self-signed, so verification
//! uses the aws-lc-rs crypto provider (for P-521) and a pinned verifier that
//! matches the presented certificate against `PLUGIN_CLIENT_CERT` exactly, rather
//! than a WebPKI chain (which rejects a self-signed end-entity certificate).
//!
//! [go-plugin]: https://github.com/hashicorp/go-plugin
use std::{
    io,
    pin::Pin,
    sync::{Arc, OnceLock},
    task::{Context, Poll},
};

use base64::Engine as _;
use rustls::{
    client::danger::HandshakeSignatureValid,
    crypto::WebPkiSupportedAlgorithms,
    pki_types::{pem::PemObject as _, CertificateDer, PrivateKeyDer, UnixTime},
    server::danger::{ClientCertVerified, ClientCertVerifier},
    DigitallySignedStruct, DistinguishedName, ServerConfig, SignatureScheme,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::server::TlsStream;
use tonic::transport::server::Connected;

/// The name of the environment variable Grafana uses to pass its client
/// certificate to the plugin.
const CLIENT_CERT_ENV: &str = "PLUGIN_CLIENT_CERT";

/// Holds the negotiated server TLS configuration between [`initialize`] and
/// [`Plugin::start`], so the public API can stay unchanged.
///
/// [`initialize`]: crate::backend::initialize
/// [`Plugin::start`]: crate::backend::Plugin::start
pub(crate) static SERVER_TLS: OnceLock<Option<Arc<ServerConfig>>> = OnceLock::new();

/// Errors that can occur while setting up automatic mTLS.
#[derive(Debug, thiserror::Error)]
pub(crate) enum MtlsError {
    /// The client certificate in `PLUGIN_CLIENT_CERT` could not be parsed.
    #[error("could not parse the client certificate in {CLIENT_CERT_ENV}: {0}")]
    ClientCert(rustls::pki_types::pem::Error),
    /// The server certificate could not be generated.
    #[error("could not generate the server certificate: {0}")]
    Generate(#[from] rcgen::Error),
    /// The rustls server configuration was invalid.
    #[error("could not build the TLS configuration: {0}")]
    Config(#[from] rustls::Error),
}

/// Build the server TLS configuration from the environment, if the host requested
/// automatic mTLS.
///
/// Returns `Ok(None)` when `PLUGIN_CLIENT_CERT` is unset or empty (plaintext), or
/// `Ok(Some((config, cert_b64)))` where `cert_b64` is the server certificate to
/// advertise in the handshake line.
pub(crate) fn server_tls_from_env() -> Result<Option<(Arc<ServerConfig>, String)>, MtlsError> {
    let client_cert = match std::env::var(CLIENT_CERT_ENV) {
        Ok(pem) if !pem.is_empty() => pem,
        _ => return Ok(None),
    };
    build_server_tls(&client_cert).map(Some)
}

/// Build the server TLS configuration that pins `client_cert_pem` as the only
/// accepted client certificate, returning the config and the base64 (raw
/// standard) DER of the generated server certificate to advertise.
pub(crate) fn build_server_tls(
    client_cert_pem: &str,
) -> Result<(Arc<ServerConfig>, String), MtlsError> {
    // Parse the (single) client certificate that we will pin.
    let expected = CertificateDer::from_pem_slice(client_cert_pem.as_bytes())
        .map_err(MtlsError::ClientCert)?;

    // Generate our own ephemeral self-signed server certificate.
    let (cert_der, key_der) = generate_server_cert()?;
    let cert_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(cert_der.as_ref());

    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let algorithms = provider.signature_verification_algorithms;
    let verifier = Arc::new(PinnedClientCertVerifier {
        expected: expected.into_owned(),
        algorithms,
    });

    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(MtlsError::Config)?
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![cert_der], key_der)?;
    // gRPC requires the "h2" ALPN protocol to be negotiated.
    config.alpn_protocols = vec![b"h2".to_vec()];

    Ok((Arc::new(config), cert_b64))
}

/// Generate a self-signed server certificate mirroring go-plugin's, returning the
/// DER certificate and its PKCS#8 private key.
fn generate_server_cert() -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>), MtlsError> {
    use rcgen::{
        BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
        KeyUsagePurpose,
    };

    let mut params = CertificateParams::new(vec!["localhost".to_owned()])?;
    params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "HashiCorp");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
        KeyUsagePurpose::KeyAgreement,
        KeyUsagePurpose::KeyCertSign,
    ];
    params.extended_key_usages = vec![
        ExtendedKeyUsagePurpose::ClientAuth,
        ExtendedKeyUsagePurpose::ServerAuth,
    ];

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;
    let cert_der = cert.der().clone();
    let key_der = PrivateKeyDer::Pkcs8(key_pair.serialize_der().into());
    Ok((cert_der, key_der))
}

/// A [`ClientCertVerifier`] that accepts exactly one pinned certificate.
///
/// go-plugin's client certificate is self-signed, which a WebPKI chain verifier
/// rejects. Since the certificate is delivered out-of-band (via the environment)
/// this instead requires the presented certificate to match it byte-for-byte,
/// while still verifying the TLS handshake signature with the crypto provider.
#[derive(Debug)]
struct PinnedClientCertVerifier {
    expected: CertificateDer<'static>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ClientCertVerifier for PinnedClientCertVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        if end_entity.as_ref() == self.expected.as_ref() {
            Ok(ClientCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// A TLS-wrapped connection that implements tonic's [`Connected`].
pub(crate) struct TlsConn(TlsStream<TcpStream>);

impl Connected for TlsConn {
    type ConnectInfo = ();
    fn connect_info(&self) -> Self::ConnectInfo {}
}

impl tokio::io::AsyncRead for TlsConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for TlsConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

/// Produce a stream of TLS-accepted connections suitable for
/// [`tonic::transport::Server::serve_with_incoming`].
///
/// Connections whose TLS handshake fails are logged and skipped rather than
/// terminating the stream.
pub(crate) fn tls_incoming(
    listener: TcpListener,
    config: Arc<ServerConfig>,
) -> impl futures_core::Stream<Item = io::Result<TlsConn>> {
    let acceptor = tokio_rustls::TlsAcceptor::from(config);
    async_stream::stream! {
        loop {
            let stream = match listener.accept().await {
                Ok((stream, _peer)) => stream,
                Err(e) => {
                    tracing::warn!(error = %e, "error accepting TCP connection");
                    continue;
                }
            };
            match acceptor.accept(stream).await {
                Ok(tls) => yield Ok(TlsConn(tls)),
                Err(e) => {
                    tracing::warn!(error = %e, "TLS handshake failed");
                    continue;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::client::danger::{ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::ServerName;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio_rustls::{TlsAcceptor, TlsConnector};

    /// Generate a self-signed ECDSA P-521 certificate, mirroring the client
    /// certificate that Grafana's go-plugin host generates for AutoMTLS.
    fn generate_p521_cert() -> (String, CertificateDer<'static>, PrivateKeyDer<'static>) {
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P521_SHA512).unwrap();
        let cert = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
            .unwrap()
            .self_signed(&key)
            .unwrap();
        let pem = cert.pem();
        let der = cert.der().clone();
        let key_der = PrivateKeyDer::Pkcs8(key.serialize_der().into());
        (pem, der, key_der)
    }

    /// Test-only server verifier that accepts any server certificate but still
    /// verifies the handshake signature with the crypto provider.
    #[derive(Debug)]
    struct AcceptAnyServerCert {
        algorithms: WebPkiSupportedAlgorithms,
    }

    impl ServerCertVerifier for AcceptAnyServerCert {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
        }
        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.algorithms.supported_schemes()
        }
    }

    fn client_config(
        cert: CertificateDer<'static>,
        key: PrivateKeyDer<'static>,
    ) -> rustls::ClientConfig {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let algorithms = provider.signature_verification_algorithms;
        let mut config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert { algorithms }))
            .with_client_auth_cert(vec![cert], key)
            .unwrap();
        config.alpn_protocols = vec![b"h2".to_vec()];
        config
    }

    // Drive one mTLS handshake between our server config and a client presenting
    // `client_cert`/`client_key`, returning the negotiated server-side ALPN.
    async fn handshake(
        server_config: Arc<ServerConfig>,
        client_cert: CertificateDer<'static>,
        client_key: PrivateKeyDer<'static>,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let acceptor = TlsAcceptor::from(server_config);

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await?;
            let mut tls = acceptor.accept(tcp).await?;
            let alpn = tls.get_ref().1.alpn_protocol().map(<[u8]>::to_vec);
            let mut buf = [0u8; 5];
            tls.read_exact(&mut buf).await?;
            tls.write_all(b"world").await?;
            tls.flush().await?;
            Ok::<_, std::io::Error>((alpn, buf))
        });

        let connector = TlsConnector::from(Arc::new(client_config(client_cert, client_key)));
        let tcp = TcpStream::connect(addr).await?;
        let mut tls = connector
            .connect(ServerName::try_from("localhost")?, tcp)
            .await?;
        tls.write_all(b"hello").await?;
        tls.flush().await?;
        let mut resp = [0u8; 5];
        tls.read_exact(&mut resp).await?;
        assert_eq!(&resp, b"world");

        let (alpn, got) = server.await??;
        assert_eq!(&got, b"hello");
        Ok(alpn)
    }

    #[tokio::test]
    async fn pinned_p521_client_completes_mtls_handshake() {
        let (client_pem, client_der, client_key) = generate_p521_cert();
        let (server_config, server_cert_b64) = build_server_tls(&client_pem).unwrap();

        // The advertised server certificate must be valid base64 of DER.
        assert!(!server_cert_b64.is_empty());
        base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(&server_cert_b64)
            .expect("server cert is valid base64");

        let alpn = handshake(server_config, client_der, client_key)
            .await
            .expect("mTLS handshake with pinned P-521 client cert should succeed");
        assert_eq!(alpn.as_deref(), Some(b"h2".as_slice()));
    }

    #[tokio::test]
    async fn unpinned_client_is_rejected() {
        // The server pins one client certificate...
        let (pinned_pem, _, _) = generate_p521_cert();
        let (server_config, _) = build_server_tls(&pinned_pem).unwrap();
        // ...but a different client presents another certificate.
        let (_, other_der, other_key) = generate_p521_cert();

        let result = handshake(server_config, other_der, other_key).await;
        assert!(
            result.is_err(),
            "handshake with a non-pinned client certificate must fail"
        );
    }
}
