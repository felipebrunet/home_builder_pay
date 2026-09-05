//! Product GUI logic (works, staging, backup). The window lives in `main`.

mod datetime;
mod flow;
mod pay;
mod protocol;
mod work;

pub use datetime::{format_unix_local_es, validate_deadline_order, DeadlineFields, MONTHS_ES};
pub use flow::{completed_steps, next_step, NextKind, NextStep, WorkProgress};
pub use pay::{
    apply_incoming_quote, apply_verified_p1_funding, build_our_partial, build_p1_funding_psbt,
    classify_funding_psbt, coin_from_fields, coin_from_watched, combine_signed_funding,
    complete_incoming_partial, coop_finish, coop_propose, coop_sign, draft_quote, escrow_addrs,
    funding_wire, funding_wire_hex, hex_artifact, hex_from_artifact, lock_quote_if_ready,
    our_funding_need, parse_fee, parse_psbt, partida_ui_enabled, party_role, pay_stage,
    preview_quote_sats, price_minor_from_major, psbt_to_hex, quote_fully_signed, sign_our_quote,
    spanish_now, suggest_watched, verify_present_quote_sigs, PayCoins, PayStage, PayUiDraft,
    ART_COIN, ART_COOP, ART_ONESIG, ART_PARTIAL, ART_PSBT, ART_SIGNED, ART_TX,
};
pub use protocol::{contratista_accept, import_signed, mandante_commit, require_signet_offer};
pub use work::{
    default_works_root, draft_equal_stages, export_backup, import_backup, read_backup_file,
    slugify, write_backup_file, UiPrefs, WorkBackup, WorkEntry, WorkStore,
};
