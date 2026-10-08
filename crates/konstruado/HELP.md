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

Paying splits that pot by the percent already agreed. The contractor gets their own guarantee back, plus that percent of the payment. The client gets the rest. The fee comes from the client's remainder first. Both have to sign. One signature is not enough. **Accept and pay** builds the transaction. Paid is set when the scan sees it. When one side gets nothing (100 % to the contractor), its output carries 0 XMR: Monero needs two outputs, but no dust is sent to anyone.

Coins received in the last 10 blocks stay locked. That includes the stage funding: the box can only pay from block *funding block + 10*. Until then the contractor cannot report finish and the client cannot accept and pay; both see **You can mark it finished in ~N blocks (~M min, block X)** instead of the button, and the job list shows **Unlocks in ~N blocks**. The app re-reads the node's tip every minute, so the button comes back on its own. If the funding is not in a block yet, it says so. The node can still reject a publish; the error is shown.

In the wallet, **Use the maximum** sends the whole free balance: the fee is taken from the amount and there is no change output with coins in it. A normal send returns the change to your wallet.

Notes inside a job are sealed for the two people. Someone else on the network can see that a note exists and cannot read it. A job already taken leaves their board.

## Prices in USD

New jobs are priced in US dollars: the client enters the job amount and the guarantee per stage in USD (e.g. `10000` and `2000`, cents allowed). The posting form shows **USD X ≈ Y XMR at the current price**.

Stagenet XMR has no value, so the app uses the **mainnet** XMR price as a reference: CoinGecko first, Kraken (XMRUSD) if CoinGecko fails. With Tor on (Orbot on the phone, the bundled tor on the desktop) the price is fetched through Tor. The last price is cached with its time; if there is none, the app says so and a stage cannot be locked until it arrives.

Each stage's XMR is **fixed when it is locked**. Whoever proposes the lock fixes the rate (USD amount, price, source and time are saved in the job). The other side sees that price before **Confirm and fund** and accepts it by confirming; if the current price moved, a note shows the difference, and they can cancel and propose again. Both sides fund exactly the same XMR, taken from the job state, so a different local price cannot break funding. A locked stage shows **Y XMR (USD X on dd/mm hh:mm, price Z/XMR, source)**. Percent payments split that fixed XMR.

Jobs posted by older versions keep their old units (1 unit = 0.00002 XMR). Both sides need this version or later to price a job in USD.

## Node

By default the app uses the public stagenet node. In the account screen, **NODE URL** + **Save node** sets your own (LAN or Tailscale, e.g. `http://100.64.0.2:38081`) for scan, balance, funding and payout. **Use default** goes back to the public node. **Test node RPC** checks it. Node RPC never goes through Tor or the room's SOCKS proxy: a LAN/Tailscale node is reached directly, and the test says which route it used. On a phone, Orbot's VPN mode must not capture Konstruado when the node is on your LAN (Tor cannot reach private IPs); the room keeps using Orbot's SOCKS.

## Backups

**Show the 25 words** (Wallet → Backups and recovery) reveals the personal English Monero seed after a confirmation. Whoever has those words can spend the personal balance; they do **not** recover job boxes (those need the shares in the full backup). The restore height is shown so Feather or monero-wallet-cli can scan from the right block. Copy is allowed; the clipboard is cleared after about a minute if it still holds the seed. On the phone, screenshots and screen recording are blocked while the words are visible. The same section also shows the personal address and private view key (view-only: balance and incoming, not spending).

One encrypted file holds everything this device needs: the seed and its block height, your jobs and offers (archived ones too), the share of every job box, your name and role, the node URL, theme and language. Export it in **Wallet → Backups and recovery → Export full backup**. Pick a password (at least 8 characters); without it the file cannot be opened and there is no way to recover it. The file ends in `.kbak`. Keep it off this device.

The file is versioned (`KSTRBAK` header), the key comes from the password with Argon2id (64 MiB, 3 passes) and the content is sealed with XChaCha20-Poly1305. A wrong password or a damaged file is refused without writing anything.

Export again after you create or join a job and after a job box is built (the new share is only in the new backup). The wallet shows when the last full backup was made, and the board reminds you when something new is missing from it.

**Restore.** On a fresh install the first screen offers **Create a new account** or **Restore from backup**; the same restore is in Backups and recovery. The app opens the file, checks the seed and every share against its job and your role, and shows a summary. Nothing is written until everything checks out. If this device already has an account, a seed or shares, you must confirm **Replace what is on this device**; it never mixes the two. The app then restarts, moves the old data to a `previo-<date>` folder next to the profile, puts the restored data in place in one step, and scans the wallet and each box from the backup's height.

After a restore, newer progress of a deal (notes, percents, payments) comes from the other person through the room once you are both online.

Backups from 0.2.7 or earlier (25-word file, box share, job backup) still import under **Advanced: import standalone backups (0.2.7 or older)**: job backup → seed → share. The app no longer exports them separately.

The version is in **Help → About**.
