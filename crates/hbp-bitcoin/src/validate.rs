use bitcoin::{Address, Amount, ScriptBuf, Transaction};

use crate::Error;

#[derive(Debug, Clone)]
pub struct ExpectedFunding {
    pub bond_script: ScriptBuf,
    pub bond_sats: u64,
    pub partida_script: ScriptBuf,
    pub partida_sats: u64,
    /// Allowed change script_pubkeys (each party's wallet).
    pub change: Vec<ScriptBuf>,
    /// If true, extra outputs (wallet change) are accepted without listing them.
    pub allow_other_outputs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingIssue {
    pub message: String,
}

/// Verify a funding transaction against the agreed escrow outputs.
///
/// Extra outputs are allowed only if they pay a known change script.
/// Amounts must match exactly. This is the check that would have caught
/// the Bisq "negative miner fee" class of bugs.
pub fn validate_funding_tx(tx: &Transaction, expected: &ExpectedFunding) -> Result<(), Error> {
    if tx.output.len() < 2 {
        return Err(Error::msg("funding tx needs bond and partida outputs"));
    }
    let mut saw_bond = false;
    let mut saw_partida = false;
    for o in &tx.output {
        if o.script_pubkey == expected.bond_script {
            if saw_bond {
                return Err(Error::msg("duplicate bond output"));
            }
            if o.value != Amount::from_sat(expected.bond_sats) {
                return Err(Error::msg(format!(
                    "bond amount {} != quoted {}",
                    o.value.to_sat(),
                    expected.bond_sats
                )));
            }
            saw_bond = true;
            continue;
        }
        if o.script_pubkey == expected.partida_script {
            if saw_partida {
                return Err(Error::msg("duplicate partida output"));
            }
            if o.value != Amount::from_sat(expected.partida_sats) {
                return Err(Error::msg(format!(
                    "partida amount {} != quoted {}",
                    o.value.to_sat(),
                    expected.partida_sats
                )));
            }
            saw_partida = true;
            continue;
        }
        if expected.allow_other_outputs || expected.change.iter().any(|c| c == &o.script_pubkey) {
            continue;
        }
        return Err(Error::msg(format!(
            "unexpected output script {}",
            Address::from_script(&o.script_pubkey, bitcoin::Network::Regtest)
                .map(|a| a.to_string())
                .unwrap_or_else(|_| hex::encode(o.script_pubkey.as_bytes()))
        )));
    }
    if !saw_bond {
        return Err(Error::msg("missing bond output"));
    }
    if !saw_partida {
        return Err(Error::msg("missing partida output"));
    }
    Ok(())
}
