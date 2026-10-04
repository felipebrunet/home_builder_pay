# Konstruado

Peer-to-peer construction escrow. Not a chat. The deal works. Money is not on the chain yet.

## Roles

1. **Client** (Mandante) pays. Posts a job: name, work amount, suggested guarantee.
2. **Contractor** (Contratista) builds. Sees that offer on the board and accepts, or proposes another guarantee.

The guarantee must divide the job amount exactly: 10 000 / 2 000 → 5 stages. In each stage both lock the same amount.

The client opens the Tor room. The contractor only looks. You do not exchange addresses.

## Language

Spanish by default. Switch to English with **ES / EN** in the top bar, or in the account screen. The deal itself does not change.

## Two people

Both need the `tor` package. The client starts first and waits until the room is open. Then the contractor.

On one PC, use two data folders:

```
KONSTRUADO_DATOS=.konstruado-dinero ./konstruado
KONSTRUADO_DATOS=.konstruado-chasquilla ./konstruado
```

State is saved in `~/.konstruado/estado.json`. Closing the app keeps name, role, language, jobs, and the hot-wallet spend key.

## The deal

Both confirm the lock. The contractor reports finish with a percent and a short note. The other accepts or counters the percent. On pay, the receipt freezes.

If nothing is locked, abandon is one-sided. If funds are at risk, closing needs both.

Irreversible actions wait until the other person is online and their latest state has arrived (“Syncing the deal…”). Posting, export, theme and pending text do not.

From the job you can export a text or PDF record.

## Money

Open **Wallet** (top bar) or the account screen and create the stagenet wallet there. That screen shows the balance, the address to receive, and a send form. Save the 25 words from the account screen. Each computer creates its own seed.

The lock, per stage, works like this. Both people send the same guarantee into one shared wallet. That wallet is a normal stagenet address, but its spend key was built by the two of them and neither holds it whole. **Confirm and fund** builds one transaction. The stage stays locked only after this app sees that transaction in a block.

Paying splits that pot by the percent already agreed. The contractor gets their own guarantee back, plus that percent of the payment. The client gets the rest. The fee comes from the client's remainder first. Both have to sign. One signature is not enough. **Accept and pay** builds the transaction. Paid is set when the scan sees it.

The public stagenet node can still reject a publish. The error is shown. Coins received in the last 10 blocks stay locked. The balance scan starts 40 blocks back.

Notes inside a job are sealed for the two people. Someone else on the network can see that a note exists and cannot read it. A job already taken leaves their board.

This build is **0.1.0-dev**. Not production.
