//! Shared Tor v3 onion for the baked swarm `konstruado-red-1`.
//!
//! The private key is public on purpose: every copy of the app may host
//! this service so two nodes meet without exchanging addresses. It is a
//! meeting room, not an identity.

/// Virtual port on both the rendezvous onion and each personal onion.
pub const VIRT_PORT: u16 = 17432;

/// Expanded ed25519 secret (64 bytes, SHA-512 of the seed) as Tor's
/// ADD_ONION wants it. A 32-byte seed is rejected with 512.
pub const RENDEZVOUS_KEY: &str =
    "MnLG3eCj6Vrkx1Zz+Kd4ihD6WdVyaxrdi6lbyEyPOy+d2uZEHibtp18L2366t/3FELrsumKRi1m3T3oYTVdAmg==";

/// Onion v3 for [`RENDEZVOUS_KEY`] as returned by Tor ADD_ONION.
pub const RENDEZVOUS_ONION: &str =
    "vhirdvyvk6vj3fzbtoahqwa5kzaxk26n4qmxfpogos5cufnmgjdjagqd.onion";
