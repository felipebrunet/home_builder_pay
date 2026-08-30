use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::contract::sha256_bytes;
use crate::Error;

/// Persisted set of consumed MuSig2 nonce seeds.
///
/// Insert the seed **before** using it to sign. If the process dies after
/// insert and before broadcast, we waste a session; we never reuse a seed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NonceJournal {
    used_seed_hashes: BTreeSet<String>,
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
}
