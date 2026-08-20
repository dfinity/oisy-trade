Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-08-14

First release since the canister went live on mainnet: the Candid interface and
the persisted state are backward compatible, so an upgrade from 0.3.0 needs no
migration and no client change (see *Changed*).

### Added

- Separate funding and trading accounts: a funding account can whitelist trading-account principals via `add_trading_account` / `remove_trading_account` / `get_my_trading_accounts`. A trading account places and cancels orders on the funding account's balance but can never deposit or withdraw (`deposit` / `withdraw` are denied with a `TradingAccountForbidden` request-error variant). Orders and reads by a trading account resolve to the funding account, and each order records the acting key via the new `placed_by` / `canceled_by` fields on `OrderRecord`. Grants require a registered granter, target a fresh principal that can actually be exercised by a single keyholder — the anonymous principal and the management canister are rejected — and are rate-limited by a per-account cooldown; revocation is never rate-limited. All additions are backward-compatible Candid ([#207](https://github.com/dfinity/oisy-trade/pull/207), [#208](https://github.com/dfinity/oisy-trade/pull/208), [#209](https://github.com/dfinity/oisy-trade/pull/209), [#219](https://github.com/dfinity/oisy-trade/pull/219), [#222](https://github.com/dfinity/oisy-trade/pull/222), [#223](https://github.com/dfinity/oisy-trade/pull/223), [#226](https://github.com/dfinity/oisy-trade/pull/226), [#230](https://github.com/dfinity/oisy-trade/pull/230), [#238](https://github.com/dfinity/oisy-trade/pull/238))
- `max_settlement_units_per_event`, an optional init/upgrade argument capping how much settlement work a single settling event carries; absent, it falls back to the conservative production default ([#216](https://github.com/dfinity/oisy-trade/pull/216))

### Changed

- **No break for clients or for persisted state.** Every Candid addition is backward compatible: new record fields are ignored by older decoders, and the new `TradingAccountForbidden` leaves sit under `RequestError : opt variant`, which the disposition contract has older clients decode as `null`. Every persisted-state addition is a new optional CBOR field at a fresh index or a new event variant, and the trading-account maps live in fresh stable-memory regions, so a canister upgraded from state persisted by 0.3.0 decodes and replays it unchanged. This is now checked against production: a test replays the live mainnet event log and asserts the reconstructed balances, order history, trades, user registry, and order-book depth match the mainnet snapshot ([#229](https://github.com/dfinity/oisy-trade/pull/229))
- A matching round now emits several bounded settling events instead of one per chunk, so `get_events` readers see more, smaller settling events for the same settlement; the debug event stream also gains the trading-account grant/revoke events and the `placed_by` / `canceled_by` attribution ([#216](https://github.com/dfinity/oisy-trade/pull/216), [#207](https://github.com/dfinity/oisy-trade/pull/207), [#223](https://github.com/dfinity/oisy-trade/pull/223), [#226](https://github.com/dfinity/oisy-trade/pull/226))
- Render principals textually in canister logs ([#203](https://github.com/dfinity/oisy-trade/pull/203))
- Finalize the ckBTC/ckUSDT tick and lot sizing, and document the live production listings ([#246](https://github.com/dfinity/oisy-trade/pull/246))

### Fixed

- Cancel applies only its own settlement: a cancellation used to drain the whole global settlement backlog, inheriting the instruction debt of unrelated matching rounds. On a large backlog it could exceed the per-message instruction limit and trap, leaving the order uncanceled and its reservation locked, so the user could not exit ([#240](https://github.com/dfinity/oisy-trade/pull/240))
- Bound the settlement work applied per message: a single fill-or-kill order sweeping many resting makers produced one oversized settling event whose apply cost, past roughly 23,000 fills, exceeded the per-message instruction limit and froze matching. Settlement is now partitioned into bounded events at construction ([#216](https://github.com/dfinity/oisy-trade/pull/216), [#210](https://github.com/dfinity/oisy-trade/pull/210))
- Validate the deposit/withdraw amount before rendering it: the handlers formatted the caller-controlled unbounded `nat` into a diagnostic string ahead of authorization and validation, and that super-linear conversion alone could exhaust a message's instruction budget and trap the call ([#237](https://github.com/dfinity/oisy-trade/pull/237))

## [0.3.0] - 2026-07-02

### Added

- Per-fill trade history and the `get_my_trades` query: persist each fill individually in stable memory and expose a caller's trades newest-first — a per-order `ByOrder` feed and an account-wide `ByAccount` feed — built on realized per-order quote/fee scalars, shared composite-id/sequence machinery, and a dedicated trade store ([#171](https://github.com/dfinity/oisy-trade/pull/171), [#192](https://github.com/dfinity/oisy-trade/pull/192), [#193](https://github.com/dfinity/oisy-trade/pull/193), [#179](https://github.com/dfinity/oisy-trade/pull/179), [#186](https://github.com/dfinity/oisy-trade/pull/186), [#180](https://github.com/dfinity/oisy-trade/pull/180))

### Changed

- **BREAKING (pre-launch persisted state only):** the trade-history work changes the stable-memory / event-log encoding — order ids now encode as a bare CBOR `u64` instead of a 1-element array, `OrderBookSnapshot` gains a non-`Option` `next_fill` field, and the settling event carries a lean per-fill record — so a canister upgraded from state persisted before this release cannot decode it. Acceptable because the canister is pre-launch with no deployed state to migrate; the Candid/public API is unchanged ([#192](https://github.com/dfinity/oisy-trade/pull/192), [#179](https://github.com/dfinity/oisy-trade/pull/179))
- Extract a dedicated settlement module and harden the fill-persistence tests, with a `get_my_trades` account-wide pagination benchmark ([#195](https://github.com/dfinity/oisy-trade/pull/195), [#196](https://github.com/dfinity/oisy-trade/pull/196))
- Render prices and amounts as human-readable floats ([#182](https://github.com/dfinity/oisy-trade/pull/182))

## [0.2.0] - 2026-06-26

### Added

- Fill-or-kill (FOK) orders: a time-in-force on order submission with an `Expired` order status, enforced as a matching gate and through execution ([#164](https://github.com/dfinity/oisy-trade/pull/164), [#169](https://github.com/dfinity/oisy-trade/pull/169))

### Changed

- **BREAKING:** rework the error types returned by the user-facing endpoints into a disposition contract — each error is categorized as a request error (client-side, do not retry), a temporary error (safe to retry), or an internal canister error; `get_my_orders` no longer traps and returns distinct `InvalidOrderId` and `OrderNotFound` errors. Impacted endpoints: `add_limit_order`, `cancel_limit_order`, `deposit`, `withdraw`, `get_balances`, `get_fee_balances`, `get_my_orders`, `get_order_book_ticker`, `get_order_book_depth` ([#158](https://github.com/dfinity/oisy-trade/pull/158), [#168](https://github.com/dfinity/oisy-trade/pull/168), [#172](https://github.com/dfinity/oisy-trade/pull/172))

## [0.1.0] - 2026-06-16

### Added

- Limit orders: submit and query order status, with validation on submission ([#11](https://github.com/dfinity/oisy-trade/pull/11), [#19](https://github.com/dfinity/oisy-trade/pull/19)); cancel orders ([#76](https://github.com/dfinity/oisy-trade/pull/76), [#77](https://github.com/dfinity/oisy-trade/pull/77))
- Order matching: order book with a timer-driven matching engine, plus a configurable execution policy and chunked execution of pending orders ([#15](https://github.com/dfinity/oisy-trade/pull/15), [#18](https://github.com/dfinity/oisy-trade/pull/18), [#90](https://github.com/dfinity/oisy-trade/pull/90), [#89](https://github.com/dfinity/oisy-trade/pull/89))
- Deposit and withdrawal flows ([#17](https://github.com/dfinity/oisy-trade/pull/17), [#45](https://github.com/dfinity/oisy-trade/pull/45))
- Balances: per-user free/reserved balances, reserved on order placement and updated on settlement, with `get_balances` and `list_supported_tokens` queries ([#27](https://github.com/dfinity/oisy-trade/pull/27), [#28](https://github.com/dfinity/oisy-trade/pull/28), [#30](https://github.com/dfinity/oisy-trade/pull/30), [#99](https://github.com/dfinity/oisy-trade/pull/99), [#98](https://github.com/dfinity/oisy-trade/pull/98))
- Trading pairs: `add_trading_pair` and `get_trading_pairs`, with token metadata ([#22](https://github.com/dfinity/oisy-trade/pull/22), [#21](https://github.com/dfinity/oisy-trade/pull/21), [#32](https://github.com/dfinity/oisy-trade/pull/32))
- Per-pair maker/taker fees: configuration and pair plumbing, per-token fee pools, deduction at fill time, and fee visibility — including the per-pair rates in `get_trading_pairs` and the dashboard ([#107](https://github.com/dfinity/oisy-trade/pull/107), [#108](https://github.com/dfinity/oisy-trade/pull/108), [#109](https://github.com/dfinity/oisy-trade/pull/109), [#105](https://github.com/dfinity/oisy-trade/pull/105), [#153](https://github.com/dfinity/oisy-trade/pull/153))
- Order history and queries: order-lifecycle history, a per-user order index with submission timestamps, and a `get_my_orders` query; order-book ticker and depth queries ([#41](https://github.com/dfinity/oisy-trade/pull/41), [#110](https://github.com/dfinity/oisy-trade/pull/110), [#111](https://github.com/dfinity/oisy-trade/pull/111), [#115](https://github.com/dfinity/oisy-trade/pull/115), [#74](https://github.com/dfinity/oisy-trade/pull/74))
- Audit and event log for state replay, with deposit, withdrawal, trading-pair, limit-order, and matching/settlement events ([#38](https://github.com/dfinity/oisy-trade/pull/38), [#42](https://github.com/dfinity/oisy-trade/pull/42), [#44](https://github.com/dfinity/oisy-trade/pull/44), [#47](https://github.com/dfinity/oisy-trade/pull/47), [#66](https://github.com/dfinity/oisy-trade/pull/66), [#68](https://github.com/dfinity/oisy-trade/pull/68))
- Trading halts: global and per-pair halt, on a permission layer ([#125](https://github.com/dfinity/oisy-trade/pull/125), [#126](https://github.com/dfinity/oisy-trade/pull/126), [#127](https://github.com/dfinity/oisy-trade/pull/127))
- State persistence: order history and balances persisted in stable memory and restored across upgrades ([#62](https://github.com/dfinity/oisy-trade/pull/62), [#63](https://github.com/dfinity/oisy-trade/pull/63), [#64](https://github.com/dfinity/oisy-trade/pull/64))
- Observability: operation logging, canister metrics, and a dashboard with trading-pair details ([#23](https://github.com/dfinity/oisy-trade/pull/23), [#52](https://github.com/dfinity/oisy-trade/pull/52), [#79](https://github.com/dfinity/oisy-trade/pull/79), [#80](https://github.com/dfinity/oisy-trade/pull/80))

### Changed

- Settlement exactness: enforce the tick·lot settlement invariant, settle fills in quote units per whole base token, and widen price and tick size to u128 ([#119](https://github.com/dfinity/oisy-trade/pull/119), [#121](https://github.com/dfinity/oisy-trade/pull/121), [#122](https://github.com/dfinity/oisy-trade/pull/122))
- Add a min/max notional filter per trading pair ([#131](https://github.com/dfinity/oisy-trade/pull/131))
- Expand order records with partial-fill information ([#133](https://github.com/dfinity/oisy-trade/pull/133))
- Rename the project from DEX to OISY TRADE ([#138](https://github.com/dfinity/oisy-trade/pull/138))

### Fixed

- Apply order-status transitions atomically with matching, fixing a cancel-order trap on fully-filled orders ([#92](https://github.com/dfinity/oisy-trade/pull/92))
- Guard concurrent deposits and withdrawals per (caller, token) ([#78](https://github.com/dfinity/oisy-trade/pull/78))
- Surface trading-pair fee rates in `get_events` ([#134](https://github.com/dfinity/oisy-trade/pull/134))

[0.4.0]: https://github.com/dfinity/oisy-trade/compare/oisy_trade_canister-v0.3.0..oisy_trade_canister-v0.4.0
[0.3.0]: https://github.com/dfinity/oisy-trade/compare/oisy_trade_canister-v0.2.0..oisy_trade_canister-v0.3.0
[0.2.0]: https://github.com/dfinity/oisy-trade/compare/oisy_trade_canister-v0.1.0..oisy_trade_canister-v0.2.0
[0.1.0]: https://github.com/dfinity/oisy-trade/compare/oisy_trade_canister-v0.0.0..oisy_trade_canister-v0.1.0
