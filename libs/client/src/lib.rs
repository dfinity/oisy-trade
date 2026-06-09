//! Client to interact with the DEX canister

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

use async_trait::async_trait;
use candid::utils::ArgumentEncoder;
use candid::{CandidType, Principal};
use dex_types::{
    AddLimitOrderError, AddTradingPairError, AddTradingPairRequest, Balance, CancelLimitOrderError,
    DepositError, DepositRequest, DepositResponse, FilterToken, GetBalancesError,
    GetBalancesRequestError, GetMyOrdersArgs, GetOrderBookDepthError, GetOrderBookDepthRequest,
    GetOrderBookTickerError, LimitOrderRequest, OrderBookDepth, OrderBookTicker, OrderId,
    OrderRecord, OrderStatus, Token, TokenId, TradingPair, TradingPairInfo, UnauthorizedError,
    UserOrder, UserTokenBalance, WithdrawError, WithdrawRequest, WithdrawResponse,
};
use ic_cdk::call::{Call, CallFailed, RejectCode};
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

/// Client to interact with the DEX canister.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DexClient<R: Runtime> {
    runtime: R,
    dex_canister: Principal,
}

impl DexClient<IcRuntime> {
    /// Instantiate a new client to be used by a canister on the Internet Computer.
    ///
    /// To use another runtime, see [`Self::new`].
    pub fn new_for_ic(dex_canister: Principal) -> Self {
        Self {
            runtime: IcRuntime {},
            dex_canister,
        }
    }
}

impl<R: Runtime> DexClient<R> {
    /// Instantiate a new client with a specific runtime.
    ///
    /// To use the client inside a canister, see [`DexClient<IcRuntime>::new_for_ic`].
    pub fn new(runtime: R, dex_canister: Principal) -> Self {
        Self {
            runtime,
            dex_canister,
        }
    }

    /// Place a new limit order on the DEX canister.
    pub async fn add_limit_order(
        &self,
        request: LimitOrderRequest,
    ) -> Result<OrderId, AddLimitOrderError> {
        self.runtime
            .call(self.dex_canister, "add_limit_order", (request,), 0)
            .await
            .unwrap()
    }

    /// Cancel a limit order previously submitted by the caller.
    pub async fn cancel_limit_order(
        &self,
        order_id: OrderId,
    ) -> Result<OrderRecord, CancelLimitOrderError> {
        self.runtime
            .call(self.dex_canister, "cancel_limit_order", (order_id,), 0)
            .await
            .unwrap()
    }

    /// Query the status of an existing order on the DEX canister.
    pub async fn get_order_status(&self, order_id: OrderId) -> OrderStatus {
        self.runtime
            .call(self.dex_canister, "get_order_status", (order_id,), 0)
            .await
            .unwrap()
    }

    /// Query the caller's orders, newest first, paginated.
    pub async fn get_my_orders(&self, args: GetMyOrdersArgs) -> Vec<UserOrder> {
        self.runtime
            .call(self.dex_canister, "get_my_orders", (args,), 0)
            .await
            .unwrap()
    }

    /// Query all listed trading pairs on the DEX canister.
    pub async fn get_trading_pairs(&self) -> Vec<TradingPairInfo> {
        self.runtime
            .call(self.dex_canister, "get_trading_pairs", (), 0)
            .await
            .unwrap()
    }

    /// Query the top-of-book for a trading pair on the DEX canister.
    pub async fn get_order_book_ticker(
        &self,
        pair: TradingPair,
    ) -> Result<OrderBookTicker, GetOrderBookTickerError> {
        self.runtime
            .call(self.dex_canister, "get_order_book_ticker", (pair,), 0)
            .await
            .unwrap()
    }

    /// Query price-aggregated depth for a trading pair on the DEX canister.
    pub async fn get_order_book_depth(
        &self,
        request: GetOrderBookDepthRequest,
    ) -> Result<OrderBookDepth, GetOrderBookDepthError> {
        self.runtime
            .call(self.dex_canister, "get_order_book_depth", (request,), 0)
            .await
            .unwrap()
    }

    /// Deposit tokens into the DEX canister.
    pub async fn deposit(&self, request: DepositRequest) -> Result<DepositResponse, DepositError> {
        self.runtime
            .call(self.dex_canister, "deposit", (request,), 0)
            .await
            .unwrap()
    }

    /// Withdraw tokens from the DEX canister.
    pub async fn withdraw(
        &self,
        request: WithdrawRequest,
    ) -> Result<WithdrawResponse, WithdrawError> {
        self.runtime
            .call(self.dex_canister, "withdraw", (request,), 0)
            .await
            .unwrap()
    }

    /// Query the caller's balances. With no filter, returns every token
    /// the caller holds with non-zero balance. With a filter, returns
    /// one entry per requested token (zeros included; unsupported
    /// tokens reported per-entry as `TokenNotSupported`).
    pub async fn get_balances(
        &self,
        filter: Option<Vec<FilterToken>>,
    ) -> Result<Vec<Result<UserTokenBalance, GetBalancesError>>, GetBalancesRequestError> {
        self.runtime
            .call(self.dex_canister, "get_balances", (filter,), 0)
            .await
            .unwrap()
    }

    /// Query the canister-owned fee pool. Mirrors [`Self::get_balances`].
    /// Each returned `Balance` carries the fee amount in `free`;
    /// `reserved` is always zero.
    pub async fn get_fee_balances(
        &self,
        filter: Option<Vec<FilterToken>>,
    ) -> Result<Vec<Result<UserTokenBalance, GetBalancesError>>, GetBalancesRequestError> {
        self.runtime
            .call(self.dex_canister, "get_fee_balances", (filter,), 0)
            .await
            .unwrap()
    }

    /// Client-side convenience: query the caller's balance for a single
    /// token via [`Self::get_balances`]. Returns `TokenNotSupported` when
    /// the DEX does not know the token.
    pub async fn get_balance(&self, token_id: TokenId) -> Result<Balance, GetBalancesError> {
        let mut result = self
            .get_balances(Some(vec![FilterToken::ById(token_id)]))
            .await
            .expect("single-element filter is always within MAX_FILTER_LEN");
        result.remove(0).map(|entry| entry.balance)
    }

    /// List every token registered with the DEX.
    pub async fn list_supported_tokens(&self) -> Vec<Token> {
        self.runtime
            .call(self.dex_canister, "list_supported_tokens", (), 0)
            .await
            .unwrap()
    }

    /// Add a new trading pair to the DEX. Only callable by a controller.
    pub async fn add_trading_pair(
        &self,
        request: AddTradingPairRequest,
    ) -> Result<(), AddTradingPairError> {
        self.runtime
            .call(self.dex_canister, "add_trading_pair", (request,), 0)
            .await
            .unwrap()
    }

    /// Globally halt trading on the DEX canister. Only callable by a controller.
    pub async fn halt_trading(&self) -> Result<(), UnauthorizedError> {
        self.runtime
            .call(self.dex_canister, "halt_trading", (), 0)
            .await
            .unwrap()
    }

    /// Resume trading after a global halt. Only callable by a controller.
    pub async fn resume_trading(&self) -> Result<(), UnauthorizedError> {
        self.runtime
            .call(self.dex_canister, "resume_trading", (), 0)
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
