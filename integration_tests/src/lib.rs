pub mod deposit_flow;
pub mod events;
pub mod fee_fill;
pub mod icrc_ledger;

pub use deposit_flow::DepositFlow;
pub use events::OisyTradeEventAssert;
pub use fee_fill::{FeeFillOutcome, fill_one_cross_with_fees};
pub use icrc_ledger::LedgerClient;

use async_trait::async_trait;
use candid::utils::ArgumentEncoder;
use candid::{CandidType, Decode, Encode, Nat, Principal, decode_args, encode_args};
use canlog::{Log, LogEntry};
use ic_cdk::call::RejectCode;
use ic_http_types::{HttpRequest, HttpResponse};
pub use ic_metrics_assert::{AsyncCanisterHttpQuery, MetricsAssert};
use icrc_ledger_types::icrc1::account::Account;
use oisy_trade_client::{OisyTradeClient, Runtime};
use oisy_trade_types::{
    AddTradingPairRequest, LimitOrderRequest, OrderId, Side, Token, TokenId, TokenMetadata,
    TradingPair, TradingStatus,
};
use oisy_trade_types_internal::{InitArg, Mode, OisyTradeArg, UpgradeArg, log::Priority};
use pocket_ic::{
    CanisterId, CanisterSettings, PocketIcBuilder, RejectResponse, nonblocking::PocketIc,
};
use serde::de::DeserializeOwned;
use std::path::PathBuf;

pub const TICK_SIZE: u64 = 1000;
pub const LOT_SIZE: u64 = 1_000_000;
/// Scales a whole-quote-per-whole-base price into the on-book representation
/// (quote smallest units per whole base token): `10^quote_decimals` (ckBTC = 8).
pub const PRICE_SCALE: u64 = 100_000_000;

pub struct Setup {
    env: Option<PocketIc>,
    caller: Principal,
    controller: Principal,
    oisy_trade_id: CanisterId,
    base_ledger_id: CanisterId,
    quote_ledger_id: CanisterId,
    base_config: icrc_ledger::LedgerConfig,
    quote_config: icrc_ledger::LedgerConfig,
}

/// Builder for [`Setup`]. Defaults to ckSOL / ckBTC ledgers and the default
/// init arg; override either before calling [`SetupBuilder::build`].
pub struct SetupBuilder {
    init_arg: InitArg,
    base_config: icrc_ledger::LedgerConfig,
    quote_config: icrc_ledger::LedgerConfig,
}

impl SetupBuilder {
    pub fn with_init_arg(mut self, init_arg: InitArg) -> Self {
        self.init_arg = init_arg;
        self
    }

    /// Use the given base/quote ledger configs (symbol, decimals, transfer fee).
    /// The submitted trading-pair metadata follows from these, so the registered
    /// pair matches what the ledgers report.
    pub fn with_ledgers(
        mut self,
        base_config: icrc_ledger::LedgerConfig,
        quote_config: icrc_ledger::LedgerConfig,
    ) -> Self {
        self.base_config = base_config;
        self.quote_config = quote_config;
        self
    }

    pub async fn build(self) -> Setup {
        const DEFAULT_CALLER_TEST_ID: Principal = Principal::from_slice(&[0x9d, 0xf7, 0x01]);
        const DEFAULT_CONTROLLER_TEST_ID: Principal = Principal::from_slice(&[0x9d, 0xf7, 0x02]);

        let env = PocketIcBuilder::new()
            .with_fiduciary_subnet()
            .build_async()
            .await;
        let controller = DEFAULT_CONTROLLER_TEST_ID;
        let canister_id = env
            .create_canister_with_settings(
                None,
                Some(CanisterSettings {
                    controllers: Some(vec![controller]),
                    ..CanisterSettings::default()
                }),
            )
            .await;
        env.add_cycles(canister_id, u128::MAX).await;
        env.install_canister(
            canister_id,
            oisy_trade_wasm(),
            Encode!(&OisyTradeArg::Init(self.init_arg)).unwrap(),
            Some(controller),
        )
        .await;

        let ledger_wasm = ledger_wasm();
        let base_ledger_id = icrc_ledger::install_ledger(
            &env,
            controller,
            ledger_wasm.clone(),
            icrc_ledger::ledger_init_args(controller, &self.base_config),
        )
        .await;
        let quote_ledger_id = icrc_ledger::install_ledger(
            &env,
            controller,
            ledger_wasm,
            icrc_ledger::ledger_init_args(controller, &self.quote_config),
        )
        .await;

        let caller = DEFAULT_CALLER_TEST_ID;

        Setup {
            env: Some(env),
            caller,
            controller,
            oisy_trade_id: canister_id,
            base_ledger_id,
            quote_ledger_id,
            base_config: self.base_config,
            quote_config: self.quote_config,
        }
    }
}

impl Setup {
    pub async fn new() -> Self {
        Self::builder().build().await
    }

    /// Start a [`Setup`] with default ledgers (ckSOL / ckBTC) and init arg.
    /// Override either with [`SetupBuilder::with_init_arg`] /
    /// [`SetupBuilder::with_ledgers`] before calling [`SetupBuilder::build`].
    pub fn builder() -> SetupBuilder {
        SetupBuilder {
            init_arg: default_init_arg(),
            base_config: icrc_ledger::LedgerConfig::cksol(),
            quote_config: icrc_ledger::LedgerConfig::ckbtc(),
        }
    }

    pub async fn with_trading_pair(self) -> Self {
        self.add_trading_pair().await;
        self
    }

    pub async fn add_trading_pair(&self) {
        let controller_client = self.oisy_trade_client_with_caller(self.controller());
        let result = controller_client
            .add_trading_pair(self.add_trading_pair_request())
            .await;
        assert_eq!(result, Ok(()));
    }

    pub fn add_trading_pair_request(&self) -> AddTradingPairRequest {
        let trading_pair = self.trading_pair();
        AddTradingPairRequest {
            base: Token {
                id: TokenId {
                    ledger_id: trading_pair.base,
                },
                metadata: TokenMetadata {
                    symbol: self.base_config.symbol.clone(),
                    decimals: self.base_config.decimals,
                },
            },
            quote: Token {
                id: TokenId {
                    ledger_id: trading_pair.quote,
                },
                metadata: TokenMetadata {
                    symbol: self.quote_config.symbol.clone(),
                    decimals: self.quote_config.decimals,
                },
            },
            tick_size: Nat::from(TICK_SIZE),
            lot_size: Nat::from(LOT_SIZE),
            maker_fee_bps: 0,
            taker_fee_bps: 0,
            min_notional: Nat::from(1u64),
            max_notional: None,
        }
    }

    pub fn trading_pair(&self) -> TradingPair {
        TradingPair {
            base: self.base_ledger_id,
            quote: self.quote_ledger_id,
        }
    }

    /// Reads the [`TradingStatus`] the canister reports for `pair` through the
    /// `get_trading_pairs` query, called from the anonymous principal.
    pub async fn pair_status(&self, pair: TradingPair) -> TradingStatus {
        self.oisy_trade_client_with_caller(Principal::anonymous())
            .get_trading_pairs()
            .await
            .into_iter()
            .find(|info| {
                info.base.id.ledger_id == pair.base && info.quote.id.ledger_id == pair.quote
            })
            .expect("trading pair must be listed")
            .status
    }

    /// Rest a GTC sell of `quantity` base units at `price`, providing liquidity
    /// a taker can cross against. Returns the resting order's id.
    pub async fn rest_sell_maker(
        &self,
        seller: Principal,
        quantity: u64,
        price: impl Into<Nat>,
    ) -> OrderId {
        let client = self.oisy_trade_client_with_caller(seller);
        self.fund_base(seller, quantity).await;
        let order_id = client
            .add_limit_order(LimitOrderRequest {
                pair: self.trading_pair(),
                side: Side::Sell,
                price: price.into(),
                quantity: quantity.into(),
                time_in_force: None,
            })
            .await
            .unwrap();
        self.env().tick().await;
        order_id
    }

    /// Mints, approves, and deposits `amount` base units for `who`.
    pub async fn fund_base(&self, who: Principal, amount: u64) {
        self.deposit_flow(who, self.base_token_id())
            .mint(amount + 2 * icrc_ledger::BASE_LEDGER_FEE)
            .approve(amount + icrc_ledger::BASE_LEDGER_FEE)
            .deposit(amount)
            .execute()
            .await;
    }

    /// Mints, approves, and deposits `amount` quote units for `who`.
    pub async fn fund_quote(&self, who: Principal, amount: u64) {
        self.deposit_flow(who, self.quote_token_id())
            .mint(amount + 2 * icrc_ledger::QUOTE_LEDGER_FEE)
            .approve(amount + icrc_ledger::QUOTE_LEDGER_FEE)
            .deposit(amount)
            .execute()
            .await;
    }

    /// Register a second, distinct trading pair reusing the two ledgers with
    /// base and quote swapped. It has its own order book, so per-pair controls
    /// can be exercised independently of [`Self::trading_pair`].
    pub async fn with_second_trading_pair(self) -> Self {
        let controller_client = self.oisy_trade_client_with_caller(self.controller());
        let request = self.add_trading_pair_request();
        let swapped = AddTradingPairRequest {
            base: request.quote,
            quote: request.base,
            ..request
        };
        assert_eq!(controller_client.add_trading_pair(swapped).await, Ok(()));
        self
    }

    pub fn second_trading_pair(&self) -> TradingPair {
        TradingPair {
            base: self.quote_ledger_id,
            quote: self.base_ledger_id,
        }
    }

    pub fn base_token_id(&self) -> TokenId {
        TokenId {
            ledger_id: self.base_ledger_id,
        }
    }

    pub fn quote_token_id(&self) -> TokenId {
        TokenId {
            ledger_id: self.quote_ledger_id,
        }
    }

    pub fn oisy_trade_account(&self) -> Account {
        Account {
            owner: self.oisy_trade_id,
            subaccount: None,
        }
    }

    pub fn oisy_trade_client(&self) -> OisyTradeClient<PocketIcRuntime<'_>> {
        OisyTradeClient::new(self.new_pocket_ic(), self.oisy_trade_id)
    }

    pub fn base_token_ledger(&self) -> LedgerClient<'_> {
        LedgerClient::new(self.env.as_ref().unwrap(), self.base_ledger_id)
    }

    pub fn quote_token_ledger(&self) -> LedgerClient<'_> {
        LedgerClient::new(self.env.as_ref().unwrap(), self.quote_ledger_id)
    }

    pub fn ledger_for(&self, token_id: &TokenId) -> LedgerClient<'_> {
        LedgerClient::new(self.env.as_ref().unwrap(), token_id.ledger_id)
    }

    pub fn deposit_flow(&self, user: Principal, token_id: TokenId) -> DepositFlow<'_> {
        DepositFlow::new(self, user, token_id)
    }

    pub fn user(&self) -> Principal {
        self.caller
    }

    pub fn controller(&self) -> Principal {
        self.controller
    }

    pub fn oisy_trade_id(&self) -> CanisterId {
        self.oisy_trade_id
    }

    pub fn base_ledger_id(&self) -> CanisterId {
        self.base_ledger_id
    }

    pub fn quote_ledger_id(&self) -> CanisterId {
        self.quote_ledger_id
    }

    pub fn env(&self) -> &PocketIc {
        self.env.as_ref().unwrap()
    }

    /// Current replica time as nanoseconds since the Unix epoch, matching the
    /// canister's `ic_cdk::api::time()` units.
    pub async fn time_nanos(&self) -> u64 {
        self.env().get_time().await.as_nanos_since_unix_epoch()
    }

    pub async fn mint_base_tokens(&self, to: Principal, amount: impl Into<Nat>) -> Nat {
        self.base_token_ledger()
            .icrc1_transfer(
                self.controller,
                Account {
                    owner: to,
                    subaccount: None,
                },
                amount,
            )
            .await
    }

    pub async fn mint_quote_tokens(&self, to: Principal, amount: impl Into<Nat>) -> Nat {
        self.quote_token_ledger()
            .icrc1_transfer(
                self.controller,
                Account {
                    owner: to,
                    subaccount: None,
                },
                amount,
            )
            .await
    }

    pub fn oisy_trade_client_with_caller(
        &self,
        caller: Principal,
    ) -> OisyTradeClient<PocketIcRuntime<'_>> {
        OisyTradeClient::new(
            PocketIcRuntime {
                env: self.env.as_ref().unwrap(),
                caller,
            },
            self.oisy_trade_id,
        )
    }

    fn new_pocket_ic(&self) -> PocketIcRuntime<'_> {
        PocketIcRuntime {
            env: self.env.as_ref().unwrap(),
            caller: self.caller,
        }
    }

    pub async fn fetch_dashboard(&self) -> String {
        let request = HttpRequest {
            method: "GET".to_string(),
            url: "/dashboard".to_string(),
            headers: vec![],
            body: Default::default(),
        };
        let response: HttpResponse = self
            .env()
            .query_call(
                self.oisy_trade_id,
                Principal::anonymous(),
                "http_request",
                Encode!(&request).unwrap(),
            )
            .await
            .map(|bytes| Decode!(&bytes, HttpResponse).unwrap())
            .expect("Failed to query http_request");
        assert_eq!(
            response.status_code,
            200,
            "dashboard request failed with body: {}",
            String::from_utf8_lossy(&response.body),
        );
        String::from_utf8(response.body.into_vec()).expect("dashboard body should be UTF-8")
    }

    pub async fn assert_metrics(&self) -> MetricsAssert<&Self> {
        MetricsAssert::from_async_http_query(self).await
    }

    pub async fn retrieve_logs(&self, priority: &Priority) -> Vec<LogEntry<Priority>> {
        let request = HttpRequest {
            method: "GET".to_string(),
            url: format!("/logs?priority={priority}"),
            headers: vec![],
            body: Default::default(),
        };
        let response: HttpResponse = self
            .env()
            .query_call(
                self.oisy_trade_id,
                Principal::anonymous(),
                "http_request",
                Encode!(&request).unwrap(),
            )
            .await
            .map(|bytes| Decode!(&bytes, HttpResponse).unwrap())
            .expect("Failed to query http_request");
        serde_json::from_slice::<Log<Priority>>(&response.body)
            .expect("Failed to deserialize logs")
            .entries
    }

    pub async fn upgrade(&self, upgrade_arg: Option<UpgradeArg>) {
        let arg = OisyTradeArg::Upgrade(upgrade_arg);
        self.env()
            .stop_canister(self.oisy_trade_id, Some(self.controller))
            .await
            .expect("failed to stop OISY TRADE");
        self.env()
            .upgrade_canister(
                self.oisy_trade_id,
                oisy_trade_wasm(),
                Encode!(&arg).unwrap(),
                Some(self.controller),
            )
            .await
            .expect("failed to upgrade OISY TRADE canister");
        self.env()
            .start_canister(self.oisy_trade_id, Some(self.controller))
            .await
            .expect("failed to start OISY TRADE after upgrade");
    }

    pub async fn assert_that_events(&self) -> OisyTradeEventAssert {
        OisyTradeEventAssert::new(self.get_all_events().await)
    }

    pub async fn get_all_events(&self) -> Vec<oisy_trade_types_internal::event::Event> {
        use oisy_trade_types_internal::event::GetEventsResult;

        const FIRST_BATCH_SIZE: u64 = 100;

        let GetEventsResult {
            mut events,
            total_event_count,
        } = self.get_events(0, FIRST_BATCH_SIZE).await;
        while events.len() < total_event_count as usize {
            let mut next_batch = self
                .get_events(events.len() as u64, total_event_count - events.len() as u64)
                .await;
            events.append(&mut next_batch.events);
        }
        events
    }

    async fn get_events(
        &self,
        start: u64,
        length: u64,
    ) -> oisy_trade_types_internal::event::GetEventsResult {
        use oisy_trade_types_internal::event::{GetEventsArgs, GetEventsResult};

        self.env()
            .query_call(
                self.oisy_trade_id,
                Principal::anonymous(),
                "get_events",
                Encode!(&GetEventsArgs { start, length }).unwrap(),
            )
            .await
            .map(|bytes| Decode!(&bytes, GetEventsResult).unwrap())
            .expect("BUG: failed to call get_events")
    }

    pub async fn drop(self) {
        let mut setup = self;
        if let Some(env) = setup.env.take() {
            env.drop().await
        }
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        if self.env.is_some() && !std::thread::panicking() {
            panic!("Setup was not dropped properly. Call Setup::drop().await to clean up.");
        }
    }
}

fn default_init_arg() -> InitArg {
    InitArg {
        mode: Mode::GeneralAvailability,
        max_orders_per_chunk: oisy_trade_types_internal::DEFAULT_MAX_ORDERS_PER_CHUNK,
        instruction_budget: oisy_trade_types_internal::DEFAULT_INSTRUCTION_BUDGET,
        max_settlement_units_per_event: Some(
            oisy_trade_types_internal::DEFAULT_MAX_SETTLEMENT_UNITS_PER_EVENT,
        ),
    }
}

pub fn oisy_trade_wasm() -> Vec<u8> {
    let path = std::env::var("OISY_TRADE_CANISTER_WASM_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
                .join("../wasms/oisy_trade_canister.wasm.gz")
        });
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to read OISY TRADE WASM at {}: {}\nRun `just build` first.",
            path.display(),
            e
        )
    })
}

pub fn ledger_wasm() -> Vec<u8> {
    let path = std::env::var("IC_ICRC1_LEDGER_WASM_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
                .join("../wasms/ic-icrc1-ledger.wasm.gz")
        });
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to read ledger WASM at {}: {}\nRun `just download-external-wasms` first.",
            path.display(),
            e
        )
    })
}

#[async_trait]
impl AsyncCanisterHttpQuery<RejectResponse> for &Setup {
    async fn http_query(&self, request: Vec<u8>) -> Result<Vec<u8>, RejectResponse> {
        self.env()
            .query_call(
                self.oisy_trade_id,
                Principal::anonymous(),
                "http_request",
                request,
            )
            .await
    }
}

#[derive(Clone)]
pub struct PocketIcRuntime<'a> {
    env: &'a PocketIc,
    caller: Principal,
}

#[async_trait]
impl<'a> Runtime for PocketIcRuntime<'a> {
    async fn call<In, Out>(
        &self,
        id: Principal,
        method: &str,
        args: In,
        _cycles: u128,
    ) -> Result<Out, (RejectCode, String)>
    where
        In: ArgumentEncoder + Send + 'static,
        Out: CandidType + DeserializeOwned + 'static,
    {
        let args_raw = encode_args(args).expect("Failed to encode arguments.");
        match self
            .env
            .update_call(id, self.caller, method, args_raw)
            .await
        {
            Ok(bytes) => decode_args(&bytes).map(|(res,)| res).map_err(|e| {
                (
                    RejectCode::CanisterError,
                    format!(
                        "failed to decode canister response as {}: {}",
                        std::any::type_name::<Out>(),
                        e
                    ),
                )
            }),
            Err(e) => {
                let rejection_code = match e.reject_code {
                    pocket_ic::RejectCode::SysFatal => RejectCode::SysFatal,
                    pocket_ic::RejectCode::SysTransient => RejectCode::SysTransient,
                    pocket_ic::RejectCode::DestinationInvalid => RejectCode::DestinationInvalid,
                    pocket_ic::RejectCode::CanisterReject => RejectCode::CanisterReject,
                    pocket_ic::RejectCode::CanisterError => RejectCode::CanisterError,
                    pocket_ic::RejectCode::SysUnknown => RejectCode::SysUnknown,
                };
                Err((rejection_code, e.reject_message))
            }
        }
    }
}
