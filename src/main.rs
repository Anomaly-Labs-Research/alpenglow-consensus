use alpenglow_consensus::{
    config::AlpenGlowConfig, error::AlpenGlowResult, message::MessageProcesser,
    network::QuicServer, node::Node,
};

fn main() -> AlpenGlowResult<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let config = AlpenGlowConfig::parse();

    let node = Node::init(&config)?;

    node.log();

    let (rx, quic_handle) = QuicServer::spawn(config)?;

    let message_handle = MessageProcesser::spawn_with_receiver(rx);

    for h in [quic_handle, message_handle] {
        h.join().expect("err joining thread");
    }

    Ok(())
}
