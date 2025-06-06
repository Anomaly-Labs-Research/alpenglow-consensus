use alpenglow_consensus::{
    config::AlpenGlowConfig,
    error::AlpenGlowResult,
    message::{
        MessageProcesser,
        solana_alpenglow_message::{SolanaMessage, SolanaMessagePool},
    },
    network::QuicServer,
    node::solana_alpenglow_node::SolanaNode,
};

fn main() -> AlpenGlowResult<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let config = AlpenGlowConfig::parse();

    println!("config {:?}", config);

    let (_node, _msgs_receiver) = SolanaNode::init(&config)?;

    // let quic_client_handle = QuicClient::spawn::<SolanaMessage>(&config, msgs_receiver);

    let (quic_server_handle, rx) = QuicServer::spawn(&config)?;

    let message_pool = SolanaMessagePool::init();

    let message_handle = MessageProcesser::spawn_with_receiver::<SolanaMessage>(rx, message_pool);

    for h in [quic_server_handle, message_handle] {
        h.join().expect("err joining thread");
    }

    Ok(())
}
