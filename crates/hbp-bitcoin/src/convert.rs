use bitcoin::Network as BtcNetwork;
use hbp_core::Network;

pub fn to_btc_network(n: Network) -> BtcNetwork {
    match n {
        Network::Bitcoin => BtcNetwork::Bitcoin,
        Network::Testnet => BtcNetwork::Testnet,
        Network::Signet => BtcNetwork::Signet,
        Network::Regtest => BtcNetwork::Regtest,
    }
}
