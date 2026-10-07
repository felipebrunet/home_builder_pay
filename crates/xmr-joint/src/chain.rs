//! Daemon HTTPS de stagenet. No guarda la cadena: escanea un rango.

use monero_simple_request_rpc::{prelude::MoneroDaemon, SimpleRequestTransport};
use monero_wallet::{
    interface::{
        FeePriority, FeeRate, ProvidesBlockchainMeta, ProvidesFeeRates,
        ProvidesScannableBlocks, PublishTransaction,
    },
    OutputWithDecoys, Scanner, ViewPair, WalletOutput,
};
use rand_core::OsRng;

use crate::network::RING_LEN;
use crate::{Error, Result};

pub type Daemon = MoneroDaemon<SimpleRequestTransport>;

pub async fn connect(url: &str) -> Result<Daemon> {
    SimpleRequestTransport::new(url.to_string())
        .await
        .map_err(|e| Error::Chain(e.to_string()))
}

pub async fn tip(rpc: &Daemon) -> Result<usize> {
    rpc.latest_block_number()
        .await
        .map_err(|e| Error::Chain(e.to_string()))
}

pub async fn fee_rate(rpc: &Daemon) -> Result<FeeRate> {
    rpc.fee_rate(FeePriority::Unimportant, u64::MAX)
        .await
        .map_err(|e| Error::Chain(e.to_string()))
}

/// Escanea `from..=to` inclusive con la view que le pases (personal o de la caja).
pub async fn scan(rpc: &Daemon, view: ViewPair, from: usize, to: usize) -> Result<Vec<WalletOutput>> {
    Ok(scan_marcado(rpc, view, from, to)
        .await?
        .into_iter()
        .map(|(_, output)| output)
        .collect())
}

/// Igual que [`scan`], y anota la altura de cada salida.
pub async fn scan_marcado(
    rpc: &Daemon,
    view: ViewPair,
    from: usize,
    to: usize,
) -> Result<Vec<(usize, WalletOutput)>> {
    if to < from {
        return Err(Error::Chain("rango de bloques al revés".into()));
    }
    let mut scanner = Scanner::new(view);
    let mut found = Vec::new();
    for height in from..=to {
        let block = rpc
            .scannable_block_by_number(height)
            .await
            .map_err(|e| Error::Chain(format!("bloque {height}: {e}")))?;
        let outs = scanner
            .scan(block)
            .map_err(|e| Error::Chain(e.to_string()))?
            .not_additionally_locked();
        found.extend(outs.into_iter().map(|output| (height, output)));
    }
    Ok(found)
}

/// Arma el anillo de una salida que la billetera ya guardó. No vuelve a escanear la cadena.
pub async fn anillar(raw: Vec<u8>) -> Result<(Vec<OutputWithDecoys>, (u64, u64))> {
    let output = WalletOutput::read(&mut std::io::Cursor::new(raw))
        .map_err(|e| Error::Chain(format!("salida: {e}")))?;
    let rpc = connect(&crate::network::daemon_url()).await?;
    let altura = tip(&rpc).await?;
    let rate = fee_rate(&rpc).await?;
    let partes = crate::fund::fee_parts(&rate);
    let decoy = with_decoys(&rpc, output, altura).await?;
    Ok((vec![decoy], partes))
}

/// Igual que [`anillar`], para las salidas que la caja ya guardó. Un solo viaje al nodo.
pub async fn anillar_varias(raws: Vec<Vec<u8>>) -> Result<(Vec<OutputWithDecoys>, (u64, u64))> {
    if raws.is_empty() {
        return Err(Error::Chain("no hay salidas".into()));
    }
    let rpc = connect(&crate::network::daemon_url()).await?;
    let altura = tip(&rpc).await?;
    let rate = fee_rate(&rpc).await?;
    let partes = crate::fund::fee_parts(&rate);
    let mut decoys = Vec::with_capacity(raws.len());
    for raw in raws {
        let output = WalletOutput::read(&mut std::io::Cursor::new(raw))
            .map_err(|e| Error::Chain(format!("salida: {e}")))?;
        decoys.push(with_decoys(&rpc, output, altura).await?);
    }
    Ok((decoys, partes))
}

pub async fn with_decoys(rpc: &Daemon, output: WalletOutput, tip_height: usize) -> Result<OutputWithDecoys> {
    OutputWithDecoys::new(&mut OsRng, rpc, RING_LEN, tip_height, output)
        .await
        .map_err(|e| Error::Chain(format!("decoys: {e}")))
}

pub async fn publish(rpc: &Daemon, tx: &monero_wallet::transaction::Transaction) -> Result<()> {
    rpc.publish_transaction(tx)
        .await
        .map_err(|e| Error::Chain(e.to_string()))
}

pub async fn publish_bytes(rpc: &Daemon, bytes: &[u8]) -> Result<()> {
    let tx = monero_wallet::transaction::Transaction::read(&mut std::io::Cursor::new(bytes))
        .map_err(|e| Error::Chain(format!("tx: {e}")))?;
    publish(rpc, &tx).await
}

/// Pregunta al daemon qué key images ya se gastaron (0 = libre, 1/2 = gastada).
///
/// Va por la ruta HTTP `/is_key_image_spent` (no es JSON-RPC: ahí responde Method not found).
/// En nodos con `restricted-rpc` puede fallar: el caller trata el error como "no supe".
pub async fn key_images_spent(rpc: &Daemon, images: &[[u8; 32]]) -> Result<Vec<bool>> {
    if images.is_empty() {
        return Ok(Vec::new());
    }
    let hexes: Vec<String> = images.iter().map(hex::encode).collect();
    let params = serde_json::json!({ "key_images": hexes }).to_string();
    let raw = rpc
        .rpc_call("is_key_image_spent", Some(params), 256 * 1024)
        .await
        .map_err(|e| Error::Chain(e.to_string()))?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| Error::Chain(format!("is_key_image_spent: {e}")))?;
    let arr = v
        .get("spent_status")
        .and_then(|x| x.as_array())
        .ok_or_else(|| Error::Chain("is_key_image_spent sin spent_status".into()))?;
    if arr.len() != images.len() {
        return Err(Error::Chain(format!(
            "is_key_image_spent devolvió {} estados, pedí {}",
            arr.len(),
            images.len()
        )));
    }
    Ok(arr
        .iter()
        .map(|x| x.as_u64().unwrap_or(0) != 0)
        .collect())
}
