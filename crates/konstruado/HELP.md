# Konstruado

Peer-to-peer construction escrow on Monero stagenet. Not a chat. Not production.

## Roles

1. **Client** (Mandante) pays. Posts a job: name, work amount, suggested guarantee.
2. **Contractor** (Contratista) builds. Sees that offer on the board and accepts, or proposes another guarantee.

The guarantee must divide the job amount exactly: 10 000 / 2 000 → 5 stages. In each stage both lock the same amount.

The client opens the Tor room. The contractor only looks. You do not exchange addresses. A phone (Android app) joins the same room through Orbot.

On the phone, the network card says what is actually wrong. **Orbot no responde** means Orbot's SOCKS proxy (127.0.0.1:9050) is closed: start Orbot. **La sala no responde** means Orbot works but the room does not answer: open Konstruado on the PC that hosts it. Orbot's per-app VPN mode is fine as long as the SOCKS proxy is on.

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

**Archive this job** hides a joint job on this device only: it leaves the board and My jobs. It does not move funds and does not cut the other side off. Share and context stay on disk.

**Remove offer** is for your own offer nobody took yet. It asks once more, then withdraws the offer for everyone: it leaves your board and the contractor's too, and it does not come back when the other side reconnects. Only the client who posted it can remove it, and only while no job exists for it. Once a contractor has taken it, open the job instead.

Inside a stage, **Leave stage (this device only)** cancels a local funding or proposal. It does not move coins or sign for the other side. If the stage is already locked on-chain, the box stays.

Irreversible actions wait until the other person is online and their latest state has arrived (“Syncing the deal…”). Posting, export, theme and pending text do not.

From the job you can export a text or PDF record.

## Screens

Each screen is split into panels. On a wide window they sit in two columns; on a narrow one they stack. Backups, recovery and advanced tools are folded away under headings you can open.

Plain text with a thin left bar is help. Coloured boxes are live state: orange means something is in progress, red means it stopped and needs you, blue means you are waiting on someone. Transaction ids and addresses are in monospace.

A stage only shows the buttons that make sense right now. After the percent is accepted and the payment is being signed or waits for a block, **Accept and pay** and **Other percent** disappear and the stage shows the payment status instead. The desktop and the phone use the same rule.

The wallet keeps its scan status in one fixed line next to the title, so the page does not jump while it scans.

## Money

Open **Wallet** (top bar) or the account screen and create the stagenet wallet there. That screen shows the balance, the address to receive, and a send form. Each computer creates its own seed. Spent outputs are dropped from the balance once the node reports their key images.

The lock, per stage, works like this. Both people send the same guarantee into one shared wallet. That wallet is a normal stagenet address, but its spend key was built by the two of them and neither holds it whole. **Confirm and fund** builds one transaction. The stage stays locked only after this app sees that transaction in a block.

If the node rejects the funding (stale decoys or a spent output), press **Start funding again**. It clears the stuck session on both sides and builds fresh rings. The job is kept. Both have to be online for a moment so the other side also clears.

Paying splits that pot by the percent already agreed. The contractor gets their own guarantee back, plus that percent of the payment. The client gets the rest. The fee comes from the client's remainder first. Both have to sign. One signature is not enough. **Accept and pay** builds the transaction. Paid is set when the scan sees it.

Coins received in the last 10 blocks stay locked. The node can still reject a publish; the error is shown.

Notes inside a job are sealed for the two people. Someone else on the network can see that a note exists and cannot read it. A job already taken leaves their board.

## Node

By default the app uses the public stagenet node. In the account screen, **NODE URL** + **Save node** sets your own (LAN or Tailscale, e.g. `http://100.64.0.2:38081`) for scan, balance, funding and payout. **Use default** goes back to the public node. **Test node RPC** checks it. Node RPC never goes through Tor or the room's SOCKS proxy: a LAN/Tailscale node is reached directly, and the test says which route it used. On a phone, Orbot's VPN mode must not capture Konstruado when the node is on your LAN (Tor cannot reach private IPs); the room keeps using Orbot's SOCKS.

## Backups

Three separate things. Keep all three.

- **Seed** (**Save the 25 words**): your personal wallet. The backup also stores the block height, so **Restore the 25 words** scans from there forward. Old seed files without a height scan the recent window; use **Scan 200 blocks further back** if needed.
- **Share** (**Save the box share**, one per job): your half of the job's shared box. It does not come from the seed. Without it the box cannot sign.
- **Job backup** (**Save job backup**): your jobs, offers and roles. A share can only be restored once its job is in the profile.

After reinstalling, restore in this order: **job backup → seed → share** (**Restore job backup**, **Restore the 25 words**, **Restore a share**). Then **Refresh balance**. A job backup can be older than the other side; the deal state catches up when you are both online.

The version is in **Help → About**.
