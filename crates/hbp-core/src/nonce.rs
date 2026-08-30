use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::contract::sha256_bytes;
use crate::Error;

/// Persisted set of consumed MuSig2 nonce seeds.
///
/// Insert the seed **before** using it to sign. If the process dies after
/// insert and before broadcast, we waste a session; we never reuse a seed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NonceJournal {
    used_seed_hashes: BTreeSet<String>,
    /// sighash hex → seed hex for an in-flight file MuSig2 session.
    #[serde(default)]
    pending: BTreeMap<String, String>,
}

impl NonceJournal {
    pub fn consume_seed(&mut self, seed: &[u8; 32]) -> crate::Result<()> {
        let hash = hex::encode(sha256_bytes(seed));
        if !self.used_seed_hashes.insert(hash) {
            return Err(Error::NonceReused);
        }
        Ok(())
    }

    pub fn contains_seed(&self, seed: &[u8; 32]) -> bool {
        let hash = hex::encode(sha256_bytes(seed));
        self.used_seed_hashes.contains(&hash)
    }

    pub fn len(&self) -> usize {
        self.used_seed_hashes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.used_seed_hashes.is_empty()
    }

    pub fn stash_pending(&mut self, sighash_hex: &str, seed: &[u8; 32]) {
        self.pending
            .insert(sighash_hex.to_string(), hex::encode(seed));
    }

    pub fn take_pending(&mut self, sighash_hex: &str) -> crate::Result<[u8; 32]> {
        let s = self.pending.remove(sighash_hex).ok_or_else(|| {
            Error::protocol("no pending MuSig2 nonce for this sighash; run coop-propose first")
        })?;
        let bytes = hex::decode(s.trim())?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| Error::protocol("pending nonce seed must be 32 bytes"))?;
        Ok(arr)
    }

    pub fn peek_pending(&self, sighash_hex: &str) -> crate::Result<[u8; 32]> {
        let s = self.pending.get(sighash_hex).ok_or_else(|| {
            Error::protocol("no pending MuSig2 nonce for this sighash; run coop-propose first")
        })?;
        let bytes = hex::decode(s.trim())?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| Error::protocol("pending nonce seed must be 32 bytes"))?;
        Ok(arr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_reuse() {
        let mut j = NonceJournal::default();
        let seed = [7u8; 32];
        j.consume_seed(&seed).unwrap();
        assert!(matches!(j.consume_seed(&seed), Err(Error::NonceReused)));
    }

    #[test]
    fn pending_seed_roundtrip() {
        let mut j = NonceJournal::default();
        let seed = [9u8; 32];
        j.consume_seed(&seed).unwrap();
        j.stash_pending("aa", &seed);
        assert_eq!(j.peek_pending("aa").unwrap(), seed);
        assert_eq!(j.take_pending("aa").unwrap(), seed);
        assert!(j.take_pending("aa").is_err());
    }
}
