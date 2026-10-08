//! Envío desde la billetera personal de una firma.
//!
//! El cambio vuelve a la misma view. Si no sobra nada, la salida de cambio va igual con
//! 0 XMR (la red pide dos salidas; una salida RingCT de monto cero es válida, como la dummy
//! de monero-wallet). Así «enviar todo» manda el saldo entero menos el fee, sin dust.
//! El fee se ajusta al que calcula oxide.

use monero_wallet::{
    ringct::RctType,
    send::{Change, SignableTransaction},
    OutputWithDecoys, ViewPair,
};
use rand_core::{CryptoRng, OsRng, RngCore};
use zeroize::Zeroizing;

use crate::network::{Net, PICONERO};
use crate::{Error, Result};

/// Transacción personal ya firmada, lista para publicar.
pub struct EnvioFirmado {
    /// Hash de la transacción, en hex.
    pub txid: String,
    /// Fee que quedó en la transacción, en piconero.
    pub fee: u64,
    /// Cambio que vuelve a esta billetera, en piconero.
    pub cambio: u64,
    /// Blob firmado.
    pub bytes: Vec<u8>,
}

/// Cuánto mandar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Monto {
    /// Este monto exacto; el fee y el cambio salen del resto.
    Exacto(u64),
    /// Todo lo de `inputs` menos el fee; el cambio queda en 0.
    Todo,
}

/// `texto` es un monto en XMR. Acepta punto o coma, hasta 12 decimales.
pub fn piconero_de(texto: &str) -> Result<u64> {
    let t = texto.trim().replace(',', ".");
    if t.is_empty() || t.starts_with(['+', '-']) || t.matches('.').count() > 1 {
        return Err(Error::Wallet("monto inválido".into()));
    }
    let (whole, frac) = match t.split_once('.') {
        Some((w, f)) => (w, f),
        None => (t.as_str(), ""),
    };
    if whole.is_empty() && frac.is_empty() {
        return Err(Error::Wallet("monto inválido".into()));
    }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::Wallet("monto inválido".into()));
    }
    if frac.len() > 12 {
        return Err(Error::Wallet("demasiados decimales".into()));
    }
    if whole.len() > 7 {
        return Err(Error::Wallet("el monto es demasiado grande".into()));
    }
    let enteros: u64 = if whole.is_empty() {
        0
    } else {
        whole
            .parse()
            .map_err(|_| Error::Wallet("monto inválido".into()))?
    };
    let mut fraccion = frac.to_string();
    while fraccion.len() < 12 {
        fraccion.push('0');
    }
    let parte: u64 = fraccion
        .parse()
        .map_err(|_| Error::Wallet("monto inválido".into()))?;
    enteros
        .checked_mul(PICONERO)
        .and_then(|v| v.checked_add(parte))
        .ok_or_else(|| Error::Wallet("el monto no entra en piconero".into()))
}

/// Índices, del más chico al más grande, que cubren `necesita`. Como máximo 8.
pub fn elegir_montos(montos: &[u64], necesita: u64) -> Result<Vec<usize>> {
    if necesita == 0 {
        return Err(Error::Wallet("el monto es cero".into()));
    }
    let mut orden: Vec<usize> = (0..montos.len()).collect();
    orden.sort_by_key(|&i| (montos[i], i));
    let mut suma = 0u64;
    let mut elegidos = Vec::new();
    for i in orden {
        if montos[i] == 0 {
            continue;
        }
        suma = suma.saturating_add(montos[i]);
        elegidos.push(i);
        if suma >= necesita {
            return Ok(elegidos);
        }
        if elegidos.len() == 8 {
            break;
        }
    }
    Err(Error::Wallet(format!(
        "alcanza {suma} piconero y hacen falta {necesita}"
    )))
}

/// Arma los anillos, firma y publica contra el daemon de stagenet.
///
/// `crudas` son [`monero_wallet::WalletOutput`] serializadas.
pub async fn publicar(
    spend: Zeroizing<monero_wallet::ed25519::Scalar>,
    view: ViewPair,
    crudas: Vec<Vec<u8>>,
    destino: &str,
    monto: Monto,
    net: Net,
) -> Result<EnvioFirmado> {
    if crudas.is_empty() {
        return Err(Error::Wallet("no hay salidas para gastar".into()));
    }
    let mut salidas = Vec::with_capacity(crudas.len());
    for raw in crudas {
        salidas.push(
            monero_wallet::WalletOutput::read(&mut std::io::Cursor::new(raw))
                .map_err(|e| Error::Wallet(format!("salida guardada: {e}")))?,
        );
    }
    let url = net
        .daemon()
        .ok_or_else(|| Error::Wallet("esta red no tiene daemon".into()))?;
    let rpc = crate::chain::connect(&url).await?;
    let tip = crate::chain::tip(&rpc).await?;
    let rate = crate::chain::fee_rate(&rpc).await?;
    let mut decoys = Vec::with_capacity(salidas.len());
    for salida in salidas {
        decoys.push(crate::chain::with_decoys(&rpc, salida, tip).await?);
    }
    let firmado = firmar_envio(&mut OsRng, &spend, view, decoys, destino, monto, net, rate)?;
    crate::chain::publish_bytes(&rpc, &firmado.bytes).await?;
    Ok(firmado)
}

/// Firma un envío de una sola firma. `monto` y el fee salen de `inputs`; el resto es cambio
/// (que puede ser 0: la salida de cambio va igual, con monto cero).
pub fn firmar_envio<R: RngCore + CryptoRng>(
    rng: &mut R,
    spend: &Zeroizing<monero_wallet::ed25519::Scalar>,
    view: ViewPair,
    inputs: Vec<OutputWithDecoys>,
    destino: &str,
    monto: Monto,
    net: Net,
    fee_rate: monero_wallet::interface::FeeRate,
) -> Result<EnvioFirmado> {
    if monto == Monto::Exacto(0) {
        return Err(Error::Wallet("el monto es cero".into()));
    }
    if inputs.is_empty() {
        return Err(Error::Wallet("no hay salidas para gastar".into()));
    }
    let dest = crate::coop::parse_address(destino, net.oxide()).map_err(|e| Error::Wallet(e.to_string()))?;
    let suma_inputs: u64 = inputs.iter().map(|i| i.commitment().amount).sum();
    let mut fee_intento = 0u64;
    let mut ultimo = None;
    for _ in 0..6 {
        let monto = match monto {
            Monto::Exacto(m) => m,
            Monto::Todo => match suma_inputs.checked_sub(fee_intento) {
                Some(m) if m > 0 => m,
                _ => return Err(Error::Wallet("el saldo no alcanza para el fee".into())),
            },
        };
        let mut ovk = [0u8; 32];
        rng.fill_bytes(&mut ovk);
        let built = SignableTransaction::new(
            RctType::ClsagBulletproofPlus,
            Zeroizing::new(ovk),
            inputs.clone(),
            vec![(dest, monto)],
            Change::new(view.clone(), None),
            vec![],
            fee_rate.clone(),
        );
        match built {
            Ok(tx) => {
                let fee = tx.necessary_fee();
                if fee != fee_intento {
                    fee_intento = fee;
                    ultimo = Some(format!("el fee quedó en {fee} piconero"));
                    continue;
                }
                let suma = tx.input_sum();
                let cambio = suma.saturating_sub(monto).saturating_sub(fee);
                if monto.saturating_add(fee).saturating_add(cambio) != suma {
                    return Err(Error::Wallet("el envío no cierra".into()));
                }
                let firmada = tx.sign(rng, spend).map_err(|e| Error::Wallet(e.to_string()))?;
                return Ok(EnvioFirmado {
                    txid: hex::encode(firmada.hash()),
                    fee,
                    cambio,
                    bytes: firmada.serialize(),
                });
            }
            Err(monero_wallet::send::SendError::NotEnoughFunds {
                necessary_fee: Some(fee),
                ..
            }) => {
                fee_intento = fee;
                ultimo = Some(format!("no alcanza: el fee pide {fee} piconero"));
            }
            Err(e) => return Err(Error::Wallet(e.to_string())),
        }
    }
    Err(Error::Wallet(
        ultimo.unwrap_or_else(|| "no cerró el fee del envío".into()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cero_cero_cuatro_son_cuatro_centesimas() {
        assert_eq!(piconero_de("0.04").unwrap(), 40_000_000_000);
        assert_eq!(piconero_de("0,04").unwrap(), 40_000_000_000);
        assert_eq!(piconero_de("1").unwrap(), PICONERO);
        assert_eq!(piconero_de("0.000000000001").unwrap(), 1);
        assert!(piconero_de("0.0000000000001").is_err());
        assert!(piconero_de("").is_err());
        assert!(piconero_de("-1").is_err());
    }

    #[test]
    fn elige_las_salidas_mas_chicas() {
        let idx = elegir_montos(&[10, 1, 0, 4], 5).unwrap();
        assert_eq!(idx, vec![1, 3]);
        assert!(elegir_montos(&[1, 2], 5).is_err());
        assert!(elegir_montos(&[1; 9], 9).is_err());
        assert!(elegir_montos(&[3], 0).is_err());
    }
}

#[cfg(test)]
mod pruebas_tx {
    use super::*;
    use crate::prueba_tx::{billetera, fee_rate, montos_para, salida_falsa, verificar};
    use monero_wallet::transaction::Transaction;

    fn armar(monto: Monto, entradas: &[u64]) -> (EnvioFirmado, Transaction, ViewPair, ViewPair, Vec<u64>) {
        let (spend, mia) = billetera();
        let (_, otra) = billetera();
        let spend_pub = mia.spend().into();
        let falsas: Vec<_> = entradas.iter().map(|m| salida_falsa(spend_pub, *m)).collect();
        let anillos: Vec<_> = falsas.iter().map(|f| f.anillo.clone()).collect();
        let destino = otra.legacy_address(Net::Stagenet.oxide()).to_string();
        let firmado = firmar_envio(
            &mut OsRng,
            &spend,
            mia.clone(),
            falsas.into_iter().map(|f| f.salida).collect(),
            &destino,
            monto,
            Net::Stagenet,
            fee_rate(),
        )
        .unwrap();
        let tx = Transaction::read(&mut std::io::Cursor::new(firmado.bytes.clone())).unwrap();
        let fee = verificar(&tx, &anillos);
        assert_eq!(fee, firmado.fee);
        (firmado, tx, mia, otra, anillos.iter().map(|_| 0).collect())
    }

    #[test]
    fn enviar_todo_deja_cambio_cero_y_es_valida() {
        let (f, tx, mia, otra, _) = armar(Monto::Todo, &[30_000_000_000, 9_000_000_000]);
        assert_eq!(f.cambio, 0);
        let Transaction::V2 { prefix, .. } = &tx else { unreachable!() };
        assert_eq!(prefix.outputs.len(), 2, "destino + cambio de 0");
        assert_eq!(montos_para(&otra, &tx), vec![39_000_000_000 - f.fee]);
        // El cambio existe y vuelve a esta billetera con 0 XMR (sin dust de 1 piconero).
        assert_eq!(montos_para(&mia, &tx), vec![0]);
    }

    #[test]
    fn monto_exacto_con_cambio_cero_ya_no_falla() {
        // Primero se mide el fee con un envío normal; después se pide justo suma − fee.
        let (f1, _, _, _, _) = armar(Monto::Exacto(1_000_000_000), &[5_000_000_000]);
        let (f2, tx, mia, _, _) = armar(Monto::Exacto(5_000_000_000 - f1.fee), &[5_000_000_000]);
        assert_eq!(f2.cambio, 0);
        assert_eq!(montos_para(&mia, &tx), vec![0]);
    }

    #[test]
    fn monto_normal_devuelve_el_cambio() {
        let (f, tx, mia, otra, _) = armar(Monto::Exacto(1_000_000_000), &[5_000_000_000]);
        assert_eq!(montos_para(&otra, &tx), vec![1_000_000_000]);
        assert_eq!(montos_para(&mia, &tx), vec![f.cambio]);
        assert_eq!(f.cambio, 5_000_000_000 - 1_000_000_000 - f.fee);
    }
}
