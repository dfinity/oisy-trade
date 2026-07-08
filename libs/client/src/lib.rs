//! Client to interact with the OISY TRADE canister

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

use async_trait::async_trait;
use candid::utils::ArgumentEncoder;
use candid::{CandidType, Principal};
use ic_cdk::call::{Call, CallFailed, RejectCode};
use oisy_trade_types::{
    AddLimitOrderError, AddTradingAccountError, AddTradingPairError, AddTradingPairRequest,
    Balance, CancelLimitOrderError, DepositError, DepositRequest, DepositResponse, FilterToken,
    GetBalancesError, GetMyOrdersArgs, GetMyOrdersError, GetMyTradesArgs, GetMyTradesError,
    GetMyTradingAccountsError, GetOrderBookDepthError, GetOrderBookDepthRequest,
    GetOrderBookTickerError, LimitOrderRequest, OrderBookDepth, OrderBookTicker, OrderId,
    OrderRecord, RemoveTradingAccountError, Token, TokenId, Trade, TradingPair, TradingPairInfo,
    UnauthorizedError, UserOrder, UserTokenBalance, WithdrawError, WithdrawRequest,
    WithdrawResponse,
};
use serde::de::DeserializeOwned;

/// Abstract the canister runtime so that the client code can be reused:
/// * in production using `ic_cdk`,
/// * in unit tests by mocking this trait,
/// * in integration tests by implementing this trait for `PocketIc`.
#[async_trait]
pub trait Runtime {
    /// Defines how asynchronous inter-canister calls are made.
    async fn call<In, Out>(
        &self,
        id: Principal,
        method: &str,
        args: In,
        cycles: u128,
    ) -> Result<Out, (RejectCode, String)>
    where
        In: ArgumentEncoder + Send + 'static,
        Out: CandidType + DeserializeOwned + 'static;
}

/// Client to interact with the OISY TRADE canister.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct OisyTradeClient<R: Runtime> {
    runtime: R,
    oisy_trade_canister: Principal,
}

impl OisyTradeClient<IcRuntime> {
    /// Instantiate a new client to be used by a canister on the Internet Computer.
    ///
    /// To use another runtime, see [`Self::new`].
    pub fn new_for_ic(oisy_trade_canister: Principal) -> Self {
        Self {
            runtime: IcRuntime {},
            oisy_trade_canister,
        }
    }
}

impl<R: Runtime> OisyTradeClient<R> {
    /// Instantiate a new client with a specific runtime.
    ///
    /// To use the client inside a canister, see [`OisyTradeClient<IcRuntime>::new_for_ic`].
    pub fn new(runtime: R, oisy_trade_canister: Principal) -> Self {
        Self {
            runtime,
            oisy_trade_canister,
        }
    }

    /// Place a new limit order on the OISY TRADE canister.
    pub async fn add_limit_order(
        &self,
        request: LimitOrderRequest,
    ) -> Result<OrderId, AddLimitOrderError> {
        self.runtime
            .call(self.oisy_trade_canister, "add_limit_order", (request,), 0)
            .await
            .unwrap()
    }

    /// Cancel a limit order previously submitted by the caller.
    pub async fn cancel_limit_order(
        &self,
        order_id: OrderId,
    ) -> Result<OrderRecord, CancelLimitOrderError> {
        self.runtime
            .call(
                self.oisy_trade_canister,
                "cancel_limit_order",
                (order_id,),
                0,
            )
            .await
            .unwrap()
    }

    /// Query the caller's orders: a page (newest first) or a single order by id,
    /// depending on the `GetMyOrdersArgs` filter.
    pub async fn get_my_orders(
        &self,
        args: GetMyOrdersArgs,
    ) -> Result<Vec<UserOrder>, GetMyOrdersError> {
        self.runtime
            .call(self.oisy_trade_canister, "get_my_orders", (Some(args),), 0)
            .await
            .unwrap()
    }

    /// Query the caller's fills, by order or across the account, depending on
    /// the `GetMyTradesArgs` filter.
    pub async fn get_my_trades(
        &self,
        args: GetMyTradesArgs,
    ) -> Result<Vec<Trade>, GetMyTradesError> {
        self.runtime
            .call(self.oisy_trade_canister, "get_my_trades", (args,), 0)
            .await
            .unwrap()
    }

    /// Whitelist `trading` as a trading account of the caller's funding account.
    pub async fn add_trading_account(
        &self,
        trading: Principal,
    ) -> Result<(), AddTradingAccountError> {
        self.runtime
            .call(
                self.oisy_trade_canister,
                "add_trading_account",
                (trading,),
                0,
            )
            .await
            .unwrap()
    }

    /// Revoke a trading account previously whitelisted by the caller.
    pub async fn remove_trading_account(
        &self,
        trading: Principal,
    ) -> Result<(), RemoveTradingAccountError> {
        self.runtime
            .call(
                self.oisy_trade_canister,
                "remove_trading_account",
                (trading,),
                0,
            )
            .await
            .unwrap()
    }

    /// Query the caller's current trading-account whitelist.
    pub async fn get_my_trading_accounts(
        &self,
    ) -> Result<Vec<Principal>, GetMyTradingAccountsError> {
        self.runtime
            .call(self.oisy_trade_canister, "get_my_trading_accounts", (), 0)
            .await
            .unwrap()
    }

    /// Point-lookup the caller's order by id. Returns `Err(OrderNotFound)` when
    /// the id is unknown or owned by another principal.
    pub async fn get_my_order(&self, order_id: OrderId) -> Result<UserOrder, GetMyOrdersError> {
        self.get_my_orders(GetMyOrdersArgs::by_id(order_id))
            .await
            .map(|orders| {
                orders
                    .into_iter()
                    .next()
                    .expect("BUG: ById query returned an empty page instead of OrderNotFound")
            })
    }

    /// Query all listed trading pairs on the OISY TRADE canister.
    pub async fn get_trading_pairs(&self) -> Vec<TradingPairInfo> {
        self.runtime
            .call(self.oisy_trade_canister, "get_trading_pairs", (), 0)
            .await
            .unwrap()
    }

    /// Query the top-of-book for a trading pair on the OISY TRADE canister.
    pub async fn get_order_book_ticker(
        &self,
        pair: TradingPair,
    ) -> Result<OrderBookTicker, GetOrderBookTickerError> {
        self.runtime
            .call(
                self.oisy_trade_canister,
                "get_order_book_ticker",
                (pair,),
                0,
            )
            .await
            .unwrap()
    }

    /// Query price-aggregated depth for a trading pair on the OISY TRADE canister.
    pub async fn get_order_book_depth(
        &self,
        request: GetOrderBookDepthRequest,
    ) -> Result<OrderBookDepth, GetOrderBookDepthError> {
        self.runtime
            .call(
                self.oisy_trade_canister,
                "get_order_book_depth",
                (request,),
                0,
            )
            .await
            .unwrap()
    }

    /// Deposit tokens into the OISY TRADE canister.
    pub async fn deposit(&self, request: DepositRequest) -> Result<DepositResponse, DepositError> {
        self.runtime
            .call(self.oisy_trade_canister, "deposit", (request,), 0)
            .await
            .unwrap()
    }

    /// Withdraw tokens from the OISY TRADE canister.
    pub async fn withdraw(
        &self,
        request: WithdrawRequest,
    ) -> Result<WithdrawResponse, WithdrawError> {
        self.runtime
            .call(self.oisy_trade_canister, "withdraw", (request,), 0)
            .await
            .unwrap()
    }

    /// Query the caller's balances. With no filter, returns every token
    /// the caller holds with non-zero balance. With a filter, returns
    /// one entry per requested token (zeros included). The whole call fails
    /// with `TokenNotSupported` if the filter references an unsupported token.
    pub async fn get_balances(
        &self,
        filter: Option<Vec<FilterToken>>,
    ) -> Result<Vec<UserTokenBalance>, GetBalancesError> {
        self.runtime
            .call(self.oisy_trade_canister, "get_balances", (filter,), 0)
            .await
            .unwrap()
    }

    /// Query the canister-owned fee pool. Mirrors [`Self::get_balances`].
    /// Each returned `Balance` carries the fee amount in `free`;
    /// `reserved` is always zero.
    pub async fn get_fee_balances(
        &self,
        filter: Option<Vec<FilterToken>>,
    ) -> Result<Vec<UserTokenBalance>, GetBalancesError> {
        self.runtime
            .call(self.oisy_trade_canister, "get_fee_balances", (filter,), 0)
            .await
            .unwrap()
    }

    /// Client-side convenience: query the caller's balance for a single
    /// token via [`Self::get_balances`]. Returns `TokenNotSupported` when
    /// the OISY TRADE does not know the token.
    pub async fn get_balance(&self, token_id: TokenId) -> Result<Balance, GetBalancesError> {
        let mut result = self
            .get_balances(Some(vec![FilterToken::ById(token_id)]))
            .await?;
        Ok(result.remove(0).balance)
    }

    /// List every token registered with the OISY TRADE.
    pub async fn list_supported_tokens(&self) -> Vec<Token> {
        self.runtime
            .call(self.oisy_trade_canister, "list_supported_tokens", (), 0)
            .await
            .unwrap()
    }

    /// Add a new trading pair to the OISY TRADE. Only callable by a controller.
    pub async fn add_trading_pair(
        &self,
        request: AddTradingPairRequest,
    ) -> Result<(), AddTradingPairError> {
        self.runtime
            .call(self.oisy_trade_canister, "add_trading_pair", (request,), 0)
            .await
            .unwrap()
    }

    /// Halt trading. `None` halts the whole DEX; `Some(pairs)` halts only those
    /// pairs. Only callable by a controller.
    pub async fn halt_trading(
        &self,
        pairs: Option<Vec<TradingPair>>,
    ) -> Result<(), UnauthorizedError> {
        self.runtime
            .call(self.oisy_trade_canister, "halt_trading", (pairs,), 0)
            .await
            .unwrap()
    }

    /// Resume trading. `None` clears the global halt and all per-pair halts;
    /// `Some(pairs)` resumes only those pairs. Only callable by a controller.
    pub async fn resume_trading(
        &self,
        pairs: Option<Vec<TradingPair>>,
    ) -> Result<(), UnauthorizedError> {
        self.runtime
            .call(self.oisy_trade_canister, "resume_trading", (pairs,), 0)
            .await
            .unwrap()
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
struct IcRuntime {}

#[async_trait]
impl Runtime for IcRuntime {
    async fn call<In, Out>(
        &self,
        id: Principal,
        method: &str,
        args: In,
        cycles: u128,
    ) -> Result<Out, (RejectCode, String)>
    where
        In: ArgumentEncoder + Send + 'static,
        Out: CandidType + DeserializeOwned + 'static,
    {
        let response = Call::bounded_wait(id, method)
            .with_args(&args)
            .with_cycles(cycles)
            .await
            .map_err(call_failed_to_reject)?;
        response
            .candid_tuple::<(Out,)>()
            .map(|(res,)| res)
            .map_err(|e| (RejectCode::CanisterError, e.to_string()))
    }
}

fn call_failed_to_reject(err: CallFailed) -> (RejectCode, String) {
    match err {
        CallFailed::CallRejected(rejected) => (
            rejected.reject_code().unwrap_or(RejectCode::SysUnknown),
            rejected.reject_message().to_string(),
        ),
        other => (RejectCode::CanisterError, other.to_string()),
    }
}
