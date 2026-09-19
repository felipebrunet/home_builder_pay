# Konstruado

Peer-to-peer construction escrow. Not a chat. There are no real payments yet (crypto is a stub).

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

State is saved in `~/.konstruado/estado.json`. Closing the app keeps name, role, language, and jobs.

## The deal

Both confirm the lock. The contractor reports finish with a percent and a short note. The other accepts or counters the percent. On pay, the receipt freezes.

If nothing is locked, abandon is one-sided. If funds are at risk, closing needs both.

Irreversible actions wait until the other person is online and their latest state has arrived (“Syncing the deal…”). Posting, export, theme and pending text do not.

From the job you can export a text or PDF record.

This build is **0.1.0-dev**. Not production.
