use std::{
    collections::HashMap,
    marker::PhantomData,
    mem,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    str::FromStr,
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    config::AlpenGlowConfig,
    consensus::PeerPoolSync,
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

pub fn pack_socket_addr(addr: &SocketAddr) -> Vec<u8> {
    match addr {
        SocketAddr::V4(addr) => pack_socket_addr_v4(addr),
        SocketAddr::V6(addr) => pack_socket_addr_v6(addr),
    }
}

pub fn pack_socket_addr_v4(addr: &SocketAddrV4) -> Vec<u8> {
    let ip = addr.ip().octets();
    let port: [u8; 2] = addr.port().to_le_bytes().try_into().expect("err ip");

    let mut buffer = Vec::new();

    buffer.extend_from_slice(&ip);
    buffer.extend_from_slice(&port);

    buffer
}

pub fn pack_socket_addr_v6(addr: &SocketAddrV6) -> Vec<u8> {
    let ip = addr.ip().octets();
    let port: [u8; 2] = addr.port().to_le_bytes().try_into().expect("err ip");

    let mut buffer = Vec::new();

    buffer.extend_from_slice(&ip);
    buffer.extend_from_slice(&port);

    buffer
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum SockertAddrType {
    Ipv4,
    Ipv6,
}

impl SockertAddrType {
    fn from_u8(data: u8) -> Self {
        match data {
            0 => Self::Ipv4,
            1 => Self::Ipv6,
            _ => panic!("invalid socket addr type"),
        }
    }
}

#[derive(Debug)]
pub struct AlpenGlowNetworkMessage<T: AlpenGlowMessage> {
    pub socket_addr_type: SockertAddrType,
    pub sender: SocketAddr,
    pub receiver: SocketAddr,
    pub data: Vec<u8>,
    _t: PhantomData<T>,
}

impl<T: AlpenGlowMessage> AlpenGlowNetworkMessage<T> {
    pub fn get_metadata_len(byte: u8) -> usize {
        match SockertAddrType::from_u8(byte) {
            SockertAddrType::Ipv4 => 1 + 2 * mem::size_of::<SocketAddrV4>(),
            SockertAddrType::Ipv6 => 1 + 2 * mem::size_of::<SocketAddrV6>(),
        }
    }

    pub fn get_payload_len(bytes: &[u8]) -> AlpenGlowResult<u16> {
        Ok(u16::from_le_bytes(
            bytes[0..2]
                .try_into()
                .map_err(|_| AlpenGlowError::InvalidMessage)?,
        ))
    }

    pub fn socket_addr_type(&self) -> u8 {
        self.socket_addr_type as u8
    }

    pub fn sender(&self) -> &SocketAddr {
        &self.sender
    }

    pub fn receiver(&self) -> &SocketAddr {
        &self.receiver
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut buffer = Vec::new();

        buffer.push(self.socket_addr_type());

        buffer.extend_from_slice(&pack_socket_addr(&self.sender));

        buffer.extend_from_slice(&pack_socket_addr(&self.receiver));

        buffer.extend_from_slice(&self.data);

        buffer
    }

    pub fn unpack(bytes: &[u8]) -> AlpenGlowResult<Self> {
        let socket_addr_type = SockertAddrType::from_u8(bytes[0]);
        let (sender, receiver, data_start_index) = match socket_addr_type {
            SockertAddrType::Ipv4 => {
                let sender = {
                    let ip = (bytes[1], bytes[2], bytes[3], bytes[4]);
                    let port = u16::from_le_bytes(
                        bytes[5..7]
                            .try_into()
                            .map_err(|_| AlpenGlowError::InvalidMessage)?,
                    );

                    SocketAddrV4::new(Ipv4Addr::new(ip.0, ip.1, ip.2, ip.3), port)
                };

                let receiver = {
                    let ip = (bytes[7], bytes[8], bytes[9], bytes[10]);
                    let port = u16::from_le_bytes(
                        bytes[11..13]
                            .try_into()
                            .map_err(|_| AlpenGlowError::InvalidMessage)?,
                    );

                    SocketAddrV4::new(Ipv4Addr::new(ip.0, ip.1, ip.2, ip.3), port)
                };

                (SocketAddr::from(sender), SocketAddr::from(receiver), 13)
            }
            // SockertAddrType::Ipv6 => {
            //     let sender = {
            //         let ip = (
            //             bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
            //             bytes[7],
            //         );
            //         let port = u16::from_le_bytes(
            //             bytes[8..10]
            //                 .try_into()
            //                 .map_err(|_| AlpenGlowError::InvalidMessage)?,
            //         );

            //         SocketAddrV6::new(
            //             Ipv6Addr::new(ip.0, ip.1, ip.2, ip.3, ip.4, ip.5, ip.6, ip.7),
            //             port,
            //         )
            //     };

            //     let receiver = {
            //         let ip = (bytes[6], bytes[7], bytes[8], bytes[9]);
            //         let port = u16::from_le_bytes(
            //             bytes[10..12]
            //                 .try_into()
            //                 .map_err(|_| AlpenGlowError::InvalidMessage)?,
            //         );

            //         SocketAddrV4::new(Ipv4Addr::new(ip.0, ip.1, ip.2, ip.3), port)
            //     };
            // }
            _ => panic!("not impl"),
        };

        Ok(Self {
            socket_addr_type,
            sender: sender,
            receiver: receiver,
            data: bytes[data_start_index..].to_vec(),
            _t: PhantomData,
        })
    }

    pub fn strip_network_meta(bytes: Vec<u8>) -> AlpenGlowResult<Vec<u8>> {
        let mut msg_buffer = Vec::new();

        if bytes.len() as u64 > MAX_QUIC_MESSAGE_BYTES {
            return Err(AlpenGlowError::InvalidMessage);
        }

        let end = bytes.len();
        let mut cur = 0;

        while cur < end {
            let metadata_len = AlpenGlowNetworkMessage::<T>::get_metadata_len(bytes[0]);

            let payload_data_len_start = metadata_len + 2 * mem::size_of::<T::Address>();

            let read_end_index = cur
                + metadata_len
                + T::MESSAGE_META_LENGTH
                + AlpenGlowNetworkMessage::<T>::get_payload_len(
                    &bytes[(payload_data_len_start)..(payload_data_len_start + 2)],
                )? as usize;

            match Self::unpack(&bytes[cur..read_end_index]) {
                Ok(m) => {
                    msg_buffer.extend_from_slice(&m.data);
                }
                Err(e) => error!("error unpacking {}", e),
            }

            cur = read_end_index;
        }

        Ok(msg_buffer)
    }
}

impl<T: AlpenGlowMessage> AlpenGlowNetworkMessage<T> {
    fn from_message_with_config(
        msg: &T,
        config: &Arc<AlpenGlowConfig>,
        peer_pool: &PeerPoolSync<T>,
    ) -> AlpenGlowResult<Self> {
        let sender = SocketAddr::from_str(config.client_addr_with_port().as_str())
            .map_err(|_| AlpenGlowError::InvalidNetworkConfig)?;

        let receiver = peer_pool
            .read()
            .map(|p| p.get_peer_ip(msg.receiver()))
            .ok()
            .flatten();

        match receiver {
            Some(r) => Ok(Self {
                socket_addr_type: SockertAddrType::Ipv4,
                sender,
                receiver: r,
                data: msg.pack(),
                _t: PhantomData,
            }),
            None => Err(AlpenGlowError::InvalidNetworkConfig),
        }
    }
}

pub struct QuicServer;

impl QuicServer {
    pub fn spawn<T: AlpenGlowMessage>(
        config: &Arc<AlpenGlowConfig>,
    ) -> (JoinHandle<()>, Receiver<Vec<u8>>) {
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
                .block_on(Self::spawn_inner::<T>(config, tx))
            {
                Ok(_) => {}
                Err(e) => error!("err {:?}", e),
            }
        });
        (handle, rx)
    }

    pub async fn spawn_inner<T: AlpenGlowMessage>(
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
                                        Ok(d) => {
                                            if let Ok(msg_data) =
                                                AlpenGlowNetworkMessage::<T>::strip_network_meta(d)
                                            {
                                                match tx.send(msg_data) {
                                                    Ok(_) => {}
                                                    Err(e) => error!("chan : send_err {}", e),
                                                }
                                            }
                                        }
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
        peer_pool: &PeerPoolSync<T>,
    ) -> (JoinHandle<()>, Sender<Vec<T>>) {
        let config = Arc::clone(&config);
        let (tx, rx) = channel::unbounded::<Vec<T>>();
        let peer_pool = Arc::clone(&peer_pool);
        let handle = thread::spawn(|| {
            info!("spawn : quicClientProcessorThread");
            match tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .map_err(|_| AlpenGlowError::InvalidQuicConfig)
                .unwrap()
                .block_on(Self::spawn_inner(config, peer_pool, rx))
            {
                Ok(_) => {}
                Err(e) => error!("err {:?}", e),
            }
        });
        (handle, tx)
    }

    pub async fn spawn_inner<T: AlpenGlowMessage + 'static>(
        config: Arc<AlpenGlowConfig>,
        peer_pool: PeerPoolSync<T>,
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
            let mut msgs_receiver_map: HashMap<SocketAddr, Vec<AlpenGlowNetworkMessage<T>>> =
                HashMap::new();

            for m in msgs {
                if let Ok(network_msg) =
                    AlpenGlowNetworkMessage::<T>::from_message_with_config(&m, &config, &peer_pool)
                {
                    let receiver = network_msg.receiver();
                    if let Some(buf) = msgs_receiver_map.get_mut(receiver) {
                        buf.push(network_msg);
                    } else {
                        msgs_receiver_map.insert(*receiver, vec![network_msg]);
                    }
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
        receiver: SocketAddr,
    ) -> AlpenGlowResult<()> {
        // connect to server
        let connection = client
            .connect(receiver, "localhost")
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
