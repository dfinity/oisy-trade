---
id: DEFI-2861
title: Auto-routing orders
tags: [orders, routing, matching-engine]
---

# Auto-routing orders

## Motivation

We want to maximize the number of *user-perceived* trading pairs without listing every
pair explicitly — a connected graph of tokens rather than a full mesh. In production the
graph is a **star**: every real pair is a spoke quoted in the hub token **ckUSDT**
(`ICP/ckUSDT`, `ckBTC/ckUSDT`, `ckETH/ckUSDT`, …). A pair like `ICP/ckUSDT` is therefore
*direct* and needs no routing; the pairs that need routing are **spoke↔spoke**, e.g.
`ICP/ckBTC`, reachable by hopping through the hub.

Auto-routing lets a user place an order on such a synthetic spoke↔spoke pair as if it
were listed directly. Internally the order is executed as two real orders that meet at
the hub. For a **buy** of `ICP/ckBTC` (user provides ckBTC, wants ICP):

1. sell the user's ckBTC for ckUSDT on `ckBTC/ckUSDT`, then
2. buy ICP with that ckUSDT on `ICP/ckUSDT`.

The hard constraint, from the ticket: this must be **all-or-nothing**. The user must
never be left holding the transit token (ckUSDT) — a token they neither provided nor
requested.

## Prior art — the routing landscape

Routing a non-listed pair through an intermediate is well-established; it appears in three
distinct shapes on existing venues, and our design sits between them.

- **CLOB liquidity routing — Binance Smart Order Routing (SOR).** Fills an order from other
  order books with the **same base** and **interchangeable (1:1-pegged) quote assets** — a
  `BTCUSDT` order can pull from `BTCUSDC`/`BTCUSDP`, and you always receive the quote asset
  of the symbol you ordered. This is the **dual** of our topology (SOR = same base, swap
  among parity quotes; ours = common hub quote, different bases). Crucially SOR has **no
  transit residual**, because its quotes are 1:1 by assumption — there is no price
  conversion across the hop. Coinbase and Kraken expose no comparable cross-book SOR.
- **Any-to-any swap — Convert (Binance / Coinbase / Kraken).** Pick any two assets and get a
  firm quote even with no direct market. It is **RFQ/quote-based, not order-book matching**:
  a market maker gives one guaranteed price and absorbs the routing and the dust internally.
  Closest *UX* analog to "trade a non-listed pair," but architecturally unlike ours (no two
  CLOB legs, no user-visible change).
- **DEX aggregator multi-hop routers — 1inch (Pathfinder), Jupiter (Metis), Uniswap
  auto-router.** One signed transaction executes a **multi-hop, atomic** route through an
  **intermediate token** (A→USDC→B) when it beats the direct path. This is the closest
  *architectural* twin and the most relevant, since we are a DEX — but they route across
  **AMM pools (continuous amounts)**, so slippage silently absorbs rounding.

**Where ours differs.** We synthesize a cross by atomically matching two separate **CLOBs**
and returning the user **change in the hub token**. That places us between SOR (CLOB, but
parity quotes ⇒ no residual) and the aggregators (multi-hop atomic, but AMM ⇒ no discrete
residual). The discrete-lot **transit residual** is the genuinely novel wrinkle none of the
analogs had to solve (see [The transit-residual problem](#the-transit-residual-problem)).
Two of our choices do have precedent, though: "you receive the symbol's quote asset" (SOR) ≈
our "requested token + hub change," and FOK-kill on insufficient liquidity ≈ SOR's
IOC/MARKET immediate-expire.

References: [Binance SOR FAQ](https://developers.binance.com/docs/binance-spot-api-docs/faqs/sor_faq),
[Binance SOR launch](https://www.binance.com/en/support/announcement/binance-spot-launches-smart-order-routing-sor-experimental-trading-feature-for-api-users-0fc3560788c74e6290ca9dd285974b32),
[Binance Spot vs Convert](https://www.binance.com/en/square/post/11497424572602),
[DEX aggregators (1inch/Jupiter/ParaSwap), 2026](https://www.dextools.io/tutorials/what-is-a-dex-aggregator-1inch-jupiter-paraswap-guide-2026),
[Jupiter Metis routing](https://eco.com/support/en/articles/14801182-jupiter-aggregator-how-solana-s-dex-router-works).

## Requirements

The headline contract is all-or-nothing on the user's target, with exactly two outcomes:

1. **Filled** — the trade goes through for the full specified quantity at a price within
   the user's limit (both legs fill, fill-or-kill). The user receives the requested token
   and *may also* receive a small amount of **change in the hub token (ckUSDT)** — a token
   other than the one requested.
2. **Killed** — the trade does not go through at all: zero effect, and the user holds no
   residual token of any kind.

- **R1 — Synthetic spoke↔spoke routing.** A user can place an order on a spoke↔spoke pair
  that is not listed directly, routed through the hub token in a single hop (two legs).
- **R2 — All-or-nothing on the target (FOK legs).** A routed order either (a) **fully**
  fills the user's specified quantity at an effective price within the user's limit — both
  legs fully fill, fill-or-kill — or (b) has **zero** effect: no fills, no resting trace on
  either book, and the placement reservation fully released. There is no partial fill.
- **R3 — Possible change in the hub token; never an unrequested volatile holding.** On the
  fill outcome (a), the user receives the full requested quantity of the destination token
  and may additionally receive **bounded change in the hub token (ckUSDT)** — the leftover
  transit, strictly less than one funding-leg lot. The user never ends with an unrequested
  *volatile* token and never with a resting position. On the kill outcome (b), the user
  holds **no residual token of any kind**.
- **R4 — Transit appears only as benign hub balance.** The kill-or-commit decision is
  atomic; once committed the route always completes. Settlement may be deferred and chunked,
  so the user **may transiently hold the hub token (ckUSDT)** before the route fully settles
  — accepted, because ckUSDT is the stable hub / unit of account. The user never holds an
  unrequested *volatile* token at any point, and never a resting position.
- **R5 — Always a taker; never rests.** A routed order is a taker on both legs and never
  rests in either book (resting one leg would strand the user mid-route).
- **R6 — Atomic decision in one message; settlement chunked.** The kill-or-commit decision
  — the read-only two-leg plan plus the in-heap book mutation that commits it — must fit a
  single message; the subsequent balance settlement reuses the existing chunked
  settling-event drain and need not fit one message. The worst-case plan+commit is
  benchmarked against the per-message instruction limit.
- **R7 — Transparent entry point.** A routed order is placed through the *same* endpoint and
  request type as a direct order: the user submits on the (synthetic) pair and receives one
  order id; the canister auto-detects whether the pair is direct or routable (no routing
  flag, no separate endpoint). A routed order is always all-or-nothing — it cannot rest —
  regardless of the request's `time_in_force`.
- **R8 — Retrieval via the returned id.** The returned id resolves through the existing
  query endpoints: on fill, `get_my_orders(id)` returns the **two leg orders** (real pairs,
  with well-defined `filled_quote` / `filled_fee` and per-leg fills); on kill, it returns a
  single `Expired` record (zero fills/fees). Each leg carries the route id so the legs and
  their `get_my_trades` are attributable to the route.

## Non-goals

- **Multi-hop (≥ 2 transit tokens).** Only a single hop (one transit token, two legs) is
  in scope. General N-hop routing is deferred.
- **On-chain best-path search.** With a star topology the transit token is always the hub
  (the common quote), so "discovery" is a hub lookup, not a graph search. Liquidity-aware
  path selection across alternative routes is out of scope.
- **Resting / time-in-force variants of routed orders.** A routed order is an atomic taker
  (R4); GTC/post-only routed orders are out of scope.

## Design Decisions

### Topology — the transit token is always the hub

Because production is a star quoted in ckUSDT, any spoke↔spoke synthetic pair routes
through ckUSDT. "Auto-discovery" reduces to "use the hub as the transit token"; no graph
search is needed and the instruction cost stays bounded.

### Execution model — synchronous atomic taker (multi-leg FOK)

Both legs are planned **read-only** against current resting liquidity in a **single
synchronous message**; if both fully fill, the message **commits** by applying both legs'
book mutations (`apply_plan`); otherwise the whole route is **killed** with zero mutation.
This kill-or-commit decision is therefore atomic, and reuses the DEFI-2853 plan/execute
split (`plan_fills` / `apply_plan`, the `require_full` kill gate, the `Expired` terminal
state), making the all-or-nothing guarantee (R2) **structural**.

Settlement — the balance movements — is **deferred and chunked**: committing the plan
queues the fills' balance operations onto the existing `pending_settling_events`, which
`drain_settling` applies across messages within the instruction budget, the same mechanism
direct matching already uses. Because settlement is the stable-memory-heavy phase, splitting
it off keeps only the cheap read-only plan + in-heap book mutation inside the bounded
message (R6). The trade-off is that the user **may transiently hold the hub token** between
the leg-1 and leg-2 settling events; this is accepted (R4) — ckUSDT is the benign hub, and
committing the plan already guarantees the route completes. (An asynchronous saga that
commits the legs across *separate* messages was rejected — see Discussed Alternatives.)

### The transit-residual problem

The core challenge. The two legs meet at the hub, so leg 1 must *produce* exactly the
transit amount leg 2 *consumes*. But each book trades on a discrete grid (`tick_size`,
`lot_size`), and a **taker** can only take whole lots at the resting makers' prices — so
the transit amount it can move by is the marginal lot value at the live price,
`lot_size × price / 10^base_decimals`. Two independently-chosen books do not generally
admit an exact common transit amount.

**Worked example (production launch-basket values, incl. the tightened ckBTC lot).**
Buy synthetic `ICP/ckBTC`: user wants **15 ICP**, paying ckBTC; transit = ckUSDT.
Decimals: ICP 8, ckBTC 8, ckUSDT 6.

*Leg 2 — buy 15 ICP on `ICP/ckUSDT`* (tick `0.001 USDT`, lot `0.01 ICP`). Asks:

| qty | price | quote = price × qty / 10⁸ |
|-----|-------|---------------------------|
| 10 ICP | 10.000 USDT | 100.00 ckUSDT |
| 5 ICP  | 10.500 USDT |  52.50 ckUSDT |

→ leg 2 must consume **exactly `C = 152.50 ckUSDT`** to deliver the 15 ICP.

*Leg 1 — sell ckBTC on `ckBTC/ckUSDT`* (tick `0.1 USDT`, **lot `0.00001 BTC`** — the
tightened value; the previous `0.0001 BTC` lot made this ~10× worse). Best bid
`100,000 USDT/ckBTC`, so **each ckBTC lot sold raises exactly `0.00001 × 100,000 =
1.00 ckUSDT`**:

```
C / per-lot = 152.50 / 1.00 = 152.5 lots   ← not a whole number of lots
152 lots → raises 152.00  → 0.50 short → cannot fund all 15 ICP
153 lots → raises 153.00  → residual = 153.00 − 152.50 = 0.50 ckUSDT left over
```

That **0.50 ckUSDT** is the residual: leftover transit token the user never asked for.
It exists purely because `152.50` is not reachable on the funding leg's lot grid — and no
choice of reasonable `tick_size`/`lot_size` for two independent books removes it in
general. The tightened ckBTC lot shrank the worst case from ~$5–$10 to ~$0.50, but cannot
drive it to zero.

Two observations that shape the disposition:

- **The residual is bounded by one lot of the funding leg** (`< lot_size × price`), i.e.
  ≤ ~$1 across the basket after the ckBTC lot fix (≤ ~$0.50 at the example's price).
- **The slack always lands on the funding (sell) leg**: the destination leg's output is
  pinned to the user's exact target (15 ICP), which fixes `C`; the funding leg then cannot
  reach `C` exactly.

### Residual disposition — change returned to the user

The residual is **returned to the user as change in the hub token (ckUSDT)** (R3). The
route funds the destination leg by raising the smallest whole-lot amount `P ≥ C` on the
funding leg, executes both legs as fill-or-kill, and leaves the leftover `P − C` in the
user's ckUSDT balance. In the example the user ends with **15 ICP and 0.50 ckUSDT change**.

Rationale: a residual is unavoidable, so the only question is who keeps the ≤ ~$1 of
leftover transit. Returning it to the user is the fairest (no value leak — it is the
user's own change) and the simplest (settlement simply does not net to zero; no exact
chaining, tolerance band, or dust-sweep logic). It is benign in the star topology because
the transit is the **stable hub / unit of account**, which every user already transacts in
and can withdraw — unlike the ticket's original concern, which was being stranded with a
*volatile* transit token (ckBTC in the full-mesh framing). The ticket's all-or-nothing
constraint is honored on the target (R2) and reframed for the transit (R3, R4): the user
never holds a volatile/unrequested token and never a mid-route or resting position, but may
receive bounded stablecoin change. (Sweeping the residual to the fee pool was the
alternative — see Discussed Alternatives.)

### Order identity — the legs are the record; the route id resolves to them

A filled routed order is recorded as its **two leg orders** on the real books — that is the
only place where price, `filled_quote`, `filled_fee`, and fills are unambiguous (each leg
has a single quote token). There is deliberately **no parent `OrderRecord` on the synthetic
pair for a filled route**, because its money scalars would be cross-token (cost in the input
token, change in the hub, fees split across both legs) and therefore meaningless.

Instead, the transparent entry point (R7) returns a **`route_id`** — its own id space, since
a route is on no book. Each leg record carries that `route_id`, and a **derived**
`route_id → [leg_id, leg_id]` index — rebuilt on replay from the leg records, so it adds no
persisted structure beyond the field — resolves `get_my_orders(route_id)` to the two legs
(R8). A **killed** route, having no legs, is recorded as a single `Expired` `OrderRecord`
carrying the `route_id` — identical in shape to a killed FOK (`filled_quantity` /
`filled_quote` / `filled_fee` all 0) — so the same lookup returns it. The id contract is thus
uniform with direct orders: you always get an id back and read the outcome from what it
resolves to. (A parent record for filled routes, and a composite id, were both rejected —
see Discussed Alternatives.)

### Listing fix folded in — ckBTC/ckUSDT lot

Independently of routing, the launch-basket ckBTC lot (`0.0001 BTC`, ~$6–$10/lot) was
coarser than the doc's own `$0.10–$1` guidance and 10× Binance's. It has been retightened
to **tick `0.1 USDT` / lot `0.00001 BTC`** (`docs/src/usage/for-admins.md`), keeping
`tick × lot = 10⁸` so the settlement-exactness invariant holds. This also reduces the
worst-case routing residual ~10×.

## Implementation

### Constraints

- **Event-sourced.** Order creation flows through `state::audit::process_event`
  (`AddLimitOrderEvent`) and is re-applied on replay under `StableMemoryOptions::Write` /
  `Skip`. The two leg orders are created via this same path, so they produce ordinary records
  and events — extended with the `route_id` field. The `route_id → [leg_id]` lookup is a
  **derived** index, rebuilt on replay from the leg records, so it needs no event of its own.
- **Per-book, chunked matcher + deferred settlement.** Direct matching already separates the
  book mutation from balance settlement: `record_matching_event` queues `pending_settling_events`
  and `drain_settling` applies them across messages within the instruction budget. Routes
  reuse this unchanged.
- **Plan/execute.** `OrderBook::plan_fills` (read-only) → `FillPlanBuilder::build` →
  `apply_plan` (mutating), with `require_full` (DEFI-2853) gives per-leg fill-or-kill.
- **Id spaces.** `OrderId` is `book_id + seq` (book-scoped). A `route_id` is a new, non-book
  id space (the route is on no book).
- **Reservation.** Taken at placement, released on cancel/kill; the kill path reuses the
  cancel-path unreserve over the full reserved amount.

### Entry point & route detection — `canister/src/lib.rs`, `canister/src/state`

`add_limit_order` keeps its signature and `LimitOrderRequest` (R7). When `request.pair` is
**not** a registered book, attempt to route instead of returning `UnknownTradingPair`:

- **Find the transit token** `H` such that both `(base, H)` and `(quote, H)` are registered
  pairs — the common quote linking the two spokes (in the star, the hub). None ⇒
  `UnknownTradingPair` (unchanged); a unique `H` ⇒ routed order. (Ambiguity cannot arise in a
  star; if it ever does, pick deterministically and document — out of scope here.)
- The synthetic pair has no book, so there is no synthetic tick/lot/notional to check;
  validation happens on the two **legs**. `request.price` / `request.quantity` are interpreted
  on the synthetic pair (`quantity` in synthetic-base units, `price` in synthetic-quote per
  base).
- Routed orders are evaluated as fill-or-kill regardless of `time_in_force` (they cannot
  rest).

### Planning direction & quantity chaining — `canister/src/order`

Map the synthetic order to two real legs through `H`, planning the **pinned** side first:

- **Buy** synthetic `base/quote` (user pays `quote`, wants `base`): destination leg = **buy
  `base` on `(base, H)`** for the exact `quantity` ⇒ fixes the transit consumed `C`; funding
  leg = **sell `quote` on `(quote, H)`** raising the smallest whole-lot `P ≥ C`.
- **Sell** synthetic `base/quote` (user sells `base`, wants `quote`): source leg = **sell
  `base` on `(base, H)`** for the exact `quantity` ⇒ produces transit `P`; destination leg =
  **buy `quote` on `(quote, H)`** consuming the largest whole-lot `C ≤ P`.

Each leg is planned with `require_full`; if either cannot fully fill — or the funding/limit
budget is exceeded — the route kills. The transit (`H`) flows internally; the residual
`P − C` is the user's hub change (R3).

### Limit semantics — *(open sub-decision; recommendation below)*

**Recommended:** reuse the existing reservation model and express the limit as an
**input budget**: reserve `price × quantity` of the input token (buy ⇒ the `quote` token;
sell ⇒ the `base` token), exactly as a direct order does. The route kills if the destination
cannot be satisfied within that budget (buy ⇒ it would need to sell more than the reserved
input; sell ⇒ the `quote` obtained would fall below `price × quantity`). This avoids
cross-token "effective price" math (the hub change is in a third token) and keeps placement
identical to a direct limit order. *Confirm before building; the alternative — a synthetic
limit price translated into per-leg price bounds — is more familiar to traders but makes the
realized price ambiguous because of the change.*

### Execution & settlement — `canister/src/order/book.rs`, `canister/src/state/mod.rs`

Synchronously, in the `add_limit_order` message: plan both legs; on kill return the terminal
outcome with **no mutation** and the reservation released; on commit `apply_plan` both legs
(mutating both books), create the two leg orders (with `route_id`), and queue the combined
fills' balance operations onto `pending_settling_events`. Kick `drive_matching` so
`drain_settling` finishes settlement across chunks, as `add_limit_order` already does. The
change `P − C` falls out of settling both legs into the user's hub balance — no dedicated
"change" operation.

### Identity, records & retrieval — `canister/src/order/history`, query API

- Mint a `route_id`; both leg `OrderRecord`s store it. Maintain the derived
  `route_id → [leg_id, leg_id]` index (rebuilt on replay).
- `get_my_orders(route_id)` resolves to the two leg records on fill, or the single `Expired`
  record on kill. Legs also appear under `ByAccount` paging, tagged with `route_id`.
- `get_my_trades` stays leg-scoped (`ByOrder{leg_id}`, `ByAccount`); optionally accept
  `ByOrder{route_id}` and aggregate via the index.

### Public types & Candid — `libs/types/src/lib.rs`, `canister/oisy_trade.did`

- `LimitOrderRequest` unchanged (R7).
- `OrderRecord` gains `route_id: Option<RouteId>` (`None` for direct orders) so the legs and
  the killed `Expired` record reference their route.
- `OrderStatus` already carries `Expired` (DEFI-2853); reused for killed routes.
- Regenerate `oisy_trade.did`; the candid equality check must pass.

### Performance

A `canbench` benchmark covers the worst case: a route whose plan + commit sweeps two
fully-populated, fragmented books in a single message, asserting it stays within the
per-message instruction limit (R6). Settlement is excluded (it is chunked).

### Docs — `docs/src/development/design.md`

Document routed orders: the transparent entry point and hub-transit detection, fill-or-kill
on both legs, change returned to the user in the hub token, the `route_id` / two-leg /
`Expired`-on-kill identity model, and the synchronous-decision / chunked-settlement split.

### Test plan

Unit (`*/tests.rs`, fixtures in `canister/src/test_fixtures`):

- **Route detection:** a synthetic spoke↔spoke pair resolves to the hub transit and two
  legs; an unroutable unknown pair still returns `UnknownTradingPair` (R1, R7).
- **Chaining + change:** a buy delivers the exact target `quantity`; the funding leg raises
  the smallest whole-lot `P ≥ C`; the user's hub change equals `P − C` (R3). The sell
  direction is symmetric. Both legs fully fill (R2).
- **Kill is mutation-free:** when either leg cannot fully fill (or the budget is exceeded),
  the route returns killed and **both** books are byte-identical to their pre-call state
  (compared via the `OrderBookSnapshot` round-trip), with the reservation released (R2).
- **Identity & replay:** filled ⇒ `route_id` resolves to two leg records that carry it;
  killed ⇒ a single `Expired` record under the `route_id`; both round-trip through history /
  snapshot, and the derived index is rebuilt after a `Skip` replay (R8).
- **Settlement:** the hub change is credited; each leg's `filled_quote` / `filled_fee` are
  correct on its own pair (R3, R8).

`canbench`: worst-case plan + commit within the instruction limit (R6).

Integration (`integration_tests/tests/tests.rs`, PocketIC) — acceptance end-to-end:

- Buy a synthetic spoke↔spoke pair against sufficient liquidity ⇒ user receives the exact
  base quantity plus the hub change; `get_my_orders(id)` returns two `Filled` legs;
  `get_my_trades` per leg shows the real maker prices (R1, R2, R3, R8).
- Insufficient liquidity ⇒ killed: zero balance effect, reservation released,
  `get_my_orders(id)` returns one `Expired` record (R2, R8).
- Sell direction symmetric (R1, R2, R3).
- Transparent entry: the same endpoint serves direct and routed; an unknown, unroutable pair
  ⇒ `UnknownTradingPair` (R7).

Verification:

```
cargo fmt --all
just lint
cargo test -p oisy_trade_canister
cargo test -p oisy_trade_int_tests
# + the repo's candid equality check (see justfile / CI)
```

### Delivery / PR sequence

A stack, each PR independently compilable/testable.

- **PR 1 (1/3) — detection + identity plumbing.** Hub/route-detection helper; `route_id` id
  space and the `route_id: Option<…>` field on `OrderRecord` (defaulting `None`, no behavior
  change); Candid regen. A synthetic pair still returns `UnknownTradingPair` (routing not yet
  wired). All existing tests pass unchanged.
  - *Acceptance:* detection helper + identity field in place; no behavior change.
- **PR 2 (2/3) — routed execution + retrieval.** Wire `add_limit_order` to route a synthetic
  pair: synchronous plan + kill-or-apply across both legs (reusing `plan_fills` / `apply_plan`
  / `require_full`), create the two leg orders with `route_id`, the `Expired` record on kill,
  reservation handling, settlement via the existing drain; the derived `route_id` index and
  `get_my_orders` / `get_my_trades` resolution; both buy and sell directions; unit + PocketIC
  tests.
  - *Acceptance:* R1, R2, R3, R4, R5, R7, R8.
- **PR 3 (3/3) — bounds + docs.** The `canbench` worst-case benchmark and any perf hardening;
  `design.md` write-up.
  - *Acceptance:* R6, plus the design-doc AC.

## Discussed Alternatives

- **Strict kill (residual must be exactly zero).** Reject a routed order unless leg 1
  produces *exactly* the transit leg 2 consumes. Rejected: this is only satisfiable when
  the two books' grids admit a common transit amount, which imposes an unreasonable
  divisibility relationship between two independently-chosen `tick_size`/`lot_size` pairs.
  Even with the tightened ckBTC lot the exact-hit case is rare (the funding leg moves in
  ~$0.50 steps and an arbitrary `C = 152.50` rarely lands on the grid), so strict kill
  would reject the large majority of otherwise-fillable routes — a poor fill rate for no
  benefit beyond a zero-leak guarantee the bounded-residual options also provide.
- **Sweep the residual to the fee pool.** Identical execution, but the leftover `P − C` is
  credited to the canister's per-token fee balances instead of the user. Rejected: it
  transfers a variable 0–~$1 of the user's value to the protocol on top of the stated taker
  fee — effectively a hidden, lot-alignment-dependent fee that is awkward to disclose — for
  the sole benefit of the user ending with *only* the requested token. Since the transit is
  a withdrawable stablecoin, returning the change to the user (the chosen design) is fairer
  at no real UX cost.
- **User-specified tolerance band.** Let the user bound acceptable slippage on input/output
  and have the planner target a reachable amount within the band. Rejected as unnecessary
  complexity: returning the bounded change to the user achieves a high fill rate with no new
  user-facing parameter and no band-validation edge cases.
- **Asynchronous saga with rollback.** *Commit* the legs in separate messages/timer ticks
  and compensate the first if the second fails. Rejected: it makes the kill-or-commit
  decision non-atomic, so a route can commit leg 1 and then fail leg 2 and have to **roll
  back a settled fill** — which harms an innocent counterparty that legitimately traded. (Our
  model differs: the *decision* is atomic in one message; only the post-commit *settlement*
  is chunked, and a committed route always completes, so nothing is ever rolled back.)
- **A parent `OrderRecord` for filled routes.** Record a synthetic-pair parent alongside the
  two legs. Rejected: the parent's money scalars (`filled_quote`, `filled_fee`) are
  cross-token and have no honest single-quote value; the legs already carry the unambiguous
  per-pair figures, so a parent record only duplicates identity while misrepresenting value.
  The `route_id` index gives the same grouping without a misleading record.
- **A composite route id (`concat(leg_id, leg_id)`).** Encode the legs in the returned id so
  no index is needed. Rejected: it rigidly assumes exactly two legs, yields a long opaque id,
  and has no form for a killed route (which has no legs) — breaking the uniform id contract a
  single `route_id` + `Expired` record provides.
