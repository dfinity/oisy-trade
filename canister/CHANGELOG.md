Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-06-16

### Added

- Add min/max notional filter per trading pair ([#131](https://github.com/dfinity/oisy-trade/pull/131))
- Add get_my_orders query (3/3) ([#115](https://github.com/dfinity/oisy-trade/pull/115))
- Add release pipeline ([#113](https://github.com/dfinity/oisy-trade/pull/113))
- Add ic-wasm check-endpoints check ([#83](https://github.com/dfinity/oisy-trade/pull/83))
- Add `cancel_limit_order` endpoint ([#77](https://github.com/dfinity/oisy-trade/pull/77))
- Add metrics ([#52](https://github.com/dfinity/oisy-trade/pull/52))
- Add DEX CLI walkthroughs for traders and admins ([#67](https://github.com/dfinity/oisy-trade/pull/67))
- Add ci-pass gate job and cargo-sort lint ([#55](https://github.com/dfinity/oisy-trade/pull/55))
- Add AddLimitOrder event ([#47](https://github.com/dfinity/oisy-trade/pull/47))
- Add Deposit event ([#44](https://github.com/dfinity/oisy-trade/pull/44))
- Add AddTradingPair event ([#42](https://github.com/dfinity/oisy-trade/pull/42))
- Add token metadata to trading pairs ([#32](https://github.com/dfinity/oisy-trade/pull/32))
- Add `get_trading_pairs query` endpoint ([#21](https://github.com/dfinity/oisy-trade/pull/21))
- Add `add_limit_order` and `get_order_status` endpoints ([#11](https://github.com/dfinity/oisy-trade/pull/11))

### Changed

- Keep first-release crates at 0.1.0, only types at 0.0.0 [DEFI-2841]
- Reset crate versions to 0.0.0 [DEFI-2841]
- Unit conventions and sizing guidelines for trading pairs ([#140](https://github.com/dfinity/oisy-trade/pull/140))
- Per-pair halt (3/4) ([#127](https://github.com/dfinity/oisy-trade/pull/127))
- Global trading halt (2/4) ([#126](https://github.com/dfinity/oisy-trade/pull/126))
- Rename DEX to OISY TRADE ([#138](https://github.com/dfinity/oisy-trade/pull/138))
- Expand order records with partial-fill information ([#133](https://github.com/dfinity/oisy-trade/pull/133))
- Permission scaffolding (behavior-neutral) (1/4) ([#125](https://github.com/dfinity/oisy-trade/pull/125))
- Widen price and tick size to u128 (3/3) ([#122](https://github.com/dfinity/oisy-trade/pull/122))
- Settle fills in quote units per whole base token (2/3) ([#121](https://github.com/dfinity/oisy-trade/pull/121))
- Maintain a per-user order index (2/3) ([#111](https://github.com/dfinity/oisy-trade/pull/111))
- Enforce trading-pair settlement-exactness invariant (1/3) ([#119](https://github.com/dfinity/oisy-trade/pull/119))
- Split history into a module dir with separate tests ([#123](https://github.com/dfinity/oisy-trade/pull/123))
- Intern principals to a compact UserId ([#120](https://github.com/dfinity/oisy-trade/pull/120))
- Fee visibility (5/5) ([#105](https://github.com/dfinity/oisy-trade/pull/105))
- Record a submission timestamp on every order (1/3) ([#110](https://github.com/dfinity/oisy-trade/pull/110))
- Deduct maker/taker fees at fill time (4/5) ([#109](https://github.com/dfinity/oisy-trade/pull/109))
- Per-token fee pool in TokenBalance (3/5) ([#108](https://github.com/dfinity/oisy-trade/pull/108))
- Per-pair maker/taker fees — types and pair plumbing (2/5) ([#107](https://github.com/dfinity/oisy-trade/pull/107))
- `get_balances` endpoint with optional token filter ([#99](https://github.com/dfinity/oisy-trade/pull/99))
- Reproducible Docker build for the canister WASM ([#100](https://github.com/dfinity/oisy-trade/pull/100))
- Chunk pending order execution ([#89](https://github.com/dfinity/oisy-trade/pull/89))
- `list_supported_tokens` endpoint ([#98](https://github.com/dfinity/oisy-trade/pull/98))
- Apply order-status transitions atomically with matching (DEFI-2743) ([#92](https://github.com/dfinity/oisy-trade/pull/92))
- Configurable execution policy via init/upgrade args ([#90](https://github.com/dfinity/oisy-trade/pull/90))
- Expand dashboard with trading pair information ([#80](https://github.com/dfinity/oisy-trade/pull/80))
- Bump askama from 0.14.0 to 0.16.0 ([#81](https://github.com/dfinity/oisy-trade/pull/81))
- Cancel order logic ([#76](https://github.com/dfinity/oisy-trade/pull/76))
- Drain SettlingEvents from a single pending queue ([#75](https://github.com/dfinity/oisy-trade/pull/75))
- Skeleton DEX dashboard ([#79](https://github.com/dfinity/oisy-trade/pull/79))
- Expose order book ticker and depth queries ([#74](https://github.com/dfinity/oisy-trade/pull/74))
- WithdrawEvent in audit log ([#68](https://github.com/dfinity/oisy-trade/pull/68))
- Audit events for matching and settling ([#66](https://github.com/dfinity/oisy-trade/pull/66))
- Save `State` with `OrderBook` in `pre_upgrade` ([#64](https://github.com/dfinity/oisy-trade/pull/64))
- Persist balances in stable memory ([#63](https://github.com/dfinity/oisy-trade/pull/63))
- Persist order history in stable memory ([#62](https://github.com/dfinity/oisy-trade/pull/62))
- Replace Quantity's BigUint with stack-allocated u256 ([#59](https://github.com/dfinity/oisy-trade/pull/59))
- Introduce TokenBalance/UserBalance and simplify settle_fill ([#60](https://github.com/dfinity/oisy-trade/pull/60))
- Refine canbench scopes to analyze matching/settling cost ([#56](https://github.com/dfinity/oisy-trade/pull/56))
- DEFI-2728: Add withdrawal endpoint ([#45](https://github.com/dfinity/oisy-trade/pull/45))
- Replace O(n) trading pair reverse lookup with BiBTreeMap ([#53](https://github.com/dfinity/oisy-trade/pull/53))
- Track order lifecycle in `OrderHistory` ([#41](https://github.com/dfinity/oisy-trade/pull/41))
- Event log infrastructure for canister state replay ([#38](https://github.com/dfinity/oisy-trade/pull/38))
- Add canbench benchmarks for order processing and balance settlement ([#31](https://github.com/dfinity/oisy-trade/pull/31))
- Quantity to wrap Nat ([#33](https://github.com/dfinity/oisy-trade/pull/33))
- Settle fills by updating user balances ([#30](https://github.com/dfinity/oisy-trade/pull/30))
- Reserve balance upon adding a limit order ([#28](https://github.com/dfinity/oisy-trade/pull/28))
- Deploy to staging ([#24](https://github.com/dfinity/oisy-trade/pull/24))
- Add logging infrastructure ([#23](https://github.com/dfinity/oisy-trade/pull/23))
- User balances with free and reserved amounts ([#27](https://github.com/dfinity/oisy-trade/pull/27))
- Abstract IC runtime behind a trait to ease unit tests ([#26](https://github.com/dfinity/oisy-trade/pull/26))
- Make `OrderId` opaque and managed by each order book ([#20](https://github.com/dfinity/oisy-trade/pull/20))
- Timer-driven matching engine ([#18](https://github.com/dfinity/oisy-trade/pull/18))
- DEFI-2741: Add add_trading_pair endpoint ([#22](https://github.com/dfinity/oisy-trade/pull/22))
- Validate limit orders when submitted ([#19](https://github.com/dfinity/oisy-trade/pull/19))
- DEFI-2727: Add deposit flow ([#17](https://github.com/dfinity/oisy-trade/pull/17))
- Order book and matching engine ([#15](https://github.com/dfinity/oisy-trade/pull/15))
- Initial cargo workspace and build pipeline ([#1](https://github.com/dfinity/oisy-trade/pull/1))

### Fixed

- Surface trading pair fee rates in get_events ([#134](https://github.com/dfinity/oisy-trade/pull/134))
- Cover InvalidBasisPoint mapping in add_trading_pair ([#117](https://github.com/dfinity/oisy-trade/pull/117))
- Guard concurrent deposits and withdrawals per (caller, token) ([#78](https://github.com/dfinity/oisy-trade/pull/78))
- Add worst-case event size tests and benchmarks ([#50](https://github.com/dfinity/oisy-trade/pull/50))
- Verify CBOR decoding of zero fails for NonZeroU64 ([#49](https://github.com/dfinity/oisy-trade/pull/49))


## [Unreleased]
