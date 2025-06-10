use std::{
    collections::HashMap,
    net::{SocketAddr, SocketAddrV4},
    str::FromStr,
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    config::AlpenGlowConfig,
    error::{AlpenGlowError, AlpenGlowResult},
    message::AlpenGlowMessage,
};
use crossbeam::channel::{self, Receiver, Sender};
use quinn::{
    ClientConfig, Endpoint, ServerConfig, VarInt,
    crypto::rustls::QuicClientConfig,
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime, pem::PemObject},
    },
};
use tokio::task;
use tracing::{debug, error, info};

pub const CLOSE_CONN_MESSAGE: &[u8] = b"CLOSE_CONN";

pub const CLOSE_CONN_CODE: VarInt = VarInt::from_u32(0);

pub const MAX_QUIC_MESSAGE_BYTES: u64 = 1024 * 100;

pub struct QuicServer;

impl QuicServer {
    pub fn spawn(config: &Arc<AlpenGlowConfig>) -> (JoinHandle<()>, Receiver<Vec<u8>>) {
        let (tx, rx) = crossbeam::channel::unbounded::<Vec<u8>>();
        let config = Arc::clone(&config);
        let handle = thread::spawn(|| {
            info!("spawn : quicServerProcessorThread");
            match tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .map_err(|_| AlpenGlowError::InvalidQuicConfig)
                .unwrap()
                .block_on(Self::spawn_inner(config, tx))
            {
                Ok(_) => {}
                Err(e) => error!("err {:?}", e),
            }
        });
        (handle, rx)
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

        let mut join_handles = Vec::new();
        while let Some(connecting) = server.accept().await {
            let tx = tx.clone();
            let handle = tokio::spawn(async move {
                match connecting.await {
                    Ok(connection) => {
                        info!("incoming conn from {}", connection.remote_address());
                        loop {
                            match connection.accept_uni().await {
                                Ok(mut recv) => {
                                    match recv.read_to_end(MAX_QUIC_MESSAGE_BYTES as usize).await {
                                        Ok(d) => match tx.send(d) {
                                            Ok(_) => {}
                                            Err(e) => error!("chan : send_err {}", e),
                                        },
                                        Err(e) => error!("err reading bytes {}", e),
                                    }
                                }
                                Err(e) => {
                                    connection.close(CLOSE_CONN_CODE, CLOSE_CONN_MESSAGE);
                                    debug!("Connection : {}", e.to_string());
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => error!("connection - {}", e),
                }
            });

            join_handles.push(handle);
        }

        for i in join_handles {
            let _ = i.await;
        }

        Ok(())
    }
}

pub struct QuicClient;

impl QuicClient {
    pub fn spawn<T: AlpenGlowMessage + 'static>(
        config: &Arc<AlpenGlowConfig>,
    ) -> (JoinHandle<()>, Sender<Vec<T>>) {
        let config = Arc::clone(&config);
        let (tx, rx) = channel::unbounded::<Vec<T>>();
        let handle = thread::spawn(|| {
            info!("spawn : quicClientProcessorThread");
            match tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .map_err(|_| AlpenGlowError::InvalidQuicConfig)
                .unwrap()
                .block_on(Self::spawn_inner(config, rx))
            {
                Ok(_) => {}
                Err(e) => error!("err {:?}", e),
            }
        });
        (handle, tx)
    }

    pub async fn spawn_inner<T: AlpenGlowMessage + 'static>(
        config: Arc<AlpenGlowConfig>,
        msg_receiver: Receiver<Vec<T>>,
    ) -> AlpenGlowResult<()> {
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

        let endpoint = Arc::new(client_endpoint);

        while let Ok(msgs) = msg_receiver.recv() {
            let mut msgs_receiver_map: HashMap<SocketAddrV4, Vec<T>> = HashMap::new();

            for m in msgs {
                let receiver = m.receiver();
                if let Some(buf) = msgs_receiver_map.get_mut(receiver) {
                    buf.push(m);
                } else {
                    msgs_receiver_map.insert(*receiver, vec![m]);
                }
            }

            let mut handles = Vec::new();
            for (receiver, msgs) in msgs_receiver_map {
                let endpoint = Arc::clone(&endpoint);
                let handle = task::spawn(async move {
                    let msgs_bytes = msgs.into_iter().flat_map(|m| m.pack()).collect::<Vec<_>>();

                    match Self::send_msgs(&endpoint, msgs_bytes, receiver).await {
                        Ok(_) => {}
                        Err(e) => error!("err sending messages {:?}", e),
                    }
                });

                handles.push(handle);
            }

            for h in handles {
                let _ = h.await;
            }
        }

        Ok(())
    }

    pub async fn send_msgs(
        client: &Endpoint,
        data: Vec<u8>,
        receiver: SocketAddrV4,
    ) -> AlpenGlowResult<()> {
        // connect to server
        let connection = client
            .connect(SocketAddr::V4(receiver), "localhost")
            .unwrap()
            .await
            .unwrap();

        info!("connected -> {}", connection.remote_address());

        if let Ok(mut send) = connection.open_uni().await {
            match send.write(&data.as_slice()).await {
                Ok(len) => info!("Sent {} bytes Successfully", len),
                Err(e) => error!("error sending {}", e),
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;

        Ok(())
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
