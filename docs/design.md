# DEX: Fully Onchain Order Book

High-level design for an order book DEX running entirely onchain as an Internet Computer canister.

## Table of Contents

- [Overview](#overview)
- [Trading](#trading)
- [Balances](#balances)
- [Architecture](#architecture)
- [Monitoring](#monitoring)
- [Potential Additional Features](#potential-additional-features)

## Overview

The DEX canister implements a central limit order book (CLOB) that matches buy and sell orders for ICRC-2 token pairs. All order management, matching, and settlement happen onchain within a **single** canister.

There are three distinct flows:

### 1. Deposit

The user moves tokens from their wallet into the DEX canister. This is a prerequisite for trading.

```
User                    DEX Canister                  ICRC-2 Ledger
 |                          |                              |
 |-- icrc2_approve ---------------------------------------->|
 |                          |                              |
 |-- deposit(token, amt) -->|                              |
 |                          |-- icrc2_transfer_from ------>|
 |                          |   (user -> DEX canister)     |
 |                          |                              |
 |                          | credit user's internal       |
 |                          | balance                      |
 |<-- block_index ----------|                              |
```

### 2. Trade

The user places orders using their deposited balance. Matching and settlement are purely internal bookkeeping — no token transfers occur, no asynchronous calls.

```
User                    DEX Canister
 |                          |
 |-- add_limit_order ------>|
 |                          | debit user's available balance
 |                          | queue order for matching
 |<-- order_id -------------|
 |                          |
 |          (timer fires)   |
 |                          | matching engine processes queue
 |                          | insert/match against book
 |                          | credit proceeds on fills
 |                          |
 |-- get_order_status ----->|
 |<-- status (Pending/      |
 |    Open/Filled/          |
 |    Canceled) ------------|
```

### 3. Withdrawal

The user moves tokens from the DEX canister back to their wallet.

```
User                    DEX Canister                  ICRC-2 Ledger
 |                          |                              |
 |-- withdraw(token, amt) ->|                              |
 |                          | debit user's internal        |
 |                          | balance                      |
 |                          |-- icrc1_transfer ----------->|
 |                          |   (DEX canister -> user)     |
 |<-- ok -------------------|                              |
```

This separation means the matching engine never waits on async inter-canister calls. Token transfers only happen at the edges (deposit/withdrawal), while trading operates entirely on internal balances.

### Access Control

| Role                       | Capabilities                                                                                  |
|----------------------------|-----------------------------------------------------------------------------------------------|
| **Admin** (controller)     | Add/remove pairs, set fees, halt trading, upgrade canister, withdraw collected platform fees   |
| **User** (any principal)   | Place orders, cancel own orders, deposit, withdraw own balance                                |

- No allowlisting: any principal can trade on any active pair.
- Admin operations are guarded by `ic_cdk::api::is_controller()`.

## Trading

### Trading Pairs

A trading pair consists of a base token and a quote token, each identified by their ICRC-2 ledger canister principal. Prices are expressed in quote token units per base token unit.

```
Example: ICP/ckBTC
  base  = ICP ledger principal
  quote = ckBTC ledger principal
  price = ckBTC per ICP
```

#### Pair Management

- An admin (the canister controller) can add or remove trading pairs.
- Each pair has configurable parameters:
  - **Tick size**: minimum price increment.
  - **Lot size**: minimum order quantity.
  - **Status**: active, halted, or delisted.
- Orders can only be placed on active pairs.

### Order Lifecycle

Since deposits are a separate step, the user's balance is already available when placing an order. Orders are not matched immediately — they are queued and processed asynchronously by a timer-driven matching engine.

```
                 add_limit_order
                      |
                      v
               +------------+
               |   Pending   |  <-- queued, awaiting processing
               +------------+
                      |
               timer fires,
               matching engine
               processes queue
                      |
                      v
               +------------+
               |    Open     |  <-- resting in the book (unfilled remainder)
               +--+----------+
               ^       |      \
               |     filled   cancel_order
          partial      |          |
          fill         v          v
               |     +-----------+  +------------+
               +--+->|  Filled   |  | Canceled   |
                     +-----------+  +------------+
```

1. **Pending**: The order is submitted. The required funds are debited from the user's available balance (quote tokens for buys, base tokens for sells). The order is placed in a queue and an order ID is returned immediately. If the user has insufficient balance, the order is rejected.
2. **Open**: The timer-driven matching engine dequeues the order and matches it against the opposite side of the book. If the order is fully filled during this initial matching, it transitions directly to `Filled` without ever resting in the book. If only partially filled, the filled portion is settled immediately (proceeds credited to the user's available balance) and the remaining quantity rests in the book at the specified price level, where it can be matched against future incoming orders.
3. **Filled**: The order has been fully matched (either immediately or after resting in the book). Proceeds from the final fill are credited to the user's available balance.
4. **Canceled**: The user canceled the order (or it was removed due to pair delisting). Reserved tokens are returned to the user's available balance.

### Order Book Data Structure

Each trading pair maintains an order book stored on the **heap** which consists of two sorted sides:

- **Bids** (buy orders): 
    - sorted by price descending, then by insertion order (price-FIFO priority).
    - Implemented as `BTreeMap<Reverse<Price>, VecDeque<Order>>`
- **Asks** (sell orders): 
    - sorted by price ascending, then by insertion order.
    - Implemented by `BTreeMap<Price, VecDeque<Order>>`

This gives O(log n) insertion/removal by price and O(1) access to the best bid/ask.

#### Memory Estimates

An `Order` instance contains:
- an ID (`u64`): 8 bytes
- a side (enum with 2 variants buy/sell): 1 byte
- a price (`u64`): 8 bytes
- a quantity (`u64`): 8 bytes

totaling approximately **25 bytes** per order. This could be reduced further to 17 bytes by removing the price from the `Order` struct given that it's already used as key in the buy/sell orders. The following estimates upper bound the memory taken by an order by 32 bytes.

Per-price-level overhead consists of a `BTreeMap` node (~64 bytes per entry, amortized across B-tree nodes) plus a `VecDeque` header (~48 bytes including pointer, length, and capacity). The `VecDeque` backing buffer grows as needed and may over-allocate by up to 2x.

Estimated memory per order book side:

| Component                                                | Memory                                    |
|----------------------------------------------------------|-------------------------------------------|
| Per order                                                | ~32 bytes                                 |
| Per price level (`BTreeMap` entry + `VecDeque` header)   | ~112 bytes                                |
| Per order (`VecDeque` buffer slot)                       | ~32 bytes (up to 2x with over-allocation) |

**Real-world reference: Binance ICP/BTC order book snapshot** (retrieved via `GET /api/v3/depth?symbol=ICPBTC&limit=5000`):

- 135 bid price levels, 1,310 ask price levels (1,445 total).
- Binance aggregates all orders at a price into a single entry. Assuming ~10 individual orders per price level on a DEX (no aggregation), the estimated memory for this pair would be:

```
1,445 levels × 112 B  +  14,450 orders × 64 B  ≈  1 MiB
```

This fits comfortably within the 4 GiB Wasm heap. Even with 100 trading pairs of similar depth, the total book state would remain well under 200 MiB.

### Matching Engine

Matching runs on a timer and processes pending queued orders, which makes it possible to chunk the matching process into smaller batches.

### Fee Model

- **Maker fee**: charged on fills where the order was resting in the book (can be 0 or negative for rebates).
- **Taker fee**: charged on fills where the order was the incoming aggressor.
- **Platform fee**: a fee charged on every trade (both maker and taker sides), used to cover canister cycle costs and fund protocol development.
- Maker and taker fees are deducted from the proceeds at fill time. The platform fee is deducted in addition to the maker/taker fee.
- All collected fees are accumulated in a fee account controlled by the canister admin.
- Fee rates are configurable per trading pair.

## Balances

The canister tracks per-user balances internally. Each user's balance for a given token is split into:

- **Available**: funds that can be used to place new orders or be withdrawn.
- **Reserved**: funds locked by open orders (quote tokens for bids, base tokens for asks).

Balance transitions:

- **On order placement**: the required amount moves from available to reserved.
- **On fill**: the reserved amount of the filled side is consumed, and the proceeds are credited to the available balance of the corresponding token. For example, when a buy order fills, the reserved quote tokens are consumed and the base tokens are credited as available.
- **On cancel**: reserved tokens move back to available.
- **On deposit**: available balance increases.
- **On withdrawal**: available balance decreases.

Actual token transfers (inter-canister calls) only happen during deposits and withdrawals. All trading operations are purely internal bookkeeping.

### Deposits

Deposits are independent from order placement. The user first approves the DEX canister on the ICRC-2 ledger, then calls `deposit(token, amount)`. The canister executes `icrc2_transfer_from` to move tokens into its custody and credits the user's internal balance.

### Withdrawals

Users call `withdraw(token, amount)` to transfer tokens from their available balance to their wallet. The canister calls `icrc1_transfer` on the relevant ledger. The withdrawal fails if the requested amount exceeds the user's available balance or if the ledger is not available.

### Memory Estimates

Assume **1M users with non-zero balances**, and that almost all quantities fit in a `u128` (per [#59](https://github.com/dfinity/dex/pull/59): `Quantity` is a stack-allocated `(u128, u128)` — 32 B — encoded as a plain CBOR integer when the value fits in `u64` and as a PosBignum Tag 2 otherwise). Balances are stored token-first (per [#60](https://github.com/dfinity/dex/pull/60): `BTreeMap<TokenId, BTreeMap<Principal, Balance>>`).

Per-entry sizes:

| Item                       | Heap  | CBOR                              |
|----------------------------|-------|-----------------------------------|
| `TokenId` / `Principal`    | ~30 B | ~32 B                             |
| `Balance` (2 × `Quantity`) | 64 B  | ~40 B (u128 values via PosBignum) |

Totals at 1M users, varying the average number of tokens held per user. The outer `TokenId` map is dominated by a handful of listed tokens, so nearly all space is in the inner `Principal → Balance` maps:

| Tokens / user | Inner entries | Heap    | CBOR snapshot |
|---------------|---------------|---------|---------------|
| 2             | 2M            | ~220 MB | ~150 MB       |
| 5             | 5M            | ~550 MB | ~370 MB       |
| 10            | 10M           | ~1.1 GB | ~740 MB       |

Fits within the 4 GiB heap limit even at 10 tokens/user. The CBOR snapshot at 5 tokens/user is ~5 900 stable-memory pages — well within the 2 TiB stable budget, but large enough that the cost of serializing balances at every upgrade needs to be measured, not assumed.

## Order History

Every order submitted to the DEX is recorded in an append-only map keyed by `OrderId`. Each `OrderRecord` captures:

- **owner**: the `Principal` that submitted the order.
- **side**: `Buy` or `Sell`.
- **price**: the limit price as a `u64`.
- **quantity**: the original submission size as a `Quantity`.
- **status**: the current lifecycle state — `Pending`, `Open`, `Filled`, or `Canceled`.

A record is inserted once at submission and its `status` field is updated as the order transitions through its lifecycle. The trading pair is not stored — it is derivable from the `OrderBookId` embedded in the `OrderId` via the canister's trading-pair registry.

The history exists for a single purpose: serving the `get_order_status(order_id)` query so clients that have lost track of a submission can recover its outcome.

### Memory Estimates

Per-record size, assuming `Quantity` encodes mostly in the `u128` range (see [Balance memory estimates](#memory-estimates)):

| Item                             | Heap                    | CBOR                       |
|----------------------------------|-------------------------|----------------------------|
| `OrderId` (key, `(u64, u64)`)    | 16 B                    | ~18 B                      |
| `owner: Principal`               | ~30 B                   | ~32 B                      |
| `price: u64`                     | 8 B                     | ~5 B                       |
| `quantity: Quantity`             | 32 B                    | ~20 B (u128 via PosBignum) |
| `side` + `status`                | 2 B                     | ~2 B                       |
| Overhead                         | ~15 B (BTree amortized) | ~5 B (CBOR map)            |
| **Per record**                   | **~100 B**              | **~85 B**                  |

Applying the [expected load](#expected-load) — 0.7 orders/s steady-state, 40 orders/s sustained during peak-hour events:

| Horizon                             | Orders | Heap    | CBOR    |
|-------------------------------------|--------|---------|---------|
| 1 day @ steady                      | 60 K   | ~6 MB   | ~5 MB   |
| 1 yr @ steady                       | 22 M   | ~2.2 GB | ~1.9 GB |
| 1 yr @ steady + ~50 peak hours      | ~30 M  | ~3.0 GB | ~2.6 GB |
| 2 yr @ steady                       | 44 M   | ~4.4 GB | ~3.7 GB |

`order_history` grows monotonically with the total number of orders ever placed. It fits comfortably on the heap for the first year of steady-state traffic but crosses the 4 GiB heap limit at around two years. A retention or archival policy is required unless the history is stored in stable memory, where the 2 TiB per-subnet budget dominates.

## Architecture

### Single-Canister Design

All state lives in one canister. This avoids cross-canister call complexity but one remains bound to the [canister resource limits](https://docs.internetcomputer.org/building-apps/canister-management/resource-limits):

- **Instruction limit**: matching must complete within a single message execution (~40B instructions on a fiduciary subnet). Very large order books may require to chunk the matching, which is possible because the matching is done on a timer.
- **Memory limit**: 
    - heap: limited to 4 GiB per canister. This can be a problem if an order book becomes too big or there are too many trading pairs (see the section on memory estimates).
    - stable memory: limited to 2 TiB, shared across the whole subnet. This can be a problem for the replayable event log stored in stable memory.

### Synchronous Trading

Since deposits and withdrawals are separate from trading, the matching engine operates entirely on internal balances with no inter-canister calls. This means:

- **No async complexity during matching**: order placement, matching, and settlement are fully synchronous within a single update call or timer execution. There are no reentrancy concerns during trading.
- **Predictable execution**: the matching engine's instruction cost depends only on the number of price levels and orders matched, not on external canister latency.

### Async Deposits and Withdrawals

Inter-canister calls (ICRC-2 `transfer_from` for deposits, ICRC-1 `transfer` for withdrawals) only occur at the edges. These calls are async, so:

- **Deposit**: the canister must handle the case where the `transfer_from` call fails (e.g., insufficient allowance, ledger unavailable). The user's internal balance is only credited after a successful transfer.
- **Withdrawal**: similarly, the available balance is debited optimistically, and if the `transfer` call fails, the balance must be restored.
- **Reentrancy**: between an async transfer call and its response, other update calls can execute. Since deposits and withdrawals only affect the initiating user's balance and do not interact with the order book, this is safe provided the implementation enforces that the same user cannot have multiple in-flight deposit/withdrawal requests for the same token (e.g., via a per-(user, token) in-flight guard).

### Main Endpoints

**Update calls** (state-changing):

- **`deposit(token, amount)`**: transfers tokens into the canister via `icrc2_transfer_from`. Credits the user's available balance on success. Involves one async inter-canister call. Time: O(1) for balance bookkeeping, dominated by the async ledger call.
- **`withdraw(token, amount)`**: transfers tokens from the canister to the user's wallet via `icrc1_transfer`. Debits the user's available balance. Time: O(1) for balance bookkeeping, dominated by the async ledger call.
- **`add_limit_order(pair, side, price, quantity)`**: validates the order (balance, tick/lot size), debits the required amount from the user's available balance to reserved, enqueues the order, and returns an order ID. Fully synchronous — no inter-canister calls. Time: O(1). Memory: O(1) for the queued order.
- **`cancel_order(order_id)`**: removes a resting order from the book and returns reserved tokens to the user's available balance. Time: O(log p + k) where p is the number of price levels and k is the queue depth at the order's price level (to find and remove the order from the `VecDeque`). Memory: frees the canceled order.

**Timer-driven** (internal):

- **Matching engine**: dequeues pending orders and matches each against the opposite side of the book. Time per order: O(f · log p) where f is the number of fills and p is the number of price levels (each fill may remove a level, requiring a `BTreeMap` operation). Memory: O(f) for the fill records produced; net memory change depends on whether the order rests (adds to the book) or fills (removes from the book).

**Query calls** (read-only):

- **`get_order_status(order_id)`**: returns the current status of an order. Time: O(1) with an order-ID-indexed map.
- **`get_balance(token)`**: returns the caller's available and reserved balances. Time: O(1).

### Expected Load

Based on Binance ICP/USDT data (the most active ICP pair), two numbers drive the design:

- **Steady-state: ~0.7 trades/sec** (avg 2,600 trades/hour). A single canister handles this trivially.
- **Peak: ~40 trades/sec** sustained over a full hour during market-wide events, with p99 at ~4 trades/sec.

Peak load is the binding constraint. The timer-driven matching engine naturally absorbs bursts by queuing orders and processing them in batches — the exact mechanism to sustain peak load will be addressed in DEFI-2724.

See [`docs/trading_data/README.md`](trading_data/README.md) for the full analysis.

### Upgrade Strategy

Canister state must survive upgrades. Different data structures have different cost profiles in terms of instructions and memory. The strategy is therefore chosen per data structure rather than applied uniformly.

#### Core Data Structures

Three hot-path data structures must be preserved across upgrades:

- **`order_books`** — see [Order Book Data Structure](#order-book-data-structure).
- **`balances`** — see [Balances](#balances).
- **`order_history`** — see [Order History](#order-history).

Each structure can live either in stable memory or on the heap. The two placements are described in the following subsections and evaluated per-structure in the later sections.

#### Stable Memory

Stored typically in a `StableBTreeMap`; per-op durability at the cost of a stable-memory tax on every access.

- Pros:
  - Per-op durability.
  - Zero upgrade cost.
  - Size bounded only by the 2 TiB per-subnet stable budget.
- Cons:
  - Roughly ~20× slower per operation (see [#57](https://github.com/dfinity/dex/pull/57)), driven by:
    - Every tree hop crosses the Wasm-to-host boundary to read or write stable memory — orders of magnitude more expensive than a heap pointer chase.
    - Keys and values are serialized bytes, so each access pays a decode (and, on writes, a re-encode) of the full value.
    - No in-place mutation: unlike heap `BTreeMap::get_mut`, `StableBTreeMap` exposes only `get` / `insert`, so even a single-field update (e.g. incrementing a `Balance`) requires a full read-modify-write of the entire value.
  - Stable-memory layout is harder to evolve than a heap struct.

#### Heap

The structure lives in memory for fast access; a dedicated mechanism preserves it across upgrades. Two variants:

- **Event replay**: state-changing events are appended to a stable log and replayed at `post_upgrade` to reconstruct the in-memory state.
  - Pros:
    - Hot path at heap speed.
    - Free audit trail.
    - Survives `pre_upgrade` trap since events are already persisted.
  - Cons:
    - One stable write per state-changing operation.
    - Replay cost grows with log size and may exceed the 300B instruction limit for `pre_upgrade`/`post_upgrade`:
        - If the log contains many events.
        - If replaying some events is costly in instruction terms.
    - Event schema must remain backwards-compatible.
- **Pre Upgrade**: the full structure is serialized to stable memory at `pre_upgrade` and restored at `post_upgrade`.
  - Pros:
    - Zero per-op cost.
    - Upgrade cost is bounded by current state size, not lifetime traffic.
  - Cons:
    - Footgun: `pre_upgrade` trap blocks the upgrade and can result in data loss (upgrade skipping `pre_upgrade`).
    - Upgrade cost is linear in state size and can exceed the 300 B `pre_upgrade`/`post_upgrade` budget for large or unbounded structures.
    - Serialization schema must remain backwards-compatible.

#### Summary

At-a-glance viability of each placement per data structure (🟢 = viable choice, 🟠 = possible but expensive, 🔴 = ruled out).

| Data structure  | Stable memory                                                                                                               | Heap + event replay                                                                                                                                                                                     | Heap + pre-upgrade snapshot                                                                                                                                                                                                                                                                                 |
|-----------------|-----------------------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `order_books`   | 🔴 21×-29x the number of instructions for matching as when done on the heap ([#57](https://github.com/dfinity/dex/pull/57)) | 🔴 replay complexity is O(matching) in the worst-case, e.g. insert resting orders which involves only tree operation that would need to be replayed.<br>Over 22 M events / yr exceeds the upgrade budget | 🟢 Once per upgrade per order book: 60M instructions for Binance ICP/USDT ([#58](https://github.com/dfinity/dex/pull/58)), about 2 bps of the 300 B instructions upgrade budget.<br>Even snapshotting 1_000 order books would be ~60 B instructions (~20% of the budget)                                   |
| `balances`      | 🟠 15× the number of instructions for settling as when done on the heap ([#57](https://github.com/dfinity/dex/pull/57)).                                                   | 🔴 replay complexity is O(settling): need to update balances according to the fills.<br>Over 22 M events / yr exceeds the upgrade budget                                                                    | 🔴 Once per upgrade per traded token: 150M for balances needed for Binance ICP/USDT ([#58](https://github.com/dfinity/dex/pull/58)).<br>Doesn't scale well:<br> - Long tail: many users will have various tokens with small balances.<br> - Adding a new trading pair adds 2 token balances to snapshot |
| `order_history` | 🟢 no efficiency concern                                                                                                    | 🔴 heap limit crossed at ~2 yr; replay blows the 300 B budget                                                                                                                                           | 🔴 snapshot blows the 300 B budget at ~22 M records                                                                                                                                                                                                                                                         |

### Auditability

* Every state-changing operation (deposit, order placement, fill, cancellation, withdrawal, pair configuration change, etc.) is recorded as an event in a persistent, append-only log in stable memory.
* Bugs can be reproduced by replaying the event log (may take a significant amount of time)

## Monitoring

### Metrics

The canister exposes basic metrics via a query endpoint:

- Number of open orders per pair.
- Total volume traded per pair.
- Current best bid/ask per pair.
- Canister cycle balance.

## Potential Additional Features

- **Market orders**: execute at best available price with no resting in the book.
- **Order expiry**: support good-til-time orders that are automatically canceled after a specified deadline, in addition to the default good-til-canceled.
- **Batch operations**: place or cancel multiple orders atomically in a single call, reducing round trips for market makers.
