//! Pruebas sin red de transacciones armadas por Konstruado.
//!
//! Se fabrican salidas "en cadena" falsas (anillo de 16 con miembros al azar y el
//! real en una posición al azar), se arma y firma la transacción como en producción,
//! y después se verifica como lo haría un nodo: CLSAG de cada input contra su anillo,
//! Bulletproof+ de las salidas y balance de commitments (pseudo_outs = salidas + fee·H).
//! Por último se escanea la transacción con la view de cada destino para leer los
//! montos que de verdad quedaron (así se ve la salida de 0 XMR).

use std::io::Cursor;

use curve25519_dalek::{constants::ED25519_BASEPOINT_TABLE, EdwardsPoint, Scalar as DScalar};
use monero_clsag::Decoys;
use monero_wallet::{
    block::{Block, BlockHeader},
    ed25519::{CompressedPoint, Commitment, Point, Scalar},
    interface::{FeeRate, ScannableBlock},
    ringct::{RctPrunable, RctType},
    transaction::{Input, Pruned, Timelock, Transaction, TransactionPrefix},
    OutputWithDecoys, Scanner, ViewPair,
};
use rand_core::{OsRng, RngCore};
use zeroize::Zeroizing;

/// Una salida falsa que `spend_pub` puede gastar, con su anillo (para verificar el CLSAG).
pub struct SalidaFalsa {
    pub salida: OutputWithDecoys,
    pub anillo: Vec<[CompressedPoint; 2]>,
}

fn punto_al_azar() -> Point {
    Point::from(&DScalar::random(&mut OsRng) * ED25519_BASEPOINT_TABLE)
}

/// `spend_pub` es la llave pública de gasto (personal o la de grupo de la caja).
pub fn salida_falsa(spend_pub: EdwardsPoint, monto: u64) -> SalidaFalsa {
    let offset = DScalar::random(&mut OsRng);
    let key = Point::from(spend_pub + &offset * ED25519_BASEPOINT_TABLE);
    let commitment = Commitment::new(Scalar::random(&mut OsRng), monto);
    let signer = (OsRng.next_u32() % 16) as u8;
    let mut ring: Vec<[Point; 2]> = (0..16).map(|_| [punto_al_azar(), punto_al_azar()]).collect();
    ring[usize::from(signer)] = [key, commitment.commit()];
    let mut offsets = vec![1_000_000u64];
    offsets.extend((1..16).map(|i| 37 + i as u64));
    let decoys = Decoys::new(offsets, signer, ring.clone()).expect("decoys");
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&key.compress().to_bytes());
    Scalar::from(offset).write(&mut bytes).unwrap();
    commitment.write(&mut bytes).unwrap();
    decoys.write(&mut bytes).unwrap();
    let salida = OutputWithDecoys::read(&mut Cursor::new(bytes)).expect("output");
    let anillo = ring.iter().map(|[k, c]| [k.compress(), c.compress()]).collect();
    SalidaFalsa { salida, anillo }
}

/// Fee típico de stagenet (por peso, máscara de redondeo).
pub fn fee_rate() -> FeeRate {
    FeeRate::new(20_000, 10_000).unwrap()
}

/// Billetera al azar: (spend privada, view pair).
pub fn billetera() -> (Zeroizing<Scalar>, ViewPair) {
    let spend = DScalar::random(&mut OsRng);
    let view = Zeroizing::new(Scalar::random(&mut OsRng));
    let vp = ViewPair::new(Point::from(&spend * ED25519_BASEPOINT_TABLE), view).unwrap();
    (Zeroizing::new(Scalar::from(spend)), vp)
}

/// Verifica la transacción como un nodo (sin cadena: los anillos los pasa el test).
/// Devuelve el fee.
pub fn verificar(tx: &Transaction, anillos: &[Vec<[CompressedPoint; 2]>]) -> u64 {
    let Transaction::V2 { prefix, proofs: Some(proofs) } = tx else { panic!("no es RingCT") };
    assert!(prefix.outputs.len() >= 2, "la red pide al menos dos salidas");
    assert_eq!(proofs.rct_type(), RctType::ClsagBulletproofPlus);
    let RctPrunable::Clsag { clsags, pseudo_outs, bulletproof } = &proofs.prunable else {
        panic!("no es CLSAG")
    };
    assert!(bulletproof.verify(&mut OsRng, &proofs.base.commitments), "bulletproof+ inválido");
    let msg = tx.signature_hash().expect("hash de firma");
    assert_eq!(clsags.len(), prefix.inputs.len());
    // Los inputs van ordenados por key image; los anillos del test, en el orden de armado.
    // Probamos cada CLSAG contra cada anillo y exigimos que todos encuentren el suyo.
    for (i, clsag) in clsags.iter().enumerate() {
        let Input::ToKey { key_image, .. } = &prefix.inputs[i] else { panic!("input raro") };
        let ok = anillos
            .iter()
            .any(|a| clsag.verify(a.clone(), key_image, &pseudo_outs[i], &msg).is_ok());
        assert!(ok, "CLSAG {i} inválido");
    }
    let suma = |v: &[CompressedPoint]| {
        v.iter().fold(EdwardsPoint::default(), |acc, p| acc + p.decompress().unwrap().into())
    };
    let h = CompressedPoint::H.decompress().unwrap().into();
    let fee = proofs.base.fee;
    assert_eq!(
        suma(pseudo_outs),
        suma(&proofs.base.commitments) + h * DScalar::from(fee),
        "los commitments no balancean"
    );
    fee
}

/// Montos que `view` recibe en `tx` (escaneando un bloque falso que la contiene).
pub fn montos_para(view: &ViewPair, tx: &Transaction) -> Vec<u64> {
    let minero = Transaction::V2 {
        prefix: TransactionPrefix {
            additional_timelock: Timelock::None,
            inputs: vec![Input::Gen(1_000)],
            outputs: vec![],
            extra: vec![],
        },
        proofs: None,
    };
    let header = BlockHeader {
        hardfork_version: 16,
        hardfork_signal: 16,
        timestamp: 0,
        previous: [0; 32],
        nonce: 0,
    };
    let block = Block::new(header, minero, vec![tx.hash()]).expect("bloque");
    let mut scanner = Scanner::new(view.clone());
    let pruned: Transaction<Pruned> = tx.clone().into();
    let encontradas = scanner
        .scan(ScannableBlock {
            block,
            transactions: vec![pruned],
            output_index_for_first_ringct_output: Some(0),
        })
        .expect("scan")
        .ignore_additional_timelock();
    let mut v: Vec<u64> = encontradas.iter().map(|o| o.commitment().amount).collect();
    v.sort();
    v
}

#[cfg(test)]
mod control {
    use super::*;
    use crate::network::Net;
    use crate::personal::{firmar_envio, Monto};

    /// El verificador no es de adorno: con un anillo cambiado el CLSAG no pasa.
    #[test]
    #[should_panic(expected = "CLSAG")]
    fn anillo_cambiado_no_verifica() {
        let (spend, mia) = billetera();
        let (_, otra) = billetera();
        let f = salida_falsa(mia.spend().into(), 5_000_000_000);
        let mut anillo = f.anillo.clone();
        anillo[3][0] = punto_al_azar().compress();
        anillo[3][1] = punto_al_azar().compress();
        for m in anillo.iter_mut() {
            m[0] = punto_al_azar().compress();
        }
        let destino = otra.legacy_address(Net::Stagenet.oxide()).to_string();
        let firmado = firmar_envio(&mut OsRng, &spend, mia, vec![f.salida], &destino, Monto::Todo, Net::Stagenet, fee_rate())
            .unwrap();
        let tx = Transaction::read(&mut Cursor::new(firmado.bytes)).unwrap();
        verificar(&tx, &[anillo]);
    }
}
