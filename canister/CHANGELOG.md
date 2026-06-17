Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-06-16

### Added

- Limit orders: submit and query order status, with validation on submission ([#11](https://github.com/dfinity/oisy-trade/pull/11), [#19](https://github.com/dfinity/oisy-trade/pull/19)); cancel orders ([#76](https://github.com/dfinity/oisy-trade/pull/76), [#77](https://github.com/dfinity/oisy-trade/pull/77))
- Order matching: order book with a timer-driven matching engine, plus a configurable execution policy and chunked execution of pending orders ([#15](https://github.com/dfinity/oisy-trade/pull/15), [#18](https://github.com/dfinity/oisy-trade/pull/18), [#90](https://github.com/dfinity/oisy-trade/pull/90), [#89](https://github.com/dfinity/oisy-trade/pull/89))
- Deposit and withdrawal flows ([#17](https://github.com/dfinity/oisy-trade/pull/17), [#45](https://github.com/dfinity/oisy-trade/pull/45))
- Balances: per-user free/reserved balances, reserved on order placement and updated on settlement, with `get_balances` and `list_supported_tokens` queries ([#27](https://github.com/dfinity/oisy-trade/pull/27), [#28](https://github.com/dfinity/oisy-trade/pull/28), [#30](https://github.com/dfinity/oisy-trade/pull/30), [#99](https://github.com/dfinity/oisy-trade/pull/99), [#98](https://github.com/dfinity/oisy-trade/pull/98))
- Trading pairs: `add_trading_pair` and `get_trading_pairs`, with token metadata ([#22](https://github.com/dfinity/oisy-trade/pull/22), [#21](https://github.com/dfinity/oisy-trade/pull/21), [#32](https://github.com/dfinity/oisy-trade/pull/32))
- Per-pair maker/taker fees: configuration and pair plumbing, per-token fee pools, deduction at fill time, and fee visibility ([#107](https://github.com/dfinity/oisy-trade/pull/107), [#108](https://github.com/dfinity/oisy-trade/pull/108), [#109](https://github.com/dfinity/oisy-trade/pull/109), [#105](https://github.com/dfinity/oisy-trade/pull/105))
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


## [Unreleased]
