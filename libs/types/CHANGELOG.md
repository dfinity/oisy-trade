Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-06-16

### Added

- Limit order types: submit and query order status, with validation and an `InsufficientBalance` error, plus cancel ([#11](https://github.com/dfinity/oisy-trade/pull/11), [#19](https://github.com/dfinity/oisy-trade/pull/19), [#28](https://github.com/dfinity/oisy-trade/pull/28), [#77](https://github.com/dfinity/oisy-trade/pull/77))
- Deposit and withdrawal types ([#17](https://github.com/dfinity/oisy-trade/pull/17), [#45](https://github.com/dfinity/oisy-trade/pull/45))
- Balance types: per-user free/reserved balances and the `get_balances` query ([#27](https://github.com/dfinity/oisy-trade/pull/27), [#99](https://github.com/dfinity/oisy-trade/pull/99))
- Trading pair types: `add_trading_pair`, `get_trading_pairs`, and token metadata ([#22](https://github.com/dfinity/oisy-trade/pull/22), [#21](https://github.com/dfinity/oisy-trade/pull/21), [#32](https://github.com/dfinity/oisy-trade/pull/32))
- Per-pair maker/taker fee types ([#107](https://github.com/dfinity/oisy-trade/pull/107))
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

[0.1.0]: https://github.com/dfinity/oisy-trade/compare/0.0.0..0.1.0
