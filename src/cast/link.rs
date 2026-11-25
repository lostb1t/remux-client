//! The connection to a Chromecast: TLS on port 8009, carrying length-prefixed [`CastMessage`]s
//! with a JSON payload each.

use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use prost::Message;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring, verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};

use super::proto::CastMessage;

pub const SENDER: &str = "sender-0";
pub const RECEIVER: &str = "receiver-0";
pub const NS_CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
pub const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
pub const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
pub const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";

/// The device proves itself with a certificate that is its own, not a known authority's: there is
/// nothing to check it against, and nothing secret goes over this link.
#[derive(Debug)]
struct AcceptAny(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

pub struct Link {
    stream: StreamOwned<ClientConnection, TcpStream>,
    /// What came in and isn't a whole message yet.
    buffer: Vec<u8>,
}

impl Link {
    pub fn connect(host: IpAddr, port: u16) -> io::Result<Self> {
        let provider = Arc::new(ring::default_provider());
        let config = ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(io::Error::other)?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAny(provider)))
            .with_no_client_auth();
        let connection = ClientConnection::new(
            Arc::new(config),
            ServerName::IpAddress(host.into()),
        )
        .map_err(io::Error::other)?;
        let tcp = TcpStream::connect_timeout(&SocketAddr::new(host, port), Duration::from_secs(5))?;
        tcp.set_nodelay(true)?;
        // The handshake happens on the first read or write; reading wakes up now and then so that
        // what has to be sent (and the heartbeat) isn't kept waiting.
        tcp.set_read_timeout(Some(Duration::from_millis(250)))?;
        tcp.set_write_timeout(Some(Duration::from_secs(5)))?;
        Ok(Self { stream: StreamOwned::new(connection, tcp), buffer: Vec::new() })
    }

    pub fn send(&mut self, namespace: &str, destination: &str, payload: &serde_json::Value) -> io::Result<()> {
        let message = CastMessage {
            protocol_version: Some(0),
            source_id: Some(SENDER.to_string()),
            destination_id: Some(destination.to_string()),
            namespace: Some(namespace.to_string()),
            payload_type: Some(0),
            payload_utf8: Some(payload.to_string()),
        };
        let body = message.encode_to_vec();
        let mut frame = Vec::with_capacity(body.len() + 4);
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(&body);
        self.stream.write_all(&frame)?;
        self.stream.flush()
    }

    /// The next message, or `None` when nothing came in during the short wait.
    pub fn receive(&mut self) -> io::Result<Option<(String, String, serde_json::Value)>> {
        loop {
            if let Some(message) = self.take()? {
                return Ok(Some(message));
            }
            let mut chunk = [0u8; 8192];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the device closed the connection")),
                Ok(n) => self.buffer.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                    return Ok(None)
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// A whole message out of the buffer, if it holds one.
    fn take(&mut self) -> io::Result<Option<(String, String, serde_json::Value)>> {
        if self.buffer.len() < 4 {
            return Ok(None);
        }
        let length = u32::from_be_bytes([self.buffer[0], self.buffer[1], self.buffer[2], self.buffer[3]]) as usize;
        if length > 4 * 1024 * 1024 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "a message that is too large"));
        }
        if self.buffer.len() < 4 + length {
            return Ok(None);
        }
        let message = CastMessage::decode(&self.buffer[4..4 + length])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        self.buffer.drain(..4 + length);
        let payload = message
            .payload_utf8
            .as_deref()
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or(serde_json::Value::Null);
        Ok(Some((
            message.namespace.unwrap_or_default(),
            message.source_id.unwrap_or_default(),
            payload,
        )))
    }
}
