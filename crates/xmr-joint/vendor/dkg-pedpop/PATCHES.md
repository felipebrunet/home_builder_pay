# Local patches vs crates.io dkg-pedpop 0.6.0

1. `multiexp` dependency: `0.4` → `0.5` with `batch` feature (schnorr-signatures 0.5.x expects multiexp 0.5 `BatchVerifier`).
2. `KeyMachine::calculate_share`: `batch.verify_with_vartime_blame()` → `verify_vartime_with_vartime_blame()` because multiexp 0.5's constant-time verify requires `G: ConditionallySelectable`, which is not part of the generic `Ciphersuite::G` bound (compile fails for generic `C`).

PoC/stagenet only; not for production hardening.
