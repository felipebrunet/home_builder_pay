# Konstruado

Peer-to-peer construction escrow on Monero (crypto still a stub). Desktop is Dioxus.

Mandante publishes a job. Contratista accepts, or proposes another guarantee. Guarantee must divide the job amount exactly: 10 000 / 2 000 → 5 installments, 10 000 / 1 000 → 10. Each installment both sides lock the same amount.

Rendezvous is hardcoded (`konstruado-red-1`). Nodes gossip a DHT. Tor is used when a local SOCKS proxy is on 9050 or 9150; otherwise two copies on the same machine still find each other on port 17432.

```bash
cargo test --workspace
cargo run
```

Two users on one PC: run `cargo run` twice (second window binds another port and joins the first).
