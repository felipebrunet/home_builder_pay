//! Local watch-only of a hot wallet (Blue/Sparrow). Never send this to the peer.
//!
//! The counterpart only ever sees one offered coin: outpoint, amount, address, change.

use std::str::FromStr;

use bitcoin::bip32::Xpub;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::{Address, NetworkKind, OutPoint, ScriptBuf, Transaction};
use hbp_core::{Network, Role};
use miniscript::Descriptor;
use miniscript::DescriptorPublicKey;
use serde::{Deserialize, Serialize};

use crate::fund::FundingCoin;
use crate::taproot::to_btc_network;
use crate::Error;

const DEFAULT_GAP: u32 = 20;

/// How the watched account encodes its addresses (Blue: Native SegWit or Taproot).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WatchKind {
    Wpkh,
    Tr,
}

impl FromStr for WatchKind {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        match s.to_ascii_lowercase().as_str() {
            "wpkh" | "bip84" | "native" => Ok(Self::Wpkh),
            "tr" | "bip86" | "taproot" => Ok(Self::Tr),
            other => Err(Error::msg(format!(
                "watch kind {other}: use wpkh (Blue Native SegWit) or tr (Taproot)"
            ))),
        }
    }
}

/// Stored only under `--dir` (watch.json). Not part of the file protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchAccount {
    pub network: Network,
    pub kind: WatchKind,
    pub receive_descriptor: String,
    pub change_descriptor: String,
    #[serde(default = "default_gap")]
    pub gap_limit: u32,
}

fn default_gap() -> u32 {
    DEFAULT_GAP
}

/// One UTXO belonging to the local watch-only account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchedUtxo {
    pub outpoint: String,
    pub sats: u64,
    pub address: String,
    pub confirmed: bool,
    pub chain: String,
    pub index: u32,
}

/// Scan result: coins plus a fresh change address (first unused on the change chain).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchScan {
    pub change: String,
    pub utxos: Vec<WatchedUtxo>,
}

/// What the other party is allowed to see for *this* funding tx. No xpub, no descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfferedCoin {
    pub role: Role,
    pub outpoint: String,
    pub sats: u64,
    pub address: String,
    pub change: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_tx_hex: Option<String>,
}

impl OfferedCoin {
    pub fn outpoint(&self) -> Result<OutPoint, Error> {
        OutPoint::from_str(&self.outpoint).map_err(|e| Error::msg(e.to_string()))
    }

    pub fn funding_coin(&self, network: Network) -> Result<FundingCoin, Error> {
        let addr = parse_addr(&self.address, network)?;
        Ok(FundingCoin {
            outpoint: self.outpoint()?,
            sats: self.sats,
            script_pubkey: addr.script_pubkey(),
        })
    }

    pub fn change_address(&self, network: Network) -> Result<Address, Error> {
        parse_addr(&self.change, network)
    }

    pub fn prev_tx(&self) -> Result<Option<Transaction>, Error> {
        match &self.prev_tx_hex {
            None => Ok(None),
            Some(h) => {
                let raw = hex::decode(h.trim()).map_err(|e| Error::msg(e.to_string()))?;
                bitcoin::consensus::deserialize(&raw)
                    .map(Some)
                    .map_err(|e| Error::msg(e.to_string()))
            }
        }
    }
}

fn parse_addr(s: &str, network: Network) -> Result<Address, Error> {
    Address::from_str(s)
        .map_err(|e| Error::Address(e.to_string()))?
        .require_network(to_btc_network(network))
        .map_err(|e| Error::Address(e.to_string()))
}

/// Import a Blue/Sparrow xpub, zpub/vpub, or a ranged descriptor. Local only.
pub fn import_watch(
    raw: &str,
    kind: Option<WatchKind>,
    network: Network,
    gap_limit: u32,
) -> Result<WatchAccount, Error> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(Error::msg("empty watch import"));
    }
    let (receive, inferred) = if raw.contains('(') {
        let desc = normalize_descriptor(raw)?;
        let inferred = kind_from_descriptor(&desc)?;
        (desc, Some(inferred))
    } else {
        let (xpub, from_slip) = slip132_to_xpub(raw)?;
        check_xpub_network(&xpub, network)?;
        let kind = kind.or(from_slip).unwrap_or(WatchKind::Wpkh);
        (wrap_xpub(&xpub, kind, 0)?, Some(kind))
    };
    let kind = kind.or(inferred).unwrap_or(WatchKind::Wpkh);
    let change = change_descriptor(&receive)?;
    // Sanity: derive index 0 on this network.
    let _ = address_at(&receive, 0, network)?;
    let _ = address_at(&change, 0, network)?;
    Ok(WatchAccount {
        network,
        kind,
        receive_descriptor: receive,
        change_descriptor: change,
        gap_limit: if gap_limit == 0 {
            DEFAULT_GAP
        } else {
            gap_limit
        },
    })
}

pub fn address_at(desc_str: &str, index: u32, network: Network) -> Result<Address, Error> {
    let secp = Secp256k1::verification_only();
    let desc = Descriptor::<DescriptorPublicKey>::from_str(desc_str)
        .map_err(|e| Error::Miniscript(e.to_string()))?;
    let derived = desc
        .derived_descriptor(&secp, index)
        .map_err(|e| Error::Miniscript(e.to_string()))?;
    derived
        .address(to_btc_network(network))
        .map_err(|e| Error::Address(e.to_string()))
}

pub fn script_at(desc_str: &str, index: u32) -> Result<ScriptBuf, Error> {
    let secp = Secp256k1::verification_only();
    let desc = Descriptor::<DescriptorPublicKey>::from_str(desc_str)
        .map_err(|e| Error::Miniscript(e.to_string()))?;
    let derived = desc
        .derived_descriptor(&secp, index)
        .map_err(|e| Error::Miniscript(e.to_string()))?;
    Ok(derived.script_pubkey())
}

/// Walk receive + change until `gap_limit` unused addresses on each chain.
///
/// `lookup` returns UTXOs for one address: (outpoint, sats, confirmed).
pub fn scan_watch<F>(account: &WatchAccount, mut lookup: F) -> Result<WatchScan, Error>
where
    F: FnMut(&Address) -> Result<Vec<(OutPoint, u64, bool)>, Error>,
{
    let recv = scan_chain(
        &account.receive_descriptor,
        account.network,
        account.gap_limit,
        "receive",
        &mut lookup,
    )?;
    let chg = scan_chain(
        &account.change_descriptor,
        account.network,
        account.gap_limit,
        "change",
        &mut lookup,
    )?;
    let change = address_at(&account.change_descriptor, chg.unused, account.network)?;
    let mut utxos = recv.utxos;
    utxos.extend(chg.utxos);
    Ok(WatchScan {
        change: change.to_string(),
        utxos,
    })
}

struct ChainScan {
    utxos: Vec<WatchedUtxo>,
    unused: u32,
}

fn scan_chain<F>(
    desc: &str,
    network: Network,
    gap: u32,
    chain: &'static str,
    lookup: &mut F,
) -> Result<ChainScan, Error>
where
    F: FnMut(&Address) -> Result<Vec<(OutPoint, u64, bool)>, Error>,
{
    let mut utxos = Vec::new();
    let mut unused_streak = 0u32;
    let mut first_unused = 0u32;
    let mut found_any = false;
    let mut i = 0u32;
    while unused_streak < gap {
        let addr = address_at(desc, i, network)?;
        let found = lookup(&addr)?;
        if found.is_empty() {
            if !found_any {
                first_unused = i;
            }
            unused_streak += 1;
        } else {
            unused_streak = 0;
            found_any = true;
            first_unused = i + 1;
            for (op, sats, confirmed) in found {
                utxos.push(WatchedUtxo {
                    outpoint: op.to_string(),
                    sats,
                    address: addr.to_string(),
                    confirmed,
                    chain: chain.to_string(),
                    index: i,
                });
            }
        }
        i += 1;
        if i > 10_000 {
            return Err(Error::msg("watch scan exceeded 10000 addresses"));
        }
    }
    if !found_any {
        first_unused = 0;
    }
    Ok(ChainScan {
        utxos,
        unused: first_unused,
    })
}

/// Convert SLIP-132 zpub/vpub to BIP32 xpub/tpub. Returns inferred kind for z/v.
pub fn slip132_to_xpub(s: &str) -> Result<(String, Option<WatchKind>), Error> {
    let data = bitcoin::base58::decode_check(s.trim()).map_err(|e| Error::msg(e.to_string()))?;
    if data.len() != 78 {
        return Err(Error::msg(format!(
            "extended key payload {} bytes (want 78)",
            data.len()
        )));
    }
    let ver = u32::from_be_bytes(data[0..4].try_into().unwrap());
    let (new_ver, kind): (u32, Option<WatchKind>) = match ver {
        0x0488_B21E | 0x0435_87CF => return Ok((s.trim().to_string(), None)), // xpub / tpub
        0x04B2_4746 => (0x0488_B21E, Some(WatchKind::Wpkh)),                  // zpub
        0x045F_1CF6 => (0x0435_87CF, Some(WatchKind::Wpkh)),                  // vpub
        0x049D_7CB2 | 0x044A_5262 => {
            return Err(Error::msg(
                "ypub/upub (nested SegWit) not supported; export Native SegWit (zpub/vpub) or Taproot from Blue",
            ));
        }
        0x0488_ADE4 | 0x0435_8394 => {
            return Err(Error::msg("xprv/tprv is a secret; import the *xpub* only"));
        }
        other => {
            return Err(Error::msg(format!(
                "unknown extended-key version {other:#x}; paste xpub/tpub/zpub/vpub or a descriptor"
            )));
        }
    };
    let mut out = data;
    out[0..4].copy_from_slice(&new_ver.to_be_bytes());
    Ok((bitcoin::base58::encode_check(&out), kind))
}

fn check_xpub_network(xpub_str: &str, network: Network) -> Result<(), Error> {
    let xpub = Xpub::from_str(xpub_str).map_err(|e| Error::msg(e.to_string()))?;
    let ok = match (xpub.network, network) {
        (NetworkKind::Main, Network::Bitcoin) => true,
        (NetworkKind::Test, Network::Signet | Network::Testnet | Network::Regtest) => true,
        _ => false,
    };
    if !ok {
        return Err(Error::msg(format!(
            "xpub network {:?} does not match identity network {network:?}",
            xpub.network
        )));
    }
    Ok(())
}

fn wrap_xpub(xpub: &str, kind: WatchKind, chain: u32) -> Result<String, Error> {
    // chain 0 = receive, 1 = change. Account xpub as exported by Blue.
    let inner = format!("{xpub}/{chain}/*");
    match kind {
        WatchKind::Wpkh => Ok(format!("wpkh({inner})")),
        WatchKind::Tr => Ok(format!("tr({inner})")),
    }
}

fn normalize_descriptor(s: &str) -> Result<String, Error> {
    let desc = s.trim();
    // Drop optional checksum after #.
    let desc = desc.split('#').next().unwrap_or(desc).trim();
    Descriptor::<DescriptorPublicKey>::from_str(desc)
        .map_err(|e| Error::Miniscript(e.to_string()))?;
    Ok(desc.to_string())
}

fn kind_from_descriptor(desc: &str) -> Result<WatchKind, Error> {
    let d = desc.trim().to_ascii_lowercase();
    if d.starts_with("wpkh(") {
        Ok(WatchKind::Wpkh)
    } else if d.starts_with("tr(") {
        Ok(WatchKind::Tr)
    } else {
        Err(Error::msg(
            "descriptor must be wpkh(...) or tr(...); nested/legacy scripts are out of MVP",
        ))
    }
}

fn change_descriptor(receive: &str) -> Result<String, Error> {
    if let Some(rest) = receive.strip_suffix("/0/*)") {
        return Ok(format!("{rest}/1/*)"));
    }
    if receive.ends_with("/*)") {
        // Already a single ranged chain; require the caller to pass both or an account xpub.
        return Err(Error::msg(
            "cannot infer change descriptor (expected .../0/*); pass account xpub or a /0/* receive descriptor",
        ));
    }
    Err(Error::msg(
        "receive descriptor is not ranged (.../0/*); Blue account xpub is the usual import",
    ))
}

/// Default public Esplora for a network. Mainnet and regtest have none (pass --esplora / RPC).
///
/// First URL is the preferred host. Callers should try the list in order: some
/// public explorers time out from some networks (mempool.space often does).
pub fn default_esplora_urls(network: Network) -> &'static [&'static str] {
    match network {
        Network::Signet => &[
            "https://blockstream.info/signet/api",
            "https://mempool.space/signet/api",
        ],
        Network::Testnet => &[
            "https://blockstream.info/testnet/api",
            "https://mempool.space/testnet/api",
        ],
        Network::Regtest | Network::Bitcoin => &[],
    }
}

pub fn default_esplora_url(network: Network) -> Option<&'static str> {
    default_esplora_urls(network).first().copied()
}
