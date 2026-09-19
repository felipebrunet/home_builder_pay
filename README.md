# Konstruado

For a new Grok session, read `CONTEXTO.md` first.

Peer-to-peer construction escrow on Monero (crypto still a stub). Desktop is Dioxus.

The two people do not see each other like a chat. Roles:

1. **Mandante** (pays) publishes a job: name, work amount, suggested guarantee.
2. **Contratista** (builds) sees that offer on the board and accepts, or proposes another guarantee.

Guarantee must divide the job amount exactly: 10 000 / 2 000 → 5 installments, 10 000 / 1 000 → 10. Each installment both sides lock the same amount.

Rendezvous is hardcoded (`konstruado-red-1` plus a baked Tor v3 onion). Each node starts its own `tor` process, publishes a personal hidden service, and also hosts/dials that shared onion so two machines meet without exchanging addresses. Two copies on one PC still find each other on port 17432 without waiting for Tor.

```bash
cargo test --workspace
cargo run
```

The UI is Spanish by default. Switch to English with **ES / EN** in the top bar (or in the account screen). The deal itself does not change.

State is saved in `~/.konstruado/estado.json` (override with `KONSTRUADO_DATOS`). Closing the app keeps name, role, language, and jobs.

Two users on one PC need two data dirs:

```bash
KONSTRUADO_DATOS=.konstruado-dinero cargo run
KONSTRUADO_DATOS=.konstruado-chasquilla cargo run
```

Window 1: José, **Pago la obra**, Publicar. Window 2: Juan, **La construyo** — the job appears on his board.

## Linux binary (not production)

Version is `0.1.0-dev`. About and the window title show that. Build and name it:

```bash
cargo build -p konstruado --release
mkdir -p dist
cp target/release/konstruado dist/konstruado-0.1.0-linux-x86_64-dev
```

On the other machine (Debian/Ubuntu):

```bash
sudo apt install tor libgtk-3-0 libwebkit2gtk-4.1-0 libxdo3
chmod +x construado-0.1.0-linux-x86_64-dev
./konstruado-0.1.0-linux-x86_64-dev
```

Upload as a GitHub **pre-release**:

```bash
gh release create v0.1.0-dev --prerelease --title "0.1.0-dev" --notes "Linux x86_64 preview. Needs tor, GTK3, WebKitGTK 4.1, libxdo. Not production." dist/konstruado-0.1.0-linux-x86_64-dev
```
