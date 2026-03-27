# DEX: Fully Onchain Order Book

High-level design for an order book DEX running entirely onchain as an Internet Computer canister.

## Overview

The DEX canister implements a central limit order book (CLOB) that matches buy and sell orders for ICRC-1/ICRC-2 token pairs. All order management, matching, and settlement happen onchain within a **single** canister.

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

- **Bids** (buy orders): sorted by price descending, then by time ascending (price-time priority).
- **Asks** (sell orders): sorted by price ascending, then by time ascending.

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

Matching runs synchronously within the `add_limit_order` update call, after the order transitions to `Open`.

### Algorithm

```
fn match_order(book, incoming_order):
    opposite_side = book.opposite(incoming_order.side)

    while incoming_order.remaining > 0:
        best = opposite_side.best_price_level()
        if best is None:
            break
        if not prices_cross(incoming_order.price, best.price, incoming_order.side):
            break

        for resting_order in best.orders:
            fill_qty = min(incoming_order.remaining, resting_order.remaining)
            execute_fill(incoming_order, resting_order, fill_qty, best.price)

            if resting_order.remaining == 0:
                remove resting_order from level

            if incoming_order.remaining == 0:
                break

        if best.orders is empty:
            remove price level

    if incoming_order.remaining > 0:
        book.insert(incoming_order)  // rest in the book
```

### Price Crossing

- A buy order crosses if `buy_price >= best_ask`.
- A sell order crosses if `sell_price <= best_bid`.
- Fills execute at the resting order's price (price improvement for the taker).

## Settlement and Token Custody

### Deposits (Escrow)

When a limit order is submitted:

1. The user must have previously called `icrc2_approve` on the relevant ledger, granting the DEX canister a sufficient allowance.
2. The canister calls `icrc2_transfer_from` to move tokens into its custody:
   - **Buy order**: escrow `price * quantity` quote tokens.
   - **Sell order**: escrow `quantity` base tokens.
3. If the transfer fails, the order stays in `Pending` and is eventually rejected.

### Balances

The canister tracks per-user balances internally:

```
balances: HashMap<(Principal, TokenId), u128>
```

- **On fill**: the buyer's quote balance decreases, base balance increases. The seller's base balance decreases, quote balance increases.
- **On cancel**: escrowed tokens are credited back to the user's balance.
- Balances are purely internal accounting; actual token transfers only happen on deposit and withdrawal.

### Withdrawals

Users call `withdraw(token, amount)` to transfer tokens from their canister balance to their wallet. The canister calls `icrc1_transfer` on the relevant ledger.

## Fee Model

- **Maker fee**: charged on fills where the order was resting in the book (can be 0 or negative for rebates).
- **Maker/taker fee**: charged on fills where the order was the incoming aggressor.
- Fees are deducted from the proceeds at fill time and accumulated in a fee account controlled by the canister admin.
- Fee rates are configurable per trading pair.

## Access Control

| Role | Capabilities |
|------|-------------|
| **Admin** (controller) | Add/remove pairs, set fees, halt trading, upgrade canister, withdraw collected fees |
| **User** (any principal) | Place orders, cancel own orders, deposit, withdraw own balance |

- No allowlisting: any principal can trade on any active pair.
- Admin operations are guarded by `ic_cdk::api::is_controller()`.

## State Persistence

Canister state must survive upgrades:

- **Pre-upgrade**: serialize the full `State` (order books, balances, pair configs, fee accounts) into stable memory using a format like CBOR or the IC stable structures library.
- **Post-upgrade**: deserialize from stable memory and reinitialize the `State`.

The `State` struct encompasses:

```
State {
    pairs: BTreeMap<PairId, TradingPair>,
    order_books: BTreeMap<PairId, OrderBook>,
    balances: HashMap<(Principal, TokenId), u128>,
    next_order_id: OrderId,
    fee_config: BTreeMap<PairId, FeeConfig>,
    collected_fees: HashMap<TokenId, u128>,
}
```

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

- **Instruction limit**: matching must complete within a single message execution (~2B instructions on a fiduciary subnet). Very large order books may require batched matching via self-calls.
- **Memory limit**: current IC stable memory limit is 400 GiB, heap is 4 GiB. Order book state should fit comfortably in heap for moderate pair counts; stable structures can be used for larger datasets.

### Async Token Transfers

ICRC-2 `transfer_from` is an inter-canister call (async). The order lifecycle handles this:

1. `add_limit_order` queues the order as `Pending` and initiates the transfer.
2. On transfer callback success, the order moves to `Open` and matching runs.
3. On transfer failure, the order is rejected.

This two-phase approach ensures the canister never matches orders with unconfirmed deposits.

### Reentrancy

Between the `transfer_from` call and its response, other update calls can execute. The canister must handle this correctly:

- Orders in `Pending` state are not visible to the matching engine.
- User balance accounting is committed only after transfer confirmation.

## Open Questions

- Should the canister support market orders (execute at best available price, no resting)?
- Should there be order expiry (good-til-cancelled vs. good-til-time)?
- Should the canister support batch order operations (place/cancel multiple orders atomically)?
- How to handle ICRC-1 ledger fee deductions in balance calculations?
- Should the canister run on a fiduciary subnet for higher instruction limits?
