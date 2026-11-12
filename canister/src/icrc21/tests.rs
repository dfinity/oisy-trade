use crate::icrc21::{build_consent_info, format_amount, format_bps, icrc10_supported_standards};
use crate::order::{
    BasisPoint, FeeRates, LotSize, OrderBookId, OrderId, OrderSeq, Quantity, TickSize, TradingPair,
};
use crate::state::State;
use crate::test_fixtures::tokens::SupportedTokens;
use crate::test_fixtures::{self, ckusdt_token_id, icp_ckusdt_trading_pair, icp_token_id};
use candid::{Encode, Nat, Principal};
use ic_stable_structures::VectorMemory;
use icrc_ledger_types::icrc21::errors::Icrc21Error;
use icrc_ledger_types::icrc21::lib::MAX_CONSENT_MESSAGE_ARG_SIZE_BYTES;
use icrc_ledger_types::icrc21::requests::{
    ConsentMessageMetadata, ConsentMessageRequest, ConsentMessageSpec, DisplayMessageType,
};
use icrc_ledger_types::icrc21::responses::{ConsentInfo, ConsentMessage, FieldsDisplay, Value};
use oisy_trade_types::{DepositRequest, LimitOrderRequest, Side, TimeInForce, WithdrawRequest};
use std::num::{NonZeroU64, NonZeroU128};

const ICP: u64 = 100_000_000;
const CKUSDT: u64 = 1_000_000;
const CKETH: u128 = 1_000_000_000_000_000_000;
const TRADING_ACCOUNT: &str = "2vxsx-fae";

/// A state with two trading pairs:
/// * ICP/ckUSDT (book 0): tick 0.001 ckUSDT, lot 0.01 ICP, 0.1% maker and 0.2%
///   taker fee;
/// * ckETH/ckUSDT (book 1): tick 0.01 ckUSDT, lot 0.0001 ckETH, no fees.
fn state() -> State<VectorMemory, VectorMemory> {
    let mut state = test_fixtures::state();
    state.record_trading_pair(
        OrderBookId::ZERO,
        icp_ckusdt_trading_pair(),
        SupportedTokens::ICP.token_metadata().into(),
        SupportedTokens::CKUSDT.token_metadata().into(),
        TickSize::new(NonZeroU128::new(1_000).unwrap()),
        LotSize::new(NonZeroU64::new(1_000_000).unwrap()),
        Quantity::from(1u64),
        None,
        FeeRates {
            maker: BasisPoint::new(10).unwrap(),
            taker: BasisPoint::new(20).unwrap(),
        },
    );
    state.record_trading_pair(
        OrderBookId::new(1),
        cketh_ckusdt_trading_pair(),
        SupportedTokens::CKETH.token_metadata().into(),
        SupportedTokens::CKUSDT.token_metadata().into(),
        TickSize::new(NonZeroU128::new(10_000).unwrap()),
        LotSize::new(NonZeroU64::new(100_000_000_000_000).unwrap()),
        Quantity::from(1u64),
        None,
        FeeRates::default(),
    );
    state
}

fn cketh_ckusdt_trading_pair() -> TradingPair {
    TradingPair {
        base: SupportedTokens::CKETH.token_id().into(),
        quote: ckusdt_token_id(),
    }
}

fn make_request(
    method: &str,
    arg: Vec<u8>,
    device_spec: Option<DisplayMessageType>,
) -> ConsentMessageRequest {
    ConsentMessageRequest {
        method: method.to_string(),
        arg,
        user_preferences: ConsentMessageSpec {
            metadata: ConsentMessageMetadata {
                language: "en".to_string(),
                utc_offset_minutes: None,
            },
            device_spec,
        },
    }
}

fn consent_info(
    method: &str,
    arg: Vec<u8>,
    device_spec: Option<DisplayMessageType>,
) -> Result<ConsentInfo, Icrc21Error> {
    build_consent_info(make_request(method, arg, device_spec), &state())
}

fn generic_message(method: &str, arg: Vec<u8>) -> String {
    let info = consent_info(method, arg, Some(DisplayMessageType::GenericDisplay)).unwrap();
    assert_eq!(info.metadata.language, "en");
    match info.consent_message {
        ConsentMessage::GenericDisplayMessage(m) => m,
        other => panic!("expected GenericDisplayMessage, got {other:?}"),
    }
}

fn fields_message(method: &str, arg: Vec<u8>) -> FieldsDisplay {
    let info = consent_info(method, arg, Some(DisplayMessageType::FieldsDisplay)).unwrap();
    match info.consent_message {
        ConsentMessage::FieldsDisplayMessage(f) => f,
        other => panic!("expected FieldsDisplayMessage, got {other:?}"),
    }
}

fn unsupported_description(result: Result<ConsentInfo, Icrc21Error>) -> String {
    match result {
        Err(Icrc21Error::UnsupportedCanisterCall(info)) => info.description,
        other => panic!("expected UnsupportedCanisterCall, got {other:?}"),
    }
}

fn text(content: &str) -> Value {
    Value::Text {
        content: content.to_string(),
    }
}

fn token_amount(amount: u64, token: SupportedTokens) -> Value {
    Value::TokenAmount {
        decimals: token.decimals(),
        amount,
        symbol: token.symbol().to_string(),
    }
}

fn fields(fields: &[(&str, Value)]) -> Vec<(String, Value)> {
    fields
        .iter()
        .map(|(label, value)| (label.to_string(), value.clone()))
        .collect()
}

fn deposit_request(token: SupportedTokens, amount: impl Into<Nat>) -> Vec<u8> {
    Encode!(&DepositRequest {
        token_id: token.token_id(),
        amount: amount.into(),
    })
    .unwrap()
}

fn withdraw_request(token: SupportedTokens, amount: impl Into<Nat>) -> Vec<u8> {
    Encode!(&WithdrawRequest {
        token_id: token.token_id(),
        amount: amount.into(),
    })
    .unwrap()
}

/// A limit order on ICP/ckUSDT for 0.5 ICP at 10.5 ckUSDT per ICP.
fn limit_order_request(side: Side, time_in_force: Option<TimeInForce>) -> LimitOrderRequest {
    LimitOrderRequest {
        pair: icp_ckusdt_trading_pair().into(),
        side,
        price: Nat::from(10_500_000u64),
        quantity: Nat::from(50_000_000u64),
        time_in_force,
    }
}

fn icp_ckusdt_order_id(seq: u64) -> String {
    OrderId::new(OrderBookId::ZERO, OrderSeq::new(seq)).to_string()
}

#[test]
fn should_format_amount() {
    for (amount, decimals, expected) in [
        (0_u128, 8, "0"),
        (1, 8, "0.00000001"),
        (100_000_000, 8, "1"),
        (150_000_000, 8, "1.5"),
        (123_456_789, 8, "1.23456789"),
        (5_250_000, 6, "5.25"),
        (42, 0, "42"),
        (25, 2, "0.25"),
        (u128::MAX, 18, "340282366920938463463.374607431768211455"),
    ] {
        assert_eq!(format_amount(&Nat::from(amount), decimals), expected);
    }
    // Amounts are `nat` and may exceed u128.
    assert_eq!(
        format_amount(&Quantity::MAX.into(), 18),
        "115792089237316195423570985008687907853269984665640564039457.584007913129639935"
    );
}

#[test]
fn should_format_bps() {
    assert_eq!(format_bps(0), "0%");
    assert_eq!(format_bps(1), "0.01%");
    assert_eq!(format_bps(25), "0.25%");
    assert_eq!(format_bps(100), "1%");
    assert_eq!(format_bps(10_000), "100%");
}

#[test]
fn should_advertise_icrc10_and_icrc21() {
    let standards = icrc10_supported_standards();
    let names: Vec<_> = standards.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["ICRC-10", "ICRC-21"]);
    assert_eq!(
        standards[1].url,
        "https://github.com/dfinity/ICRC/blob/main/ICRCs/ICRC-21/ICRC-21.md"
    );
}

#[test]
fn should_reject_unsupported_method() {
    // Admin endpoints are controller-only and not signed by wallet users, and
    // queries do not need a consent message.
    for method in [
        "add_trading_pair",
        "halt_trading",
        "resume_trading",
        "get_balances",
        "get_my_orders",
        "get_events",
        "icrc21_canister_call_consent_message",
        "",
    ] {
        let description = unsupported_description(consent_info(method, Encode!().unwrap(), None));
        assert_eq!(
            description,
            format!("The method '{method}' is not supported by the OISY TRADE ICRC-21 endpoint.")
        );
    }
}

#[test]
fn should_reject_argument_too_large() {
    let description = unsupported_description(consent_info(
        "deposit",
        vec![0; MAX_CONSENT_MESSAGE_ARG_SIZE_BYTES as usize + 1],
        None,
    ));
    assert!(description.contains("argument size is too large"));
}

#[test]
fn should_reject_undecodable_args() {
    for (method, type_name) in [
        ("deposit", "DepositRequest"),
        ("withdraw", "WithdrawRequest"),
        ("add_limit_order", "LimitOrderRequest"),
        ("cancel_limit_order", "OrderId"),
        ("add_trading_account", "principal"),
        ("remove_trading_account", "principal"),
    ] {
        let description = unsupported_description(consent_info(method, vec![1, 2, 3], None));
        assert!(
            description.starts_with(&format!("Failed to decode {type_name}")),
            "unexpected error description for {method}: {description}"
        );
        // Well-formed Candid of the wrong type is rejected too.
        let description =
            unsupported_description(consent_info(method, Encode!(&42_u8).unwrap(), None));
        assert!(description.starts_with(&format!("Failed to decode {type_name}")));
    }
}

#[test]
fn should_default_to_generic_display_and_echo_utc_offset() {
    let mut request = make_request(
        "deposit",
        deposit_request(SupportedTokens::CKUSDT, CKUSDT),
        None,
    );
    request.user_preferences.metadata.language = "de".to_string();
    request.user_preferences.metadata.utc_offset_minutes = Some(120);
    let info = build_consent_info(request, &state()).unwrap();
    assert_eq!(
        info.metadata,
        ConsentMessageMetadata {
            language: "en".to_string(),
            utc_offset_minutes: Some(120),
        }
    );
    assert!(matches!(
        info.consent_message,
        ConsentMessage::GenericDisplayMessage(_)
    ));
}

mod deposit {
    use super::*;

    #[test]
    fn should_render_generic_display() {
        assert_eq!(
            generic_message(
                "deposit",
                deposit_request(SupportedTokens::CKUSDT, 1_500_000u64)
            ),
            "# Deposit ckUSDT to OISY TRADE\n\n\
             Authorize OISY TRADE to transfer ckUSDT from your wallet, using the allowance \
             you approved for OISY TRADE, and credit it to your OISY TRADE balance.\n\n\
             **Amount:** `1.5 ckUSDT`\n\n\
             **Fees:** The ckUSDT ledger transfer fee is charged to your wallet on top of \
             the amount."
        );
    }

    #[test]
    fn should_render_fields_display() {
        assert_eq!(
            fields_message("deposit", deposit_request(SupportedTokens::ICP, 2 * ICP)),
            FieldsDisplay {
                intent: "Deposit ICP".to_string(),
                fields: fields(&[
                    ("Amount", token_amount(2 * ICP, SupportedTokens::ICP)),
                    ("Fees", text("Ledger fee, charged on top of the amount")),
                ]),
            }
        );
    }

    #[test]
    fn should_fall_back_to_text_for_amounts_exceeding_u64() {
        let amount = 100 * CKETH;
        assert_eq!(
            fields_message("deposit", deposit_request(SupportedTokens::CKETH, amount)).fields[0],
            ("Amount".to_string(), text("100 ckETH"))
        );
    }

    #[test]
    fn should_reject_unsupported_token() {
        let description = unsupported_description(consent_info(
            "deposit",
            deposit_request(SupportedTokens::CKBTC, ICP),
            None,
        ));
        assert_eq!(
            description,
            format!(
                "Unsupported token: {}.",
                SupportedTokens::CKBTC.token_id().ledger_id
            )
        );
    }

    #[test]
    fn should_reject_amount_exceeding_maximum() {
        let too_large = Nat::from(Quantity::MAX) + Nat::from(1u64);
        let description = unsupported_description(consent_info(
            "deposit",
            deposit_request(SupportedTokens::CKUSDT, too_large),
            None,
        ));
        assert_eq!(
            description,
            "The amount exceeds the maximum supported value."
        );
    }
}

mod withdraw {
    use super::*;

    #[test]
    fn should_render_generic_display() {
        assert_eq!(
            generic_message("withdraw", withdraw_request(SupportedTokens::ICP, ICP / 4)),
            "# Withdraw ICP from OISY TRADE\n\n\
             Withdraw ICP from your free OISY TRADE balance to your wallet.\n\n\
             **Amount:** `0.25 ICP`\n\n\
             **Fees:** The ICP ledger transfer fee is deducted from the amount, so you \
             receive the amount minus the fee."
        );
    }

    #[test]
    fn should_render_fields_display() {
        assert_eq!(
            fields_message(
                "withdraw",
                withdraw_request(SupportedTokens::CKUSDT, 3 * CKUSDT)
            ),
            FieldsDisplay {
                intent: "Withdraw ckUSDT".to_string(),
                fields: fields(&[
                    ("Amount", token_amount(3 * CKUSDT, SupportedTokens::CKUSDT)),
                    ("Fees", text("Ledger fee, deducted from the amount")),
                ]),
            }
        );
    }

    #[test]
    fn should_reject_unsupported_token() {
        let description = unsupported_description(consent_info(
            "withdraw",
            withdraw_request(SupportedTokens::VCHF, ICP),
            None,
        ));
        assert_eq!(
            description,
            format!(
                "Unsupported token: {}.",
                SupportedTokens::VCHF.token_id().ledger_id
            )
        );
    }
}

mod add_limit_order {
    use super::*;

    #[test]
    fn should_render_generic_display_for_buy() {
        assert_eq!(
            generic_message(
                "add_limit_order",
                Encode!(&limit_order_request(Side::Buy, None)).unwrap()
            ),
            "# Buy ICP on OISY TRADE\n\n\
             Place a limit order to buy ICP with ckUSDT on OISY TRADE, at the limit price \
             or better. The order uses your OISY TRADE balance.\n\n\
             **Market:** `ICP/ckUSDT`\n\n\
             **Side:** `Buy`\n\n\
             **Quantity:** `0.5 ICP`\n\n\
             **Limit price:** `10.5 ckUSDT per ICP`\n\n\
             **Reserved:** `5.25 ckUSDT`\n\
             Locked from your free OISY TRADE balance while the order is open; any amount \
             not spent is released back to it.\n\n\
             **Time in force:** `Good 'til canceled`\n\
             The order stays in the order book until it is filled or canceled.\n\n\
             **Trading fee:** `0.1% maker / 0.2% taker`\n\
             Deducted from the ICP you receive, rounded up on each fill."
        );
    }

    #[test]
    fn should_render_generic_display_for_fill_or_kill_sell() {
        assert_eq!(
            generic_message(
                "add_limit_order",
                Encode!(&limit_order_request(
                    Side::Sell,
                    Some(TimeInForce::FillOrKill)
                ))
                .unwrap()
            ),
            "# Sell ICP on OISY TRADE\n\n\
             Place a limit order to sell ICP for ckUSDT on OISY TRADE, at the limit price \
             or better. The order uses your OISY TRADE balance.\n\n\
             **Market:** `ICP/ckUSDT`\n\n\
             **Side:** `Sell`\n\n\
             **Quantity:** `0.5 ICP`\n\n\
             **Limit price:** `10.5 ckUSDT per ICP`\n\n\
             **Reserved:** `0.5 ICP`\n\
             Locked from your free OISY TRADE balance while the order is open; any amount \
             not spent is released back to it.\n\n\
             **Time in force:** `Fill or kill`\n\
             When the order is matched, it must be filled in full, otherwise it \
             expires without any trade.\n\n\
             **Trading fee:** `0.2% taker`\n\
             Deducted from the ckUSDT you receive, rounded up on each fill."
        );
    }

    #[test]
    fn should_render_fields_display_for_buy() {
        assert_eq!(
            fields_message(
                "add_limit_order",
                Encode!(&limit_order_request(
                    Side::Buy,
                    Some(TimeInForce::GoodTilCanceled)
                ))
                .unwrap()
            ),
            FieldsDisplay {
                intent: "Buy ICP".to_string(),
                fields: fields(&[
                    ("Market", text("ICP/ckUSDT")),
                    ("Side", text("Buy")),
                    ("Quantity", token_amount(ICP / 2, SupportedTokens::ICP)),
                    ("Limit price", text("10.5 ckUSDT per ICP")),
                    ("Reserved", token_amount(5_250_000, SupportedTokens::CKUSDT)),
                    ("Time in force", text("Good 'til canceled")),
                    ("Trading fee", text("0.1% maker / 0.2% taker")),
                ]),
            }
        );
    }

    #[test]
    fn should_render_fields_display_for_sell() {
        let fields_display = fields_message(
            "add_limit_order",
            Encode!(&limit_order_request(Side::Sell, None)).unwrap(),
        );
        assert_eq!(fields_display.intent, "Sell ICP");
        assert_eq!(
            fields_display.fields[4],
            (
                "Reserved".to_string(),
                token_amount(ICP / 2, SupportedTokens::ICP)
            )
        );
    }

    #[test]
    fn should_render_amounts_exceeding_u64_as_text() {
        // Buy 20 ckETH (18 decimals, exceeds u64) at 2_500 ckUSDT per ckETH:
        // the quantity is rendered as text, the 50_000 ckUSDT notional still
        // fits a TokenAmount.
        let request = LimitOrderRequest {
            pair: cketh_ckusdt_trading_pair().into(),
            side: Side::Buy,
            price: Nat::from(2_500 * CKUSDT),
            quantity: Nat::from(20 * CKETH),
            time_in_force: None,
        };
        assert_eq!(
            fields_message("add_limit_order", Encode!(&request).unwrap()),
            FieldsDisplay {
                intent: "Buy ckETH".to_string(),
                fields: fields(&[
                    ("Market", text("ckETH/ckUSDT")),
                    ("Side", text("Buy")),
                    ("Quantity", text("20 ckETH")),
                    ("Limit price", text("2500 ckUSDT per ckETH")),
                    (
                        "Reserved",
                        token_amount(50_000 * CKUSDT, SupportedTokens::CKUSDT)
                    ),
                    ("Time in force", text("Good 'til canceled")),
                    ("Trading fee", text("0% maker / 0% taker")),
                ]),
            }
        );
    }

    #[test]
    fn should_reject_unknown_trading_pair() {
        // Registered tokens, but not as a pair in this orientation.
        let mut request = limit_order_request(Side::Buy, None);
        request.pair = TradingPair {
            base: ckusdt_token_id(),
            quote: icp_token_id(),
        }
        .into();
        let description = unsupported_description(consent_info(
            "add_limit_order",
            Encode!(&request).unwrap(),
            None,
        ));
        assert_eq!(
            description,
            format!(
                "Unknown trading pair: {}/{}.",
                SupportedTokens::CKUSDT.token_id().ledger_id,
                SupportedTokens::ICP.token_id().ledger_id
            )
        );
    }

    #[test]
    fn should_reject_invalid_price_or_quantity() {
        let invalid_price = LimitOrderRequest {
            price: Nat::from(10_500_001u64),
            ..limit_order_request(Side::Buy, None)
        };
        let zero_price = LimitOrderRequest {
            price: Nat::from(0u64),
            ..limit_order_request(Side::Buy, None)
        };
        let invalid_quantity = LimitOrderRequest {
            quantity: Nat::from(50_000_001u64),
            ..limit_order_request(Side::Sell, None)
        };
        let price_too_large = LimitOrderRequest {
            price: Nat::from(u128::MAX) + Nat::from(1u64),
            ..limit_order_request(Side::Buy, None)
        };
        for (request, expected) in [
            (
                invalid_price,
                "The price is not a positive multiple of the tick size 1_000.",
            ),
            (
                zero_price,
                "The price is not a positive multiple of the tick size 1_000.",
            ),
            (
                invalid_quantity,
                "The quantity is not a positive multiple of the lot size 1_000_000.",
            ),
            (
                price_too_large,
                "The price or quantity exceeds the maximum supported value.",
            ),
        ] {
            let description = unsupported_description(consent_info(
                "add_limit_order",
                Encode!(&request).unwrap(),
                None,
            ));
            assert_eq!(description, expected);
        }
    }
}

mod cancel_limit_order {
    use super::*;

    #[test]
    fn should_render_generic_display() {
        let order_id = icp_ckusdt_order_id(42);
        assert_eq!(
            generic_message("cancel_limit_order", Encode!(&order_id).unwrap()),
            "# Cancel order on OISY TRADE\n\n\
             Cancel your limit order on the OISY TRADE ICP/ckUSDT market. The unfilled part \
             of the order is removed from the order book and its reserved funds are released \
             to your free OISY TRADE balance.\n\n\
             **Market:** `ICP/ckUSDT`\n\n\
             **Order ID:**\n`0000000000000000000000000000002a`"
        );
    }

    #[test]
    fn should_render_fields_display() {
        let order_id = OrderId::new(OrderBookId::new(1), OrderSeq::new(7)).to_string();
        assert_eq!(
            fields_message("cancel_limit_order", Encode!(&order_id).unwrap()),
            FieldsDisplay {
                intent: "Cancel order".to_string(),
                fields: fields(&[
                    ("Market", text("ckETH/ckUSDT")),
                    ("Order ID", text("00000000000000010000000000000007")),
                ]),
            }
        );
    }

    #[test]
    fn should_reject_malformed_order_id() {
        // The order ID is free-form text in the Candid interface: anything the
        // canister would not parse must not be interpolated into the Markdown
        // consent message.
        let valid = icp_ckusdt_order_id(42);
        for bad_order_id in [
            String::new(),
            "not-an-order-id".to_string(),
            valid.to_uppercase(),
            format!("{valid}0"),
            format!("{valid}`\n\n**Market:** `ckBTC/ICP`"),
            format!("{valid}\n# You will receive 100 ICP"),
        ] {
            let description = unsupported_description(consent_info(
                "cancel_limit_order",
                Encode!(&bad_order_id).unwrap(),
                Some(DisplayMessageType::GenericDisplay),
            ));
            assert_eq!(description, "Invalid order ID.");
        }
    }

    #[test]
    fn should_reject_order_id_of_unknown_trading_pair() {
        let order_id = OrderId::new(OrderBookId::new(2), OrderSeq::new(1)).to_string();
        let description = unsupported_description(consent_info(
            "cancel_limit_order",
            Encode!(&order_id).unwrap(),
            None,
        ));
        assert_eq!(description, "Unknown trading pair for this order ID.");
    }
}

mod trading_accounts {
    use super::*;

    fn trading_account() -> Principal {
        Principal::from_text(TRADING_ACCOUNT).unwrap()
    }

    #[test]
    fn should_render_add_trading_account() {
        let arg = Encode!(&trading_account()).unwrap();
        assert_eq!(
            generic_message("add_trading_account", arg.clone()),
            "# Add trading account on OISY TRADE\n\n\
             Allow the principal below to place and cancel limit orders on your behalf, \
             using your OISY TRADE balances, until you remove it. It cannot deposit or \
             withdraw funds, but the orders it places can trade away your balances. Only \
             authorize a principal you trust.\n\n\
             **Trading account:**\n`2vxsx-fae`"
        );
        assert_eq!(
            fields_message("add_trading_account", arg),
            FieldsDisplay {
                intent: "Add trading account".to_string(),
                fields: fields(&[
                    ("Trading account", text(TRADING_ACCOUNT)),
                    (
                        "Permission",
                        text("Place and cancel orders with your balances until removed")
                    ),
                ]),
            }
        );
    }

    #[test]
    fn should_render_remove_trading_account() {
        let arg = Encode!(&trading_account()).unwrap();
        assert_eq!(
            generic_message("remove_trading_account", arg.clone()),
            "# Remove trading account on OISY TRADE\n\n\
             Revoke the principal below as a trading account: it can no longer place or \
             cancel orders on your behalf. Open orders it already placed stay open.\n\n\
             **Trading account:**\n`2vxsx-fae`"
        );
        assert_eq!(
            fields_message("remove_trading_account", arg),
            FieldsDisplay {
                intent: "Remove trading account".to_string(),
                fields: fields(&[
                    ("Trading account", text(TRADING_ACCOUNT)),
                    ("Open orders", text("Orders it placed stay open")),
                ]),
            }
        );
    }
}
