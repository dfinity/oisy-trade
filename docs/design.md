# DEX: Fully Onchain Order Book

High-level design for an order book DEX running entirely onchain as an Internet Computer canister.

## Table of Contents

- [Overview](#overview)
  - [1. Deposit](#1-deposit)
  - [2. Trade](#2-trade)
  - [3. Withdrawal](#3-withdrawal)
- [Trading Pairs](#trading-pairs)
  - [Pair Management](#pair-management)
- [Order Lifecycle](#order-lifecycle)
- [Order Book Data Structure](#order-book-data-structure)
  - [Price Levels](#price-levels)
- [Matching Engine](#matching-engine)
- [Settlement and Token Custody](#settlement-and-token-custody)
  - [Deposits](#deposits)
  - [Balances](#balances)
  - [Withdrawals](#withdrawals)
- [Fee Model](#fee-model)
- [Access Control](#access-control)
- [State Persistence](#state-persistence)
  - [Event Sourcing](#event-sourcing)
  - [Upgrade Process](#upgrade-process)
  - [Benefits](#benefits)
  - [Considerations](#considerations)
- [Monitoring and Observability](#monitoring-and-observability)
  - [Query Endpoints](#query-endpoints)
  - [Metrics](#metrics)
- [IC-Specific Considerations](#ic-specific-considerations)
  - [Single-Canister Design](#single-canister-design)
  - [Synchronous Trading](#synchronous-trading)
  - [Async Deposits and Withdrawals](#async-deposits-and-withdrawals)
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
 |                          | insert order into book
 |                          | run matching engine
 |                          | credit proceeds on fills
 |<-- order_id -------------|
 |                          |
 |-- get_order_status ----->|
 |<-- status (Pending/      |
 |    Open/Filled/          |
 |    Cancelled) -----------|
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

## Trading Pairs

A trading pair consists of a base token and a quote token, each identified by their ICRC-2 ledger canister principal. Prices are expressed in quote token units per base token unit.

```
Example: ICP/ckBTC
  base  = ICP ledger principal
  quote = ckBTC ledger principal
  price = ckBTC per ICP
```

### Pair Management

- An admin (the canister controller) can add or remove trading pairs.
- Each pair has configurable parameters:
  - **Tick size**: minimum price increment.
  - **Lot size**: minimum order quantity.
  - **Status**: active, halted, or delisted.
- Orders can only be placed on active pairs.

## Order Lifecycle

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
               +--+->|  Filled   |  | Cancelled  |
                     +-----------+  +------------+
```

1. **Pending**: The order is submitted. The required funds are debited from the user's available balance (quote tokens for buys, base tokens for sells). The order is placed in a queue and an order ID is returned immediately. If the user has insufficient balance, the order is rejected.
2. **Open**: The timer-driven matching engine dequeues the order and matches it against the opposite side of the book. If the order is fully filled during this initial matching, it transitions directly to `Filled` without ever resting in the book. If only partially filled, the filled portion is settled immediately (proceeds credited to the user's available balance) and the remaining quantity rests in the book at the specified price level, where it can be matched against future incoming orders.
3. **Filled**: The order has been fully matched (either immediately or after resting in the book). Proceeds from the final fill are credited to the user's available balance.
4. **Canceled**: The user canceled the order (or it was removed due to pair delisting). Reserved tokens are returned to the user's available balance.

## Order Book Data Structure

Each trading pair maintains two sorted sides:

- **Bids** (buy orders): sorted by price descending, then by insertion order (price-FIFO priority).
- **Asks** (sell orders): sorted by price ascending, then by insertion order.

### Price Levels

Orders at the same price are grouped into a price level. Within a level, orders are matched in FIFO order.

```
Price Level (BTreeMap key = price)
  |
  +-- VecDeque<Order>  (FIFO queue at this price)
```

The full book for one side:

```
BTreeMap<Price, VecDeque<Order>>
```

This gives O(log n) insertion/removal by price and O(1) access to the best bid/ask.

## Matching Engine

Matching runs on a timer and process pending orders in the order book. This allows to potentially chunk the matching process into smaller batches.

## Settlement and Token Custody

### Deposits

Deposits are independent from order placement. The user first approves the DEX canister on the ICRC-2 ledger, then calls `deposit(token, amount)`. The canister executes `icrc2_transfer_from` to move tokens into its custody and credits the user's internal balance.

### Balances

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

### Withdrawals

Users call `withdraw(token, amount)` to transfer tokens from their available balance to their wallet. The canister calls `icrc1_transfer` on the relevant ledger. The withdrawal fails if the requested amount exceeds the user's available balance or if the ledger is not available.

## Fee Model

- **Maker fee**: charged on fills where the order was resting in the book (can be 0 or negative for rebates).
- **Taker fee**: charged on fills where the order was the incoming aggressor.
- **Platform fee**: a fee charged on every trade (both maker and taker sides), used to cover canister cycle costs and fund protocol development.
- Maker and taker fees are deducted from the proceeds at fill time. The platform fee is deducted in addition to the maker/taker fee.
- All collected fees are accumulated in a fee account controlled by the canister admin.
- Fee rates are configurable per trading pair.

## Access Control

| Role | Capabilities |
|------|-------------|
| **Admin** (controller) | Add/remove pairs, set fees, halt trading, upgrade canister, withdraw collected platform fees |
| **User** (any principal) | Place orders, cancel own orders, deposit, withdraw own balance |

- No allowlisting: any principal can trade on any active pair.
- Admin operations are guarded by `ic_cdk::api::is_controller()`.

## State Persistence

Canister state must survive upgrades. Rather than serializing and deserializing the full state, the canister uses an **append-only event log** stored in stable memory. This is the same approach used by the ckBTC, ckETH, and ckSOL minters.

### Event Sourcing

Every state-changing operation (deposit, order placement, fill, cancellation, withdrawal, pair configuration change, etc.) is recorded as an event in a persistent, append-only log in stable memory. The in-memory `State` is a derived projection of these events.

### Upgrade Process

- **Pre-upgrade**: nothing to do — events are already persisted in stable memory as they occur.
- **Post-upgrade**: the canister replays the event log from the beginning to reconstruct the full in-memory `State`.

### Benefits

- **Auditability**: the event log is a complete, ordered history of every state change.
- **Simpler upgrades**: no need to maintain serialization compatibility for the `State` struct across versions. Only the event format needs to remain stable (new event types can be added without breaking existing ones).
- **Debuggability**: bugs can be reproduced by replaying the event log.

### Considerations

- **Replay time**: as the event log grows, post-upgrade replay takes longer. This can be mitigated by periodic checkpointing (snapshotting the state and only replaying events after the checkpoint).
- **Event schema evolution**: new event types can be added freely, but existing event types should not be modified to ensure backwards-compatible replay.

## Monitoring and Observability

### Query Endpoints

| Endpoint | Description |
|----------|-------------|
| `get_order_status(OrderId)` | Status of a specific order |
| `get_order_book(PairId, depth)` | Top N price levels for both sides |
| `get_balance(Principal, TokenId)` | User's available and escrowed balance |
| `get_pairs()` | List of all trading pairs and their status |
| `get_trades(PairId, since)` | Recent trade history |

### Metrics

The canister exposes basic metrics via a query endpoint:

- Number of open orders per pair.
- Total volume traded per pair.
- Current best bid/ask per pair.
- Canister cycle balance.

## IC-Specific Considerations

### Single-Canister Design

All state lives in one canister. This avoids cross-canister call complexity for matching but means:

- **Instruction limit**: matching must complete within a single message execution (~2B instructions on a fiduciary subnet). Very large order books may require batched matching via self-calls or timer-based chunking.
- **Memory limit**: current IC stable memory limit is 400 GiB, heap is 4 GiB. Order book state should fit comfortably in heap for moderate pair counts; stable structures can be used for larger datasets.

### Synchronous Trading

Since deposits and withdrawals are separate from trading, the matching engine operates entirely on internal balances with no inter-canister calls. This means:

- **No async complexity during matching**: order placement, matching, and settlement are fully synchronous within a single update call or timer execution. There is no reentrancy concern during trading.
- **Predictable execution**: the matching engine's instruction cost depends only on the number of price levels and orders matched, not on external canister latency.

### Async Deposits and Withdrawals

Inter-canister calls (ICRC-2 `transfer_from` for deposits, ICRC-1 `transfer` for withdrawals) only occur at the edges. These calls are async, so:

- **Deposit**: the canister must handle the case where the `transfer_from` call fails (e.g., insufficient allowance, ledger unavailable). The user's internal balance is only credited after a successful transfer.
- **Withdrawal**: similarly, the available balance is debited optimistically, and if the `transfer` call fails, the balance must be restored.
- **Reentrancy**: between an async transfer call and its response, other update calls can execute. Since deposits and withdrawals only affect the initiating user's balance and do not interact with the order book, this is safe as long as the same user cannot issue concurrent deposit/withdrawal requests for the same token.

## Potential Additional Features

- **Market orders**: execute at best available price with no resting in the book.
- **Order expiry**: support good-til-time orders that are automatically canceled after a specified deadline, in addition to the default good-til-canceled.
- **Batch operations**: place or cancel multiple orders atomically in a single call, reducing round trips for market makers.
