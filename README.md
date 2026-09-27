# Konstruado

For a new Grok session, read `CONTEXTO.md` first.

Peer-to-peer construction escrow. Desktop is Dioxus. The deal and the meeting room work. Monero keys and the 2-of-2 box are in the `xmr-joint` crate and covered by tests. The window does not broadcast a transaction yet.

The two people do not see each other like a chat. Roles:

1. **Mandante** (pays) publishes a job: name, work amount, suggested guarantee.
2. **Contratista** (builds) sees that offer on the board and accepts, or proposes another guarantee.

Guarantee must divide the job amount exactly: 10 000 / 2 000 → 5 stages. In each stage both sides lock the same amount.

Rendezvous is hardcoded (`konstruado-red-1` plus a baked Tor v3 onion). Each node starts its own `tor` process, publishes a personal hidden service, and also hosts/dials that shared onion so two machines meet without exchanging addresses. Two copies on one PC still find each other on port 17432 without waiting for Tor.

## Status

| Piece | Where it stands |
|---|---|
| Deal, board, Tor rendezvous, two data dirs | In the window. |
| Notes and extra text between the two parties | Sealed. A third person on the swarm can see the box, not the words. A live job leaves their board. |
| Hot wallet | Created when you enter. Stagenet address on the account screen. Spend key stays in `estado.json`. |
| 2-of-2 box (PedPoP / FROST) | Library and test. Not exchanged over the gossip when a job is accepted. |
| Stage funding and payout | Amounts and the two-signature gate are tested. No chain transaction: that needs real rings from a daemon, and stock `monero-wallet` 0.2 signs every input with one spend key. |

`cargo test -p xmr-joint` checks keys, the DKG, and the split. It does not talk to stagenet.

## How Monero pays a stage

Two wallets, then one shared box, then one transaction in and one transaction out. Nothing is a special on-chain multisig address. The shared box is an ordinary Monero address whose spend key was built by two people and can only be used by both.

**1. Hot wallet, one per person.** On enter, the app draws a spend scalar and sets the view key to `keccak256(spend)` reduced into the Ed25519 scalar field (the same derivation wallet2 uses). `monero-wallet` 0.2, which is the wallet layer of [monero-oxide](https://github.com/monero-oxide/monero-oxide), turns that pair into a stagenet legacy address. The spend key is never gossiped. The address is, so the other person knows where a payout should land.

**2. One 2-of-2 box when the two match.** PedPoP, the distributed key generation from the FROST paper, runs through `dkg-pedpop` and `modular-frost` (the `multisig` feature of `monero-wallet`). Three messages: a commitment, an encrypted share, and a view key the client generates and hands to the contractor. Each side stores only its own share (`ThresholdKeys`). Both end on the same group spend public key and the same address. Nobody ever holds the full spend scalar, so one person cannot move the box alone. The crate that ships on crates.io does not compile against this frost stack, so the patched copy lives in `crates/xmr-joint/vendor/dkg-pedpop`. Lab key files from earlier stagenet runs are gitignored.

**3. Funding one stage, one transaction.** The plan is a single output of `2 × guarantee` to that address. Each hot wallet contributes `guarantee`. Both inputs need their own CLSAG. If either signature is missing, the transaction is not valid, so the lock is atomic: both amounts arrive together or the chain never sees the transaction. The session in `xmr-joint` already refuses to close with one side missing. Building the real CLSAG is the missing piece. It needs decoys from a stagenet daemon, and a small oxide change (`Clsag::sign_input_with_mask` and a public `sum_output_masks`) so two machines can sign their own inputs. `SignableTransaction::sign` in 0.2 cannot do that.

**4. Paying the stage.** The pot is `2 × guarantee`. The agreed percent is how much of the *payment* the contractor keeps. Their own guarantee always comes back.

- 100% and no fee: the contractor receives the whole pot. The client receives nothing.
- 80% and no fee: the contractor receives `1.8 × guarantee` (their bond plus 80% of the payment). The client receives `0.2 × guarantee`.

That spend is one transaction with two outputs, back to the two hot wallets. Both FROST shares have to sign it (`SignableTransaction::multisig`). One share is not a transaction. The amounts and the two-share gate are tested. The chain bytes wait on the same daemon inputs as the funding transaction.

The fee, when a daemon quotes one, comes off the pot before the split. It is not wired yet.

## Build from source

Needs Rust 1.89 or newer (`monero-wallet` 0.2), GTK3, WebKitGTK 4.1, `libxdo-dev`, and the `tor` package.

```bash
sudo apt install tor libgtk-3-0 libwebkit2gtk-4.1-0 libxdo-dev
cargo test --workspace
cargo run
```

The UI is Spanish by default. Switch to English with **ES / EN** in the top bar (or in the account screen). The deal itself does not change.

State is saved in `~/.konstruado/estado.json` (override with `KONSTRUADO_DATOS`). Closing the app keeps name, role, language, jobs, and the hot-wallet spend key.

Two users on one PC need two data dirs:

```bash
KONSTRUADO_DATOS=.konstruado-dinero cargo run
KONSTRUADO_DATOS=.konstruado-chasquilla cargo run
```

Window 1: José, **Pago la obra**, Publicar. Window 2: Juan, **La construyo** — the job appears on his board.

## Linux binary (not production)

A preview build is on [Releases](https://github.com/felipebrunet/home_builder_pay/releases). Version is `0.1.0-dev` (About and the window title show that).

On Debian/Ubuntu:

```bash
sudo apt install tor libgtk-3-0 libwebkit2gtk-4.1-0 libxdo3
chmod +x construado-0.1.0-linux-x86_64-dev
./konstruado-0.1.0-linux-x86_64-dev
```

To rebuild it:

```bash
cargo build -p konstruado --release
mkdir -p dist
cp target/release/konstruado dist/konstruado-0.1.0-linux-x86_64-dev
```

How to use the app (roles, two machines, the deal) is **Help → README** inside the window, not this file.
