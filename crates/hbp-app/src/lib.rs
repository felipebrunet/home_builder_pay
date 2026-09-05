//! Product GUI logic (works, staging, backup). The window lives in `main`.

mod datetime;
mod flow;
mod pay;
mod protocol;
mod work;

pub use datetime::{format_unix_local_es, validate_deadline_order, DeadlineFields, MONTHS_ES};
pub use flow::{completed_steps, next_step, NextKind, NextStep, WorkProgress};
pub use pay::{
    agreed_fx_line, apply_finished_coop_hex, apply_incoming_quote, apply_verified_p1_funding,
    build_our_partial,
    build_p1_funding_psbt, can_open_stop_wizard, can_recotizar, classify_funding_psbt,
    coin_from_fields, coin_from_watched, combine_signed_funding, complete_incoming_partial,
    contract_bond_minor, coop_action, coop_contribute, coop_filename, coop_finish, coop_propose,
    coop_propose_on, coop_sign, coop_tx_kind_from_artifact, coop_tx_wire, draft_quote, escrow_addrs,
    format_obra_money, fund_handshake_step, funding_wire, funding_wire_hex, hex_artifact,
    hex_from_artifact, infer_coop_kind, lock_quote_if_ready, looks_like_signed_coop_hex,
    needs_coop_publish, pago_coop_gate, recover_coop_tx_into_draft, restore_unconfirmed_bond,
    spanish_now_pay, stash_finished_coop_hex, can_open_stop_or_redo, can_redo_bond_return,
    merge_coop_file, obra_amount_pair, our_funding_need, p1_blocks_bond_return, parse_fee,
    parse_psbt, parse_psbt_bytes, partida_spec_minor, partida_ui_enabled, party_role, pay_stage,
    prefer_funding_psbt, preview_quote_sats, price_minor_from_major, psbt_display_text,
    psbt_file_bytes, psbt_to_base64, psbt_to_hex, quote_fully_signed, quote_price_minor,
    recotizar_if_unfunded, show_main_fund_ui, sign_our_quote, spanish_chain_status, spanish_now,
    suggest_watched, txid_from_tx_hex, verify_present_quote_sigs, CoopAction, FundHandshakeStep,
    FundMark, FundView,
    FundingSendKind, PagoCoopGate, PayCoins, PayStage, PayUiDraft, StopStep, ART_COIN, ART_COOP,
    ART_COOP_TX,
    ART_ONESIG, ART_PARTIAL, ART_PSBT, ART_SIGNED, ART_TX, KIND_BOND, KIND_PARTIDA,
};
pub use protocol::{contratista_accept, import_signed, mandante_commit, require_signet_offer};
pub use work::{
    default_works_root, draft_equal_stages, export_backup, import_backup, read_backup_file,
    slugify, write_backup_file, UiPrefs, WorkBackup, WorkEntry, WorkStore,
};
