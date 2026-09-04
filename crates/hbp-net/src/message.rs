use hbp_core::{Offer, Quote, SignedContract};
use serde::{Deserialize, Serialize};

/// File-passing remains the fallback / dev channel. Same JSON, different pipe.
pub const FILE_FALLBACK: &str = "file";

/// Wire messages. Payloads reuse `hbp-core` (and opaque hex/JSON for bitcoin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NetMessage {
    Ping {
        work_name: String,
    },
    Pong {
        work_name: String,
        onion: Option<String>,
    },
    Offer {
        offer: Offer,
    },
    Accept {
        pending: SignedContract,
    },
    Commit {
        signed: SignedContract,
    },
    Quote {
        quote: Quote,
    },
    /// Coop / fee-burn / coin files: already-canonical JSON from hbp-bitcoin/CLI.
    Artifact {
        name: String,
        json: serde_json::Value,
    },
}

impl NetMessage {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Ping { .. } => "ping",
            Self::Pong { .. } => "pong",
            Self::Offer { .. } => "offer",
            Self::Accept { .. } => "accept",
            Self::Commit { .. } => "commit",
            Self::Quote { .. } => "quote",
            Self::Artifact { .. } => "artifact",
        }
    }

    pub fn encode(&self) -> crate::Result<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }

    pub fn decode(bytes: &[u8]) -> crate::Result<Self> {
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hbp_core::{ContractBody, DisputePolicy, Network, PartidaSpec, Role, Unit};

    fn pk(n: u8) -> String {
        let mut b = [0x02u8; 33];
        b[32] = n;
        hex::encode(b)
    }

    #[test]
    fn offer_message_roundtrip_shares_core_types() {
        let offer = Offer {
            body: ContractBody {
                network: Network::Signet,
                unit: Unit::Usd,
                work_name: "Casa".into(),
                bond_bps: 1000,
                t_project: 1_800_000_000,
                partidas: vec![PartidaSpec {
                    id: 1,
                    description: "Radier".into(),
                    amount_minor: 100_000,
                    plazo_unix: 1_700_000_000,
                }],
                mandante_pubkey: pk(1),
                contratista_pubkey: None,
                dispute: DisputePolicy::fee_burn(1_700_000_000, 1_800_000_000),
            },
            mandante_sig: "aa".into(),
        };
        let msg = NetMessage::Offer { offer: offer.clone() };
        let back = NetMessage::decode(&msg.encode().unwrap()).unwrap();
        assert_eq!(back, msg);
        assert_eq!(back.kind(), "offer");
        let _ = Role::Mandante;
    }
}
