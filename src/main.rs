use alpenglow_consensus::{
    config::AlpenGlowConfig,
    consensus::PeerPool,
    error::AlpenGlowResult,
    message::{
        MessageProcesser,
        solana_alpenglow_message::{SolanaMessage, SolanaMessagePool},
    },
    network::{QuicClient, QuicServer},
    node::{AlpenGlowNode, solana_alpenglow_node::SolanaNode},
};

fn main() -> AlpenGlowResult<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let config = AlpenGlowConfig::parse();

    let peer_pool = PeerPool::init(&config);

    let (quic_client_handle, msg_sender) = QuicClient::spawn::<SolanaMessage>(&config, &peer_pool);

    let node = SolanaNode::init(&config, &msg_sender)?;

    let (quic_server_handle, msg_receiver) = QuicServer::spawn::<SolanaMessage>(&config);

    let message_pool = SolanaMessagePool::init(&msg_sender, node.pubkey());

    let message_handle = MessageProcesser::spawn_with_receiver::<SolanaMessage>(
        &msg_receiver,
        message_pool,
        &peer_pool,
    );

    let _ = peer_pool.read().map(|p| p.log());

    for h in [quic_client_handle, quic_server_handle, message_handle] {
        h.join().expect("err joining thread");
    }

    Ok(())
}
