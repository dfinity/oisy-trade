//! Candid types used by the candid interface of the OISY TRADE canister.

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

#[cfg(test)]
mod tests;

mod error;

pub use error::{
    AddLimitOrderError, AddLimitOrderRequestError, AddLimitOrderTemporaryError,
    AddTradingAccountError, AddTradingAccountRequestError, AddTradingAccountTemporaryError,
    CancelLimitOrderError, CancelLimitOrderRequestError, DepositError, DepositInternalError,
    DepositRequestError, DepositTemporaryError, Error, ErrorKind, GetBalancesError,
    GetBalancesRequestError, GetMyOrdersError, GetMyOrdersRequestError, GetMyTradesError,
    GetMyTradesRequestError, GetMyTradingAccountsError, GetOrderBookDepthError,
    GetOrderBookDepthRequestError, GetOrderBookTickerError, GetOrderBookTickerRequestError, Never,
    RemoveTradingAccountError, RemoveTradingAccountRequestError, WithdrawError,
    WithdrawInternalError, WithdrawRequestError, WithdrawTemporaryError,
};

use candid::{CandidType, Nat, Principal};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for an order, encoded as a lowercase hex string.
pub type OrderId = String;

/// Side of an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, CandidType)]
pub enum Side {
    /// Buy order (bid).
    Buy,
    /// Sell order (ask).
    Sell,
}

/// Time-in-force policy governing how long a limit order stays active in the
/// order book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, CandidType)]
pub enum TimeInForce {
    /// Rests in the book until filled or canceled; may fill partially over time.
    GoodTilCanceled,
    /// Must fill in full against resting liquidity when the engine processes it,
    /// otherwise the whole order is killed with zero execution. Never rests.
    FillOrKill,
}

/// A trading pair identified by the base and quote token ledger principals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, CandidType)]
pub struct TradingPair {
    /// The base token ledger canister principal.
    pub base: Principal,
    /// The quote token ledger canister principal.
    pub quote: Principal,
}

impl fmt::Display for TradingPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { base, quote } = self;
        write!(f, "{base}/{quote}")
    }
}

/// Request to place a new limit order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct LimitOrderRequest {
    /// The trading pair to place the order on.
    pub pair: TradingPair,
    /// Whether this is a buy or sell order.
    pub side: Side,
    /// Limit price in quote token smallest units per one whole base token.
    pub price: Nat,
    /// Order quantity in base token units.
    pub quantity: Nat,
    /// Time-in-force policy. Absent defaults to
    /// [`TimeInForce::GoodTilCanceled`].
    pub time_in_force: Option<TimeInForce>,
}

impl fmt::Display for LimitOrderRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            pair,
            side,
            price,
            quantity,
            time_in_force,
        } = self;
        write!(
            f,
            "LimitOrderRequest(pair={pair}, side={side:?}, price={price}, quantity={quantity}, time_in_force={time_in_force:?})"
        )
    }
}

/// Error returned by controller-gated endpoints when the caller is not
/// authorized to perform the requested action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub enum UnauthorizedError {
    /// The caller is not a controller of the canister.
    NotController,
}

/// Whether trading on a pair is currently active or halted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub enum TradingStatus {
    /// Trading on the pair is active.
    Trading,
    /// Trading on the pair is halted.
    Halted,
}

/// Information about a listed trading pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct TradingPairInfo {
    /// The base token.
    pub base: Token,
    /// The quote token.
    pub quote: Token,
    /// Whether trading on this pair is currently active or halted.
    pub status: TradingStatus,
    /// Minimum price increment.
    pub tick_size: Nat,
    /// Minimum order quantity.
    pub lot_size: Nat,
    /// Maker fee rate, in basis points.
    pub maker_fee_bps: u16,
    /// Taker fee rate, in basis points.
    pub taker_fee_bps: u16,
    /// Minimum order notional in quote token smallest units.
    pub min_notional: Nat,
    /// Maximum order notional in quote token smallest units, if any.
    pub max_notional: Option<Nat>,
}

/// A single price level in an order book, aggregated across all resting orders at that price.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct PriceLevel {
    /// Price in quote token smallest units per one whole base token.
    pub price: Nat,
    /// Total quantity in base token units across all resting orders at this price.
    pub quantity: Nat,
}

/// Top-of-book view of an order book for a trading pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct OrderBookTicker {
    /// Best bid (highest-priced buy level), or `None` if the bid side is empty.
    pub bid: Option<PriceLevel>,
    /// Best ask (lowest-priced sell level), or `None` if the ask side is empty.
    pub ask: Option<PriceLevel>,
}

/// Price-aggregated depth view of an order book for a trading pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct OrderBookDepth {
    /// Bid levels sorted by price descending (best bid first).
    pub bids: Vec<PriceLevel>,
    /// Ask levels sorted by price ascending (best ask first).
    pub asks: Vec<PriceLevel>,
}

/// Default depth served by `get_order_book_depth` when the caller omits `limit`.
pub const DEFAULT_DEPTH_LIMIT: u32 = 100;

/// Maximum depth (levels per side) that `get_order_book_depth` will serve.
/// Requests for more than this return a [`GetOrderBookDepthError`] carrying
/// [`GetOrderBookDepthRequestError::LimitTooLarge`] under `kind = RequestError`.
pub const MAX_DEPTH_LIMIT: u32 = 1_000;

/// Request for the `get_order_book_depth` query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct GetOrderBookDepthRequest {
    /// The trading pair whose depth should be returned.
    pub trading_pair: TradingPair,
    /// Maximum number of price levels returned per side.
    /// When `None`, [`DEFAULT_DEPTH_LIMIT`] is used. Values greater than
    /// [`MAX_DEPTH_LIMIT`] are rejected with a [`GetOrderBookDepthError`]
    /// carrying [`GetOrderBookDepthRequestError::LimitTooLarge`] under
    /// `kind = RequestError`. A value of `Some(0)` is accepted and returns
    /// empty bids/asks vectors.
    pub limit: Option<u32>,
}

/// Status of an order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub enum OrderStatus {
    /// The order is pending processing.
    Pending,
    /// The order is open and resting in the order book.
    Open,
    /// The order has been fully filled.
    Filled,
    /// The order has been canceled.
    Canceled,
    /// The order was terminated by the engine because its time-in-force could
    /// not be honored (a Fill-or-Kill that could not fully fill).
    Expired,
}

/// Full view of an order as stored by the OISY TRADE. Returned by endpoints that
/// have the whole record already loaded in hand (e.g. `cancel_limit_order`),
/// saving the caller a follow-up status/metadata query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct OrderRecord {
    /// Principal that placed the order.
    pub owner: Principal,
    /// Whether the order is a buy or a sell.
    pub side: Side,
    /// Limit price in quote token smallest units per one whole base token, as originally placed.
    pub price: Nat,
    /// Quantity originally placed, in base token units.
    pub quantity: Nat,
    /// Cumulative quantity filled so far, in base token units. Remaining is
    /// `quantity − filled_quantity`.
    pub filled_quantity: Nat,
    /// Current lifecycle state.
    pub status: OrderStatus,
    /// Submission time in nanoseconds since the Unix epoch.
    pub created_at: u64,
    /// Time of the most recent modifying event (fill, status transition, or
    /// cancel) in nanoseconds since the Unix epoch; `None` until first modified.
    pub last_updated_at: Option<u64>,
    /// Time-in-force policy the order was placed with.
    pub time_in_force: TimeInForce,
    /// Cumulative realized quote notional transacted across the order's fills.
    /// Always quote-denominated; a buy taker's released reservation surplus is
    /// excluded. VWAP (average execution price) is `filled_quote / filled_quantity`,
    /// a ratio in the two tokens' smallest units.
    pub filled_quote: Nat,
    /// Cumulative realized fee charged across the order's fills, denominated in
    /// the order's receive token — base for a buy, quote for a sell.
    pub filled_fee: Nat,
    /// The principal that placed the order when it differs from `owner` (a
    /// trading account acting for the funding account); `None` when the owner
    /// placed it itself.
    pub placed_by: Option<Principal>,
    /// The principal that canceled the order when it differs from `owner` (a
    /// trading account acting for the funding account); `None` when the owner
    /// canceled it itself.
    pub canceled_by: Option<Principal>,
}

impl fmt::Display for OrderRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            owner,
            side,
            price,
            quantity,
            filled_quantity,
            status,
            created_at,
            last_updated_at,
            time_in_force,
            filled_quote,
            filled_fee,
            placed_by,
            canceled_by,
        } = self;
        let placed_by = match placed_by {
            Some(principal) => format!("Some({principal})"),
            None => "None".to_string(),
        };
        let canceled_by = match canceled_by {
            Some(principal) => format!("Some({principal})"),
            None => "None".to_string(),
        };
        write!(
            f,
            "OrderRecord(owner={owner}, side={side:?}, price={price}, quantity={quantity}, filled_quantity={filled_quantity}, status={status:?}, created_at={created_at}, last_updated_at={last_updated_at:?}, time_in_force={time_in_force:?}, filled_quote={filled_quote}, filled_fee={filled_fee}, placed_by={placed_by}, canceled_by={canceled_by})"
        )
    }
}

/// Maximum number of orders returned by a single `get_my_orders` call.
/// Requests for more are silently capped to this many.
pub const MAX_ORDERS_PER_RESPONSE: u32 = 100;

/// Request for the `get_my_orders` query.
///
/// The endpoint takes an `opt GetMyOrdersArgs`; an absent argument is
/// equivalent to [`GetMyOrdersArgs::default()`], the first page from the
/// newest order with the maximum length.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct GetMyOrdersArgs {
    /// How to select the caller's orders.
    pub filter: GetMyOrdersFilter,
}

impl GetMyOrdersArgs {
    /// A point lookup by order id.
    pub fn by_id(id: OrderId) -> Self {
        Self {
            filter: GetMyOrdersFilter::ById(id),
        }
    }

    /// A page over the caller's orders, newest first.
    pub fn by_page(after: Option<OrderId>, length: u32) -> Self {
        Self {
            filter: GetMyOrdersFilter::ByPage(GetMyOrdersPage { after, length }),
        }
    }
}

/// Selector for `get_my_orders`: either a point lookup by id or a page. The
/// two modes are mutually exclusive by construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub enum GetMyOrdersFilter {
    /// Return the single matching order if the caller owns it, else empty.
    ById(OrderId),
    /// Return a page over the caller's orders, newest first.
    ByPage(GetMyOrdersPage),
}

impl Default for GetMyOrdersFilter {
    fn default() -> Self {
        Self::ByPage(GetMyOrdersPage::default())
    }
}

/// A page over the caller's orders, newest first. `length` is capped at
/// [`MAX_ORDERS_PER_RESPONSE`].
///
/// Pages via a cursor: pass the previous page's last [`UserOrder::id`] as
/// `after` to get the next page; `None` starts from the newest order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct GetMyOrdersPage {
    /// Resume strictly after this order id (a prior page's last `id`).
    /// `None` starts from the newest order.
    pub after: Option<OrderId>,
    /// Maximum number of orders to return.
    pub length: u32,
}

impl Default for GetMyOrdersPage {
    fn default() -> Self {
        Self {
            after: None,
            length: MAX_ORDERS_PER_RESPONSE,
        }
    }
}

/// One entry in a `get_my_orders` response: an order the caller placed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct UserOrder {
    /// The order's unique identifier.
    pub id: OrderId,
    /// The trading pair the order was placed on.
    pub pair: TradingPair,
    /// The full order record. `get_my_orders` only returns the caller's own
    /// orders, so `order.owner` is always the caller — reused as-is for shape
    /// parity with other order-returning endpoints.
    pub order: OrderRecord,
}

/// Selector for the base or quote token of a trading pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, CandidType)]
pub enum PairToken {
    /// The base token of the pair.
    Base,
    /// The quote token of the pair.
    Quote,
}

/// Unique identifier for a trade, encoded as a hex string. Opaque to callers
/// (like [`OrderId`]): a client passes the last value it received back as the
/// next page's `after` and never parses it. Treating it as opaque text lets the
/// endpoint tell a malformed token (an error) from a well-formed-but-unknown one
/// (an empty page).
pub type TradeId = String;

/// Maximum number of trades returned by a single `get_my_trades` call.
/// Requests for more are silently capped to this many.
pub const MAX_TRADES_PER_RESPONSE: u32 = 100;

/// Caller's order's projected fill, as returned by `get_my_trades`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct Trade {
    /// This trade's unique identifier.
    pub id: TradeId,
    /// The owning (caller's) order.
    pub order_id: OrderId,
    /// This order's side.
    pub side: Side,
    /// Execution price (the maker's price), in quote-token smallest units per
    /// one whole base token.
    pub price: Nat,
    /// Base filled, in base-token smallest units.
    pub quantity: Nat,
    /// Quote transacted (`price × quantity / 10^base_decimals`, realized), in
    /// quote-token smallest units. A buy taker's reservation surplus is excluded.
    pub notional: Nat,
    /// Realized fee charged to this side, in `fee_token` smallest units.
    pub fee: Nat,
    /// The token the fee is charged in — base for a buy, quote for a sell.
    pub fee_token: PairToken,
    /// This side's role on this fill.
    pub is_maker: bool,
    /// Settlement time in nanoseconds since the Unix epoch.
    pub timestamp: u64,
}

/// Request for the `get_my_trades` query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct GetMyTradesArgs {
    /// How to select the caller's trades.
    pub filter: TradesFilter,
}

/// Selector for `get_my_trades`: the caller's fills for one order, or across all
/// their orders. Both modes are owner-scoped, newest-first, and paginated by an
/// `after` cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub enum TradesFilter {
    /// The caller's fills for a single order.
    ByOrder(TradesByOrder),
    /// The caller's fills across all their orders.
    ByAccount(TradesByAccount),
}

/// A page over the caller's fills for one order, newest first. `length` is
/// capped at [`MAX_TRADES_PER_RESPONSE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct TradesByOrder {
    /// The order whose fills should be returned. Errors with `OrderNotFound` if
    /// the caller does not own it.
    pub order_id: OrderId,
    /// Resume strictly after this cursor — the [`Trade::id`] of the prior page's
    /// last entry. `None` starts from the newest fill.
    pub after: Option<TradeId>,
    /// Maximum number of trades to return.
    pub length: u32,
}

/// A page over the caller's fills across all their orders, newest first.
/// `length` is capped at [`MAX_TRADES_PER_RESPONSE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct TradesByAccount {
    /// Resume strictly after this cursor — the [`Trade::id`] of the prior page's
    /// last entry. `None` starts from the newest fill.
    pub after: Option<TradeId>,
    /// Maximum number of trades to return.
    pub length: u32,
}

/// A token identified by its ledger canister ID.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, CandidType,
)]
pub struct TokenId {
    /// The canister ID of the token's ledger.
    pub ledger_id: Principal,
}

impl fmt::Display for TokenId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { ledger_id } = self;
        write!(f, "{ledger_id}")
    }
}

/// Metadata associated with a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct TokenMetadata {
    /// The token's ticker symbol (e.g. "ckBTC").
    pub symbol: String,
    /// The number of decimal places used by the token.
    pub decimals: u8,
}

/// A token with its identity and metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct Token {
    /// The token's unique identifier.
    pub id: TokenId,
    /// The token's metadata.
    pub metadata: TokenMetadata,
}

/// Request to deposit tokens into the OISY TRADE.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct DepositRequest {
    /// The token to deposit.
    pub token_id: TokenId,
    /// The amount to deposit.
    pub amount: Nat,
}

impl fmt::Display for DepositRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { token_id, amount } = self;
        write!(f, "DepositRequest(token_id={token_id}, amount={amount})")
    }
}

/// Response after a successful deposit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct DepositResponse {
    /// The block index of the transfer on the token ledger.
    pub block_index: Nat,
}

/// A user's balance for a given token.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct Balance {
    /// Funds available for new orders or withdrawal.
    pub free: Nat,
    /// Funds locked by open orders.
    pub reserved: Nat,
}

/// Maximum number of entries allowed in a [`get_balances`] filter.
pub const MAX_FILTER_LEN: u32 = 100;

/// Selector for filtering tokens. New variants may be added in
/// backward-compatible upgrades.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, CandidType)]
pub enum FilterToken {
    /// Select a token by its identifier.
    ById(TokenId),
}

/// A single `(token, balance)` entry in a [`get_balances`] response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct UserTokenBalance {
    /// The token whose balance is reported.
    pub token: Token,
    /// The caller's free + reserved holdings for `token`.
    pub balance: Balance,
}

/// Request to add a new trading pair to the OISY TRADE.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct AddTradingPairRequest {
    /// The base token of the pair (e.g. ckSOL).
    pub base: Token,
    /// The quote token of the pair (e.g. ckBTC).
    pub quote: Token,
    /// Minimum price increment. Must be greater than zero.
    pub tick_size: Nat,
    /// Minimum order quantity. Must be greater than zero.
    pub lot_size: Nat,
    /// Maker fee rate in basis points (1 bps = 0.01 %). Must be in `0..=10_000`.
    pub maker_fee_bps: u16,
    /// Taker fee rate in basis points (1 bps = 0.01 %). Must be in `0..=10_000`.
    pub taker_fee_bps: u16,
    /// Minimum order notional in quote token smallest units. Must be greater than zero.
    pub min_notional: Nat,
    /// Maximum order notional in quote token smallest units, if any.
    /// When set, must be greater than or equal to `min_notional`.
    pub max_notional: Option<Nat>,
}

/// Request to withdraw tokens from the OISY TRADE.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct WithdrawRequest {
    /// The token to withdraw.
    pub token_id: TokenId,
    /// The amount to withdraw from the caller's free balance.
    /// The ledger transfer fee is deducted from this amount,
    /// so the caller receives `amount - fee` on the ledger.
    pub amount: Nat,
}

impl fmt::Display for WithdrawRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { token_id, amount } = self;
        write!(f, "WithdrawRequest(token_id={token_id}, amount={amount})")
    }
}

/// Response after a successful withdrawal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct WithdrawResponse {
    /// The block index of the transfer on the token ledger.
    pub block_index: Nat,
}

/// Error returned by the `add_trading_pair` endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum AddTradingPairError {
    /// The caller is not a controller of the canister.
    NotController,
    /// The base and quote tokens are the same.
    BaseEqualsQuote,
    /// The tick size must be greater than zero.
    InvalidTickSize,
    /// The lot size must be greater than zero.
    InvalidLotSize,
    /// One of the fee rates is outside `0..=10_000` bps; the carried
    /// value is the offending bps.
    InvalidBasisPoint(u16),
    /// A trading pair with the same base and quote tokens already exists.
    TradingPairAlreadyExists,
    /// The submitted token metadata does not match the previously registered metadata.
    InconsistentTokenMetadata {
        /// The token whose metadata is inconsistent.
        token: TokenId,
        /// The previously registered metadata.
        expected: TokenMetadata,
        /// The metadata that was submitted.
        submitted: TokenMetadata,
    },
    /// The base token has too many decimals for settlement to be representable:
    /// `10^base_decimals` must fit a `u64`, i.e. `decimals <= 19`.
    BaseDecimalsTooLarge {
        /// The offending base-token decimals.
        decimals: u8,
    },
    /// `tick_size × lot_size` is not a multiple of `10^base_decimals`, so some
    /// fills could not settle to an exact quote amount. Choose a coarser
    /// `tick_size` or `lot_size`.
    IndivisibleTickLotForBaseDecimals {
        /// The submitted tick size.
        tick_size: Nat,
        /// The submitted lot size.
        lot_size: Nat,
        /// The base token's decimals (the divisor exponent).
        base_decimals: u8,
    },
    /// The notional bounds are invalid: `min_notional` is zero, a bound is too
    /// large to fit the 256-bit quantity representation, or `max_notional` is
    /// set and smaller than `min_notional`.
    InvalidNotional {
        /// The submitted minimum notional.
        min_notional: Nat,
        /// The submitted maximum notional, if any.
        max_notional: Option<Nat>,
    },
}
