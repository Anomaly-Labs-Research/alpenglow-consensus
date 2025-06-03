use std::{
    net::SocketAddr,
    str::FromStr,
    sync::Arc,
    thread::{self, JoinHandle},
};

use crate::{
    config::AlpenGlowConfig,
    error::{AlpenGlowError, AlpenGlowResult},
    message::{MAX_MESSAGE_LEN_BYTES, TxMessage},
};
use crossbeam::channel::{Receiver, Sender};
use quinn::{
    ClientConfig, Endpoint, ServerConfig, VarInt,
    crypto::rustls::QuicClientConfig,
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime, pem::PemObject},
    },
};
use tracing::{error, info};

pub const CLOSE_CONN_MESSAGE: &[u8] = b"CLOSE_CONN";

pub const CLOSE_CONN_CODE: VarInt = VarInt::from_u32(0);

pub struct QuicServer;

impl QuicServer {
    pub fn spawn(
        config: Arc<AlpenGlowConfig>,
    ) -> AlpenGlowResult<(Receiver<Vec<u8>>, JoinHandle<()>)> {
        let (tx, rx) = crossbeam::channel::unbounded::<Vec<u8>>();
        let config = Arc::clone(&config);
        let handle = thread::spawn(|| {
            info!("spawn : quicServerThread");
            match tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .map_err(|_| AlpenGlowError::InvalidQuicConfig)
                .unwrap()
                .block_on(Self::spawn_inner(config, tx))
            {
                Ok(_) => {}
                Err(e) => println!("err {:?}", e),
            }
        });
        Ok((rx, handle))
    }

    pub async fn spawn_inner(
        config: Arc<AlpenGlowConfig>,
        tx: Sender<Vec<u8>>,
    ) -> AlpenGlowResult<()> {
        // Load certificate chain from PEM file, ignoring any malformed certs
        let (certs, key) = (
            CertificateDer::pem_file_iter(config.rsa_cert_path())
                .expect("Failed to read certificate PEM file")
                .collect::<Vec<_>>() // Vec<Result<CertificateDer, _>>
                .into_iter()
                .filter_map(|c| c.ok()) // Drop invalid certs silently
                .collect::<Vec<_>>(), // Final Vec<CertificateDer>
            // Load private key from PEM file
            PrivateKeyDer::from_pem_file(config.rsa_key_path())
                .expect("Failed to read private key PEM file"),
        );

        // Construct QUIC server config using the certs and private key
        let server_config = ServerConfig::with_single_cert(certs, key)
            .expect("Invalid certificate or private key for QUIC server");

        // Build server endpoint at configured address
        let server = Endpoint::server(
            server_config,
            SocketAddr::from_str(&config.server_addr_with_port())
                .expect("Invalid server address format"),
        )
        .expect("Failed to initialize QUIC server endpoint");

        info!("quick server running at {}", config.server_addr_with_port());

        while let Some(connecting) = server.accept().await {
            let tx = tx.clone();
            tokio::spawn(async move {
                let connection = connecting.await.unwrap();
                info!("incoming conn from {}", connection.remote_address());
                loop {
                    match connection.accept_uni().await {
                        Ok(mut recv) => {
                            match recv.read_to_end(MAX_MESSAGE_LEN_BYTES as usize).await {
                                Ok(d) => match tx.send(d) {
                                    Ok(_) => {}
                                    Err(e) => error!("chan : send_err {}", e),
                                },
                                Err(e) => error!("err reading bytes {}", e),
                            }
                        }
                        Err(e) => {
                            connection.close(CLOSE_CONN_CODE, CLOSE_CONN_MESSAGE);
                            error!("closing conn due to {}", e.to_string());
                            break;
                        }
                    }
                }
            });
        }

        Ok(())
    }
}

pub struct QuicClient(pub Endpoint);

impl QuicClient {
    pub async fn build(config: &AlpenGlowConfig) -> Self {
        // Build client endpoint at configured address
        let mut client_endpoint = Endpoint::client(
            SocketAddr::from_str(&config.client_addr_with_port())
                .expect("Invalid client address format"),
        )
        .expect("Failed to initialize QUIC client endpoint");

        // Set a custom client config with certificate verification disabled (for dev/testing)
        let rustls_config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_no_client_auth();

        let quic_client_config = QuicClientConfig::try_from(rustls_config)
            .expect("Failed to convert rustls config into QuicClientConfig");

        client_endpoint.set_default_client_config(ClientConfig::new(Arc::new(quic_client_config)));

        Self(client_endpoint)
    }

    pub fn client(&self) -> &Endpoint {
        &self.0
    }

    pub async fn send_data(
        &self,
        data: &[impl TxMessage],
        receiver: SocketAddr,
    ) -> AlpenGlowResult<()> {
        let bytes = data.into_iter().flat_map(|d| d.pack()).collect::<Vec<u8>>();

        // connect to server
        let connection = self
            .client()
            .connect(receiver, "localhost")
            .unwrap()
            .await
            .unwrap();

        println!("connected -> {}", connection.remote_address());

        if let Ok(mut send) = connection.open_uni().await {
            println!(
                "data {}",
                String::from_utf8(bytes.to_vec()).unwrap_or("".to_string())
            );
            match send.write(bytes.as_slice()).await {
                Ok(len) => println!("Sent {} bytes Successfully", len),
                Err(e) => println!("error sending {}", e),
            }
        }

        Ok(())
    }

    pub async fn close(&self) {
        self.0.close(VarInt::from_u32(0), CLOSE_CONN_MESSAGE);
    }
}

#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self(Arc::new(rustls::crypto::ring::default_provider())))
    }
}

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, str::FromStr};

    use crate::{config::AlpenGlowConfig, message::AlpenGlowMessage, network::QuicClient};

    // need to spawn the server in a sep thread
    #[tokio::test]
    async fn test_quic() {
        // Spawn Quic server
        let config = AlpenGlowConfig::parse();

        let client = QuicClient::build(&config).await;

        let input = "hello";

        client
            .send_data(
                &[AlpenGlowMessage::from_str(&input)],
                SocketAddr::from_str(&config.server_addr_with_port()).unwrap(),
            )
            .await
            .unwrap();
    }
}
