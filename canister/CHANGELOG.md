Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-07-02

### Added

- Add get_my_trades account-wide pagination benchmark ([#196](https://github.com/dfinity/oisy-trade/pull/196))
- Add the trade store and its read API (2/5) ([#193](https://github.com/dfinity/oisy-trade/pull/193))
- Add realized quote and fee onto OrderRecord ([#171](https://github.com/dfinity/oisy-trade/pull/171))

### Changed

- Extract a settlement module and harden fill-persistence tests ([#195](https://github.com/dfinity/oisy-trade/pull/195))
- Expose the account-wide get_my_trades ByAccount filter (5/5) ([#180](https://github.com/dfinity/oisy-trade/pull/180))
- Expose get_my_trades ByOrder feed (4/5) ([#186](https://github.com/dfinity/oisy-trade/pull/186))
- Persist per-fill records in stable memory (3/5) ([#179](https://github.com/dfinity/oisy-trade/pull/179))
- Shared id/seq machinery and composite ids (1/4) ([#192](https://github.com/dfinity/oisy-trade/pull/192))
- Render prices and amounts as human-readable floats ([#182](https://github.com/dfinity/oisy-trade/pull/182))

[0.3.0]: https://github.com/dfinity/oisy-trade/compare/oisy_trade_canister-v0.2.0..oisy_trade_canister-v0.3.0

## [Unreleased]

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

[0.2.0]: https://github.com/dfinity/oisy-trade/compare/oisy_trade_canister-v0.1.0..oisy_trade_canister-v0.2.0
[0.1.0]: https://github.com/dfinity/oisy-trade/compare/oisy_trade_canister-v0.0.0..oisy_trade_canister-v0.1.0
