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

Entering creates a stagenet hot wallet. The address is on the account screen. The spend key stays in the data folder.

The lock, per stage, is meant to work like this. Both people send the same guarantee into one shared wallet. That wallet is a normal Monero address, but its spend key was built by the two of them and neither holds it whole. The funding is one transaction: both amounts arrive together, or the transaction does not exist.

Paying splits that pot by the percent already agreed. The contractor always gets their own guarantee back, plus that percent of the payment. At 100% the contractor receives everything. At 80% they receive 1.8 guarantees and the client receives the remaining 0.2. Both have to sign. One signature is not enough.

This build does not broadcast those transactions. The keys and the amounts are in place. The chain step is not.

Notes inside a job are sealed for the two people. Someone else on the network can see that a note exists and cannot read it. A job already taken leaves their board.

This build is **0.1.0-dev**. Not production.
