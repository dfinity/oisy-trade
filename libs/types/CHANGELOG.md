Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-07-02

### Added

- Public types for the trade feed: realized per-order quote/fee scalars on order records, the `get_my_trades` request/response types (`GetMyTradesArgs`, `Trade`, `TradeId`), and the shared composite-id/sequence machinery they build on ([#171](https://github.com/dfinity/oisy-trade/pull/171), [#192](https://github.com/dfinity/oisy-trade/pull/192), [#186](https://github.com/dfinity/oisy-trade/pull/186))

[0.3.0]: https://github.com/dfinity/oisy-trade/compare/oisy_trade_types-v0.2.0..oisy_trade_types-v0.3.0

## [Unreleased]

## [0.2.0] - 2026-06-26

### Added

- Fill-or-kill (FOK) order types: a time-in-force field and an `Expired` order status ([#164](https://github.com/dfinity/oisy-trade/pull/164))

### Changed

- **BREAKING:** rework the error types returned by the user-facing endpoints into a disposition contract — each error is categorized as a request error (client-side, do not retry), a temporary error (safe to retry), or an internal canister error, with distinct `InvalidOrderId` and `OrderNotFound` errors. Impacted endpoints: `add_limit_order`, `cancel_limit_order`, `deposit`, `withdraw`, `get_balances`, `get_fee_balances`, `get_my_orders`, `get_order_book_ticker`, `get_order_book_depth` ([#158](https://github.com/dfinity/oisy-trade/pull/158), [#168](https://github.com/dfinity/oisy-trade/pull/168), [#172](https://github.com/dfinity/oisy-trade/pull/172))

## [0.1.0] - 2026-06-16

### Added

- Limit order types: submit and query order status, with validation and an `InsufficientBalance` error, plus cancel ([#11](https://github.com/dfinity/oisy-trade/pull/11), [#19](https://github.com/dfinity/oisy-trade/pull/19), [#28](https://github.com/dfinity/oisy-trade/pull/28), [#77](https://github.com/dfinity/oisy-trade/pull/77))
- Deposit and withdrawal types ([#17](https://github.com/dfinity/oisy-trade/pull/17), [#45](https://github.com/dfinity/oisy-trade/pull/45))
- Balance types: per-user free/reserved balances and the `get_balances` query ([#27](https://github.com/dfinity/oisy-trade/pull/27), [#99](https://github.com/dfinity/oisy-trade/pull/99))
- Trading pair types: `add_trading_pair`, `get_trading_pairs`, and token metadata ([#22](https://github.com/dfinity/oisy-trade/pull/22), [#21](https://github.com/dfinity/oisy-trade/pull/21), [#32](https://github.com/dfinity/oisy-trade/pull/32))
- Per-pair maker/taker fee types, also returned by `get_trading_pairs` ([#107](https://github.com/dfinity/oisy-trade/pull/107), [#153](https://github.com/dfinity/oisy-trade/pull/153))
- Order history and query types: lifecycle status, submission timestamps, and a `get_my_orders` query ([#41](https://github.com/dfinity/oisy-trade/pull/41), [#110](https://github.com/dfinity/oisy-trade/pull/110), [#115](https://github.com/dfinity/oisy-trade/pull/115))
- Order-book ticker and depth query types ([#74](https://github.com/dfinity/oisy-trade/pull/74))
- Trading-halt types: global and per-pair halt ([#126](https://github.com/dfinity/oisy-trade/pull/126), [#127](https://github.com/dfinity/oisy-trade/pull/127))

### Changed

- Settlement exactness: settle fills in quote units per whole base token, widen price and tick size to u128, and add tick·lot validation errors on `add_trading_pair` ([#119](https://github.com/dfinity/oisy-trade/pull/119), [#122](https://github.com/dfinity/oisy-trade/pull/122))
- Add a min/max notional filter per trading pair ([#131](https://github.com/dfinity/oisy-trade/pull/131))
- Expand order records with partial-fill information ([#133](https://github.com/dfinity/oisy-trade/pull/133))
- Rename the project from DEX to OISY TRADE ([#138](https://github.com/dfinity/oisy-trade/pull/138))

### Fixed

- Add an `OperationInProgress` error to guard concurrent deposits and withdrawals per (caller, token) ([#78](https://github.com/dfinity/oisy-trade/pull/78))

[0.2.0]: https://github.com/dfinity/oisy-trade/compare/oisy_trade_types-v0.1.0..oisy_trade_types-v0.2.0
[0.1.0]: https://github.com/dfinity/oisy-trade/compare/oisy_trade_types-v0.0.0..oisy_trade_types-v0.1.0
