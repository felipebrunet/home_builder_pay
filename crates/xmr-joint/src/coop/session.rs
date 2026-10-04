//! Session state machine: `AliceProposed → BobSigned → AliceComplete` (or Abort).
//!
//! This module tracks protocol progress and enforces who may act next. Crypto
//! work lives in [`crate::coop::assemble`], [`crate::coop::sign_partial`], [`crate::coop::verify`].

use monero_wallet::address::Network;

use crate::coop::messages::{CompletedTransaction, Proposal, Skeleton};
use crate::coop::{Error, Result};

/// Which party we are in this session.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
  /// Alice: proposes, verifies Bob, signs last, broadcasts.
  Alice,
  /// Bob: assembles skeleton, signs Bob's CLSAGs first, returns to Alice.
  Bob,
}

/// Why a session aborted.
#[derive(Clone, Debug, thiserror::Error)]
pub enum AbortReason {
  /// Peer sent malformed / inconsistent data.
  #[error("invalid message: {0}")]
  InvalidMessage(String),
  /// Policy / payment check failed.
  #[error("policy: {0}")]
  Policy(String),
  /// CLSAG verification failed.
  #[error("clsag verify: {0}")]
  ClsagVerify(String),
  /// Local user cancelled.
  #[error("cancelled")]
  Cancelled,
  /// Oxide API patch missing.
  #[error("oxide patch required: {0}")]
  OxidePatch(&'static str),
  /// Peer timed out / disconnected (transport — out of scope to implement).
  #[error("transport: {0}")]
  Transport(String),
}

/// Session states.
#[derive(Clone, Debug)]
pub enum SessionState {
  /// Fresh session, nothing sent.
  Idle,
  /// Alice built and (logically) sent [`Proposal`].
  AliceProposed {
    /// The proposal on the wire.
    proposal: Proposal,
  },
  /// Bob accepted proposal, assembled, signed Bob's inputs, sent [`Skeleton`].
  BobSigned {
    /// Original proposal.
    proposal: Proposal,
    /// Skeleton + Bob partials.
    skeleton: Skeleton,
  },
  /// Alice verified, signed Alice's inputs, produced final TX.
  AliceComplete {
    /// Original proposal.
    proposal: Proposal,
    /// Skeleton used.
    skeleton: Skeleton,
    /// Completed TX message (or broadcast-equivalent).
    completed: CompletedTransaction,
  },
  /// Terminal failure.
  Aborted {
    /// Why.
    reason: AbortReason,
  },
}

impl SessionState {
  /// Human-readable label.
  pub fn label(&self) -> &'static str {
    match self {
      SessionState::Idle => "Idle",
      SessionState::AliceProposed { .. } => "AliceProposed",
      SessionState::BobSigned { .. } => "BobSigned",
      SessionState::AliceComplete { .. } => "AliceComplete",
      SessionState::Aborted { .. } => "Aborted",
    }
  }

  /// True if no further protocol steps are expected.
  pub fn is_terminal(&self) -> bool {
    matches!(self, SessionState::AliceComplete { .. } | SessionState::Aborted { .. })
  }
}

/// Cooperative session handle (local state only — no network).
#[derive(Clone, Debug)]
pub struct CoopSession {
  /// Our role.
  pub role: Role,
  /// Current state.
  pub state: SessionState,
}

impl CoopSession {
  /// Start an idle session for `role`.
  pub fn new(role: Role) -> Self {
    Self { role, state: SessionState::Idle }
  }

  /// Alice: record that we emitted a proposal.
  pub fn alice_propose(&mut self, proposal: Proposal) -> Result<()> {
    self.ensure_role(Role::Alice)?;
    match &self.state {
      SessionState::Idle => {
        proposal.validate().map_err(|e| Error::Abort(AbortReason::InvalidMessage(e)))?;
        self.state = SessionState::AliceProposed { proposal };
        Ok(())
      }
      other => Err(Error::Protocol(format!(
        "alice_propose invalid in state {}",
        other.label()
      ))),
    }
  }

  /// Bob: after assembling + signing, record the skeleton we return to Alice.
  pub fn bob_signed(&mut self, proposal: Proposal, skeleton: Skeleton) -> Result<()> {
    self.ensure_role(Role::Bob)?;
    match &self.state {
      SessionState::Idle | SessionState::AliceProposed { .. } => {
        proposal.validate().map_err(|e| Error::Abort(AbortReason::InvalidMessage(e)))?;
        skeleton.validate().map_err(|e| Error::Abort(AbortReason::InvalidMessage(e)))?;
        if skeleton.session_id != proposal.session_id {
          return Err(Error::Abort(AbortReason::InvalidMessage(
            "skeleton session_id != proposal".into(),
          )));
        }
        self.state = SessionState::BobSigned { proposal, skeleton };
        Ok(())
      }
      other => Err(Error::Protocol(format!(
        "bob_signed invalid in state {}",
        other.label()
      ))),
    }
  }

  /// Alice: accept Bob's skeleton into local state (before signing).
  ///
  /// `network` selects address encoding when checking destination C.
  pub fn alice_receive_skeleton(&mut self, skeleton: Skeleton, network: Network) -> Result<()> {
    self.ensure_role(Role::Alice)?;
    match &self.state {
      SessionState::AliceProposed { proposal } => {
        skeleton.validate().map_err(|e| Error::Abort(AbortReason::InvalidMessage(e)))?;
        if skeleton.session_id != proposal.session_id {
          return Err(Error::Abort(AbortReason::InvalidMessage(
            "skeleton session_id mismatch".into(),
          )));
        }
        crate::coop::verify::verify_skeleton_against_proposal(proposal, &skeleton, network)
          .map_err(|e| Error::Abort(AbortReason::Policy(e.to_string())))?;
        self.state = SessionState::BobSigned {
          proposal: proposal.clone(),
          skeleton,
        };
        Ok(())
      }
      other => Err(Error::Protocol(format!(
        "alice_receive_skeleton invalid in state {}",
        other.label()
      ))),
    }
  }

  /// Alice: mark complete after signing + (optional) broadcast.
  pub fn alice_complete(&mut self, completed: CompletedTransaction) -> Result<()> {
    self.ensure_role(Role::Alice)?;
    match &self.state {
      SessionState::BobSigned { proposal, skeleton } => {
        if completed.session_id != proposal.session_id {
          return Err(Error::Abort(AbortReason::InvalidMessage(
            "completed session_id mismatch".into(),
          )));
        }
        self.state = SessionState::AliceComplete {
          proposal: proposal.clone(),
          skeleton: skeleton.clone(),
          completed,
        };
        Ok(())
      }
      other => Err(Error::Protocol(format!(
        "alice_complete invalid in state {}",
        other.label()
      ))),
    }
  }

  /// Abort the session.
  pub fn abort(&mut self, reason: AbortReason) {
    self.state = SessionState::Aborted { reason };
  }

  fn ensure_role(&self, expected: Role) -> Result<()> {
    if self.role != expected {
      Err(Error::Protocol(format!(
        "role {:?} cannot perform {:?} action",
        self.role, expected
      )))
    } else {
      Ok(())
    }
  }

  /// Borrow proposal if present.
  pub fn proposal(&self) -> Option<&Proposal> {
    match &self.state {
      SessionState::AliceProposed { proposal }
      | SessionState::BobSigned { proposal, .. }
      | SessionState::AliceComplete { proposal, .. } => Some(proposal),
      _ => None,
    }
  }

  /// Borrow skeleton if present.
  pub fn skeleton(&self) -> Option<&Skeleton> {
    match &self.state {
      SessionState::BobSigned { skeleton, .. }
      | SessionState::AliceComplete { skeleton, .. } => Some(skeleton),
      _ => None,
    }
  }
}

/// Documented abort cases (for PROTOCOL.md parity).
pub fn abort_cases_summary() -> &'static [&'static str] {
  &[
    "Alice aborts if Bob's skeleton payments/fee/C/KI set disagree with the proposal",
    "Alice aborts if Bob's CLSAGs fail verify against signature_hash",
    "Alice aborts if Bob signed an Alice-owned sorted_index",
    "Bob aborts if proposal fails structural/policy validation",
    "Either aborts if oxide partial-sign API is missing (patch required)",
    "Abort before broadcast ⇒ no on-chain TX ⇒ funds remain (atomicity)",
    "After broadcast, consensus accepts or rejects the whole blob (both CLSAGs required)",
  ]
}
