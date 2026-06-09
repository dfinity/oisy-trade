//! Candid types used by the candid interface of the OISY TRADE canister.

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

#[cfg(test)]
mod tests;

use candid::{CandidType, Nat, Principal};
use serde::{Deserialize, Serialize};

/// Unique identifier for an order, encoded as a hex string.
pub type OrderId = String;

/// Side of an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, CandidType)]
pub enum Side {
    /// Buy order (bid).
    Buy,
    /// Sell order (ask).
    Sell,
}

/// A trading pair identified by the base and quote token ledger principals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, CandidType)]
pub struct TradingPair {
    /// The base token ledger canister principal.
    pub base: Principal,
    /// The quote token ledger canister principal.
    pub quote: Principal,
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
}

/// Error returned when placing a limit order fails.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum AddLimitOrderError {
    /// The amount exceeds the maximum supported value.
    AmountExceedsMaximum,
    /// The requested trading pair is not registered.
    UnknownTradingPair,
    /// The price is not a positive multiple of the tick size.
    InvalidPrice {
        /// The rejected price.
        price: Nat,
        /// The required tick size.
        tick_size: Nat,
    },
    /// The quantity is not a positive multiple of the lot size.
    InvalidQuantity {
        /// The rejected quantity.
        quantity: Nat,
        /// The required lot size.
        lot_size: Nat,
    },
    /// The user does not have enough balance to place the order.
    InsufficientBalance {
        /// The token for which the balance is insufficient.
        token: TokenId,
        /// The user's available balance.
        available: Nat,
        /// The balance required to place the order.
        required: Nat,
    },
    /// The order's notional (`price × quantity / 10^base_decimals`, in quote
    /// smallest units) is below `min` or above `max`.
    InvalidNotional {
        /// The order's notional in quote token smallest units.
        notional: Nat,
        /// The configured minimum notional.
        min: Nat,
        /// The configured maximum notional, if any.
        max: Option<Nat>,
    },
    /// Trading is globally halted; no new orders are accepted.
    TradingHalted,
}

/// Error returned when canceling a limit order fails.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum CancelLimitOrderError {
    /// No order with the given ID exists.
    OrderNotFound,
    /// The caller does not own the order.
    NotOrderOwner,
    /// The order has already been fully filled and cannot be canceled.
    OrderAlreadyFilled,
    /// The order has already been canceled.
    OrderAlreadyCanceled,
}

/// Error returned by controller-gated endpoints when the caller is not
/// authorized to perform the requested action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub enum UnauthorizedError {
    /// Trading is globally halted.
    TradingHalted,
    /// The trading pair is halted.
    PairHalted,
    /// The account is frozen.
    AccountFrozen,
    /// The caller is not a controller of the canister.
    NotController,
}

/// Information about a listed trading pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct TradingPairInfo {
    /// The base token.
    pub base: Token,
    /// The quote token.
    pub quote: Token,
    /// Minimum price increment.
    pub tick_size: Nat,
    /// Minimum order quantity.
    pub lot_size: Nat,
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
/// Requests for more than this return [`GetOrderBookDepthError::LimitTooLarge`].
pub const MAX_DEPTH_LIMIT: u32 = 1_000;

/// Error returned by the `get_order_book_ticker` query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub enum GetOrderBookTickerError {
    /// The requested trading pair is not registered on the OISY TRADE.
    UnknownTradingPair,
}

/// Request for the `get_order_book_depth` query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub struct GetOrderBookDepthRequest {
    /// The trading pair whose depth should be returned.
    pub trading_pair: TradingPair,
    /// Maximum number of price levels returned per side.
    /// When `None`, [`DEFAULT_DEPTH_LIMIT`] is used. Values greater than
    /// [`MAX_DEPTH_LIMIT`] are rejected with
    /// [`GetOrderBookDepthError::LimitTooLarge`]. A value of `Some(0)` is
    /// accepted and returns empty bids/asks vectors.
    pub limit: Option<u32>,
}

/// Error returned by the `get_order_book_depth` query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub enum GetOrderBookDepthError {
    /// The requested trading pair is not registered on the OISY TRADE.
    UnknownTradingPair,
    /// The requested depth limit exceeds [`MAX_DEPTH_LIMIT`].
    LimitTooLarge {
        /// The rejected limit.
        requested: u32,
        /// The maximum limit the OISY TRADE will serve.
        max: u32,
    },
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

/// A token identified by its ledger canister ID.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, CandidType,
)]
pub struct TokenId {
    /// The canister ID of the token's ledger.
    pub ledger_id: Principal,
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

/// Error returned by the deposit endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum DepositError {
    /// The amount exceeds the maximum supported value.
    AmountExceedsMaximum,
    /// The token is not part of any trading pair on the OISY TRADE.
    UnsupportedToken {
        /// The unsupported token.
        token_id: TokenId,
    },
    /// Another deposit or withdrawal is already in flight for this
    /// `(caller, token)`. Retry once the previous operation completes.
    OperationInProgress,
    /// The inter-canister call to the token ledger failed.
    CallFailed {
        /// The ledger canister that was called.
        ledger: Principal,
        /// The name of the method that was called.
        method: String,
        /// The reason the call failed.
        reason: String,
    },
    /// The icrc2_transfer_from call to the token ledger returned an error.
    LedgerError(LedgerTransferFromError),
}

/// Errors that can be returned by the ICRC-2 `transfer_from` endpoint on a ledger canister.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum LedgerTransferFromError {
    /// The source account does not hold enough funds.
    InsufficientFunds {
        /// The current balance of the source account.
        balance: Nat,
    },
    /// The caller's allowance is not large enough.
    InsufficientAllowance {
        /// The current allowance.
        allowance: Nat,
    },
    /// The ledger is temporarily unavailable.
    TemporarilyUnavailable,
    /// Internal error
    InternalError(String),
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

/// Per-entry error reported in [`get_balances`] responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub enum GetBalancesError {
    /// The filter referenced a token that the OISY TRADE does not support.
    TokenNotSupported(FilterToken),
}

/// Whole-request error reported when [`get_balances`] rejects the
/// request before any per-entry lookup runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
pub enum GetBalancesRequestError {
    /// The filter exceeded [`MAX_FILTER_LEN`] entries.
    FilterTooLarge {
        /// The submitted filter length.
        len: u32,
        /// The maximum allowed filter length ([`MAX_FILTER_LEN`]).
        max: u32,
    },
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

/// Response after a successful withdrawal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct WithdrawResponse {
    /// The block index of the transfer on the token ledger.
    pub block_index: Nat,
}

/// Error returned by the withdraw endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum WithdrawError {
    /// The amount exceeds the maximum supported value.
    AmountExceedsMaximum,
    /// The token is not part of any trading pair on the OISY TRADE.
    UnsupportedToken {
        /// The unsupported token.
        token_id: TokenId,
    },
    /// The caller does not have enough free balance.
    InsufficientBalance {
        /// The caller's available free balance.
        available: Nat,
    },
    /// The requested amount is too small to cover the ledger transfer fee.
    AmountTooSmall {
        /// The minimum withdrawal amount (ledger fee + 1).
        min_amount: Nat,
    },
    /// Another deposit or withdrawal is already in flight for this
    /// `(caller, token)`. Retry once the previous operation completes.
    OperationInProgress,
    /// The inter-canister call to the token ledger failed.
    CallFailed {
        /// The ledger canister that was called.
        ledger: Principal,
        /// The name of the method that was called.
        method: String,
        /// The reason the call failed.
        reason: String,
    },
    /// The icrc1_transfer call to the token ledger returned an error.
    LedgerError(LedgerTransferError),
}

/// Errors that can be returned by the ICRC-1 `transfer` endpoint on a ledger canister.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum LedgerTransferError {
    /// The source account does not hold enough funds.
    InsufficientFunds {
        /// The current balance of the source account.
        balance: Nat,
    },
    /// The ledger is temporarily unavailable.
    TemporarilyUnavailable,
    /// Internal error.
    InternalError(String),
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
