//! Shared Tor v3 onion for the baked swarm `konstruado-red-1`.
//!
//! The private key is public on purpose: every copy of the app may host
//! this service so two nodes meet without exchanging addresses. It is a
//! meeting room, not an identity.

/// Virtual port on both the rendezvous onion and each personal onion.
pub const VIRT_PORT: u16 = 17432;

/// Ed25519 seed (32 bytes, base64) for the swarm hidden service.
pub const RENDEZVOUS_KEY: &str = "wTJ9MZmRCqOQVmSM9MKDQ3dMUH0JU4h1e55QMD9fWz0=";

/// Onion v3 matching [`RENDEZVOUS_KEY`].
pub const RENDEZVOUS_ONION: &str =
    "e5czeiobcupe344mgi5nf4wlpera5bzs3fh425o53qvobs7hez4melqd.onion";
