use alpenglow_consensus::{
    config::AlpenGlowConfig,
    error::AlpenGlowResult,
    message::{
        MessageProcesser,
        solana_alpenglow_message::{SolanaMessage, SolanaMessagePool},
    },
    network::{QuicClient, QuicServer},
    node::{self, AlpenGlowNode, solana_alpenglow_node::SolanaNode},
};

fn main() -> AlpenGlowResult<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let config = AlpenGlowConfig::parse();

    let (quic_client_handle, msg_sender) = QuicClient::spawn::<SolanaMessage>(&config);

    let node = SolanaNode::init(&config, &msg_sender)?;

    let (quic_server_handle, msg_receiver) = QuicServer::spawn(&config);

    let message_pool = SolanaMessagePool::init(&msg_sender, node.pubkey());

    let message_handle =
        MessageProcesser::spawn_with_receiver::<SolanaMessage>(&msg_receiver, message_pool);

    for h in [quic_client_handle, quic_server_handle, message_handle] {
        h.join().expect("err joining thread");
    }

    Ok(())
}
