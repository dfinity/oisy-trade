//! Implementation of the [ICRC-21](https://github.com/dfinity/ICRC/blob/main/ICRCs/ICRC-21/ICRC-21.md)
//! Canister Call Consent Message standard for OISY TRADE.
//!
//! Consent messages are produced for the user-facing update endpoints a wallet
//! signs on behalf of its user: `deposit`, `withdraw`, `add_limit_order`,
//! `cancel_limit_order`, `add_trading_account` and `remove_trading_account`.
//! Token symbols and decimals are resolved from the canister state, and calls
//! referring to an unknown trading pair or token are rejected.

#[cfg(test)]
mod tests;

use crate::order::{self, TokenMetadata};
use crate::state::{self, State};
use candid::{CandidType, Decode, Nat, Principal};
use ic_stable_structures::Memory;
use icrc_ledger_types::icrc21::errors::{ErrorInfo, Icrc21Error};
use icrc_ledger_types::icrc21::lib::MAX_CONSENT_MESSAGE_ARG_SIZE_BYTES;
use icrc_ledger_types::icrc21::requests::{
    ConsentMessageMetadata, ConsentMessageRequest, DisplayMessageType,
};
use icrc_ledger_types::icrc21::responses::{ConsentInfo, ConsentMessage, FieldsDisplay, Value};
use oisy_trade_types::{DepositRequest, LimitOrderRequest, OrderId, WithdrawRequest};

/// An entry of the ICRC-10 supported standards list.
#[derive(Clone, Debug, CandidType, Eq, PartialEq)]
pub struct StandardRecord {
    pub name: String,
    pub url: String,
}

pub fn icrc10_supported_standards() -> Vec<StandardRecord> {
    vec![
        StandardRecord {
            name: "ICRC-10".to_string(),
            url: "https://github.com/dfinity/ICRC/blob/main/ICRCs/ICRC-10/ICRC-10.md".to_string(),
        },
        StandardRecord {
            name: "ICRC-21".to_string(),
            url: "https://github.com/dfinity/ICRC/blob/main/ICRCs/ICRC-21/ICRC-21.md".to_string(),
        },
    ]
}

pub fn icrc21_canister_call_consent_message(
    consent_msg_request: ConsentMessageRequest,
) -> Result<ConsentInfo, Icrc21Error> {
    state::with_state(|s| build_consent_info(consent_msg_request, s))
}

fn build_consent_info<MH: Memory, MB: Memory>(
    consent_msg_request: ConsentMessageRequest,
    state: &State<MH, MB>,
) -> Result<ConsentInfo, Icrc21Error> {
    if consent_msg_request.arg.len() > MAX_CONSENT_MESSAGE_ARG_SIZE_BYTES as usize {
        return Err(unsupported(format!(
            "The argument size is too large. The maximum allowed size is \
             {MAX_CONSENT_MESSAGE_ARG_SIZE_BYTES} bytes."
        )));
    }

    let display_type = consent_msg_request
        .user_preferences
        .device_spec
        .clone()
        .unwrap_or(DisplayMessageType::GenericDisplay);
    let arg = consent_msg_request.arg.as_slice();

    let consent_message = match consent_msg_request.method.as_str() {
        "deposit" => {
            let args = Decode!(arg, DepositRequest)
                .map_err(|e| unsupported(format!("Failed to decode DepositRequest: {e}")))?;
            let token = known_token(state, &args.token_id.ledger_id)?;
            let amount = validate_amount(&args.amount)?;
            build_deposit_message(&amount, token, &display_type)
        }
        "withdraw" => {
            let args = Decode!(arg, WithdrawRequest)
                .map_err(|e| unsupported(format!("Failed to decode WithdrawRequest: {e}")))?;
            let token = known_token(state, &args.token_id.ledger_id)?;
            let amount = validate_amount(&args.amount)?;
            build_withdraw_message(&amount, token, &display_type)
        }
        "add_limit_order" => {
            let args = Decode!(arg, LimitOrderRequest)
                .map_err(|e| unsupported(format!("Failed to decode LimitOrderRequest: {e}")))?;
            let order = LimitOrder::resolve(args, state)?;
            build_add_limit_order_message(&order, &display_type)
        }
        "cancel_limit_order" => {
            let order_id = Decode!(arg, OrderId)
                .map_err(|e| unsupported(format!("Failed to decode OrderId: {e}")))?;
            // Parse with the same parser as `cancel_limit_order`: this rules out
            // Markdown injection through the free-form `text` argument, and the
            // canonical re-encoding is what gets displayed.
            let order_id = order_id
                .parse::<order::OrderId>()
                .map_err(|_| unsupported("Invalid order ID.".to_string()))?;
            let pair = state
                .trading_pairs()
                .get_pair(&order_id.book_id())
                .ok_or_else(|| {
                    unsupported("Unknown trading pair for this order ID.".to_string())
                })?;
            let market = Market::new(state, pair);
            build_cancel_limit_order_message(&order_id, &market, &display_type)
        }
        "add_trading_account" => {
            let trading = Decode!(arg, Principal)
                .map_err(|e| unsupported(format!("Failed to decode principal: {e}")))?;
            build_add_trading_account_message(&trading, &display_type)
        }
        "remove_trading_account" => {
            let trading = Decode!(arg, Principal)
                .map_err(|e| unsupported(format!("Failed to decode principal: {e}")))?;
            build_remove_trading_account_message(&trading, &display_type)
        }
        method => {
            return Err(unsupported(format!(
                "The method '{method}' is not supported by the OISY TRADE ICRC-21 endpoint."
            )));
        }
    };

    // Respond in English regardless of what the client requested for now.
    let metadata = ConsentMessageMetadata {
        language: "en".to_string(),
        utc_offset_minutes: consent_msg_request
            .user_preferences
            .metadata
            .utc_offset_minutes,
    };

    Ok(ConsentInfo {
        metadata,
        consent_message,
    })
}

fn unsupported(description: String) -> Icrc21Error {
    Icrc21Error::UnsupportedCanisterCall(ErrorInfo { description })
}

/// Looks up the metadata of a token registered with the canister, so that
/// amounts are displayed with the right symbol and decimals. Unknown tokens are
/// rejected: `deposit` and `withdraw` would fail with `UnsupportedToken`.
fn known_token<'a, MH: Memory, MB: Memory>(
    state: &'a State<MH, MB>,
    ledger_id: &Principal,
) -> Result<&'a TokenMetadata, Icrc21Error> {
    state
        .token_metadata(&order::TokenId::new(*ledger_id))
        .ok_or_else(|| unsupported(format!("Unsupported token: {ledger_id}.")))
}

/// Rejects amounts the canister would reject with `AmountExceedsMaximum`.
fn validate_amount(amount: &Nat) -> Result<Nat, Icrc21Error> {
    order::Quantity::try_from(amount)
        .map(Nat::from)
        .map_err(|_| unsupported("The amount exceeds the maximum supported value.".to_string()))
}

/// A trading pair resolved to the metadata of its base and quote tokens.
struct Market<'a> {
    base: &'a TokenMetadata,
    quote: &'a TokenMetadata,
}

impl<'a> Market<'a> {
    fn new<MH: Memory, MB: Memory>(state: &'a State<MH, MB>, pair: &order::TradingPair) -> Self {
        Self {
            base: state
                .token_metadata(&pair.base)
                .expect("BUG: trading pair registered but base token metadata missing"),
            quote: state
                .token_metadata(&pair.quote)
                .expect("BUG: trading pair registered but quote token metadata missing"),
        }
    }

    fn name(&self) -> String {
        format!("{}/{}", self.base.symbol, self.quote.symbol)
    }
}

/// A `LimitOrderRequest` validated against the order book it targets.
struct LimitOrder<'a> {
    market: Market<'a>,
    side: order::Side,
    price: Nat,
    quantity: Nat,
    time_in_force: order::TimeInForce,
    /// Amount reserved from the caller's free balance: the quote notional
    /// `price × quantity / 10^base_decimals` for a buy, `quantity` for a sell.
    reserved: Nat,
    fee_rates: order::FeeRates,
}

impl<'a> LimitOrder<'a> {
    /// Resolves the trading pair and applies the same validation as
    /// `add_limit_order` that is needed to display the order accurately: the
    /// pair must be registered, the price and quantity must be positive
    /// multiples of the tick and lot sizes, and the reserved amount must not
    /// overflow.
    fn resolve<MH: Memory, MB: Memory>(
        request: LimitOrderRequest,
        state: &'a State<MH, MB>,
    ) -> Result<Self, Icrc21Error> {
        let pair = order::TradingPair::from(request.pair);
        let book = state
            .trading_pairs()
            .get_book_id(&pair)
            .and_then(|book_id| state.order_book(book_id))
            .ok_or_else(|| {
                unsupported(format!(
                    "Unknown trading pair: {}/{}.",
                    pair.base.as_principal(),
                    pair.quote.as_principal()
                ))
            })?;
        let pending = order::PendingOrder::try_from(request).map_err(|_| {
            unsupported("The price or quantity exceeds the maximum supported value.".to_string())
        })?;
        book.validate_order(pending.price, &pending.quantity)
            .map_err(|e| {
                unsupported(match e {
                    order::MatchOrderError::InvalidTickSize { tick_size, .. } => format!(
                        "The price is not a positive multiple of the tick size {}.",
                        Nat::from(tick_size)
                    ),
                    order::MatchOrderError::InvalidLotSize { lot_size, .. } => format!(
                        "The quantity is not a positive multiple of the lot size {}.",
                        Nat::from(lot_size)
                    ),
                })
            })?;
        let notional = pending
            .price
            .checked_mul_quantity_scaled(&pending.quantity, state.base_scale(&pair.base))
            .ok_or_else(|| {
                unsupported("The order amount exceeds the maximum supported value.".to_string())
            })?;
        let reserved = match pending.side {
            order::Side::Buy => notional,
            order::Side::Sell => pending.quantity,
        };
        Ok(Self {
            market: Market::new(state, &pair),
            side: pending.side,
            price: Nat::from(pending.price),
            quantity: Nat::from(pending.quantity),
            time_in_force: pending.time_in_force,
            reserved: Nat::from(reserved),
            fee_rates: book.fee_rates(),
        })
    }

    /// The token reserved by the order, which is also the token it pays with.
    fn spent_token(&self) -> &TokenMetadata {
        match self.side {
            order::Side::Buy => self.market.quote,
            order::Side::Sell => self.market.base,
        }
    }

    /// The token the order receives when it fills, in which fees are charged.
    fn received_token(&self) -> &TokenMetadata {
        match self.side {
            order::Side::Buy => self.market.base,
            order::Side::Sell => self.market.quote,
        }
    }

    fn side_label(&self) -> &'static str {
        match self.side {
            order::Side::Buy => "Buy",
            order::Side::Sell => "Sell",
        }
    }

    fn price_label(&self) -> String {
        format!(
            "{} per {}",
            format_token_amount(&self.price, self.market.quote),
            self.market.base.symbol
        )
    }

    fn time_in_force_label(&self) -> &'static str {
        match self.time_in_force {
            order::TimeInForce::GoodTilCanceled => "Good 'til canceled",
            order::TimeInForce::FillOrKill => "Fill or kill",
        }
    }

    /// A fill-or-kill order never rests in the book, so it is always the taker.
    fn fee_label(&self) -> String {
        let taker = format_bps(self.fee_rates.taker.get());
        match self.time_in_force {
            order::TimeInForce::GoodTilCanceled => format!(
                "{} maker / {taker} taker",
                format_bps(self.fee_rates.maker.get())
            ),
            order::TimeInForce::FillOrKill => format!("{taker} taker"),
        }
    }
}

fn build_deposit_message(
    amount: &Nat,
    token: &TokenMetadata,
    display_type: &DisplayMessageType,
) -> ConsentMessage {
    let symbol = &token.symbol;
    match display_type {
        DisplayMessageType::GenericDisplay => ConsentMessage::GenericDisplayMessage(format!(
            "# Deposit {symbol} to OISY TRADE\n\n\
             Authorize OISY TRADE to transfer {symbol} from your wallet, using the allowance \
             you approved for OISY TRADE, and credit it to your OISY TRADE balance.\n\n\
             **Amount:** `{amount}`\n\n\
             **Fees:** The {symbol} ledger transfer fee is charged to your wallet on top of \
             the amount.",
            amount = format_token_amount(amount, token),
        )),
        DisplayMessageType::FieldsDisplay => ConsentMessage::FieldsDisplayMessage(FieldsDisplay {
            intent: format!("Deposit {symbol}"),
            fields: vec![
                ("Amount".to_string(), token_amount_value(amount, token)),
                (
                    "Fees".to_string(),
                    Value::Text {
                        content: "Ledger fee, charged on top of the amount".to_string(),
                    },
                ),
            ],
        }),
    }
}

fn build_withdraw_message(
    amount: &Nat,
    token: &TokenMetadata,
    display_type: &DisplayMessageType,
) -> ConsentMessage {
    let symbol = &token.symbol;
    match display_type {
        DisplayMessageType::GenericDisplay => ConsentMessage::GenericDisplayMessage(format!(
            "# Withdraw {symbol} from OISY TRADE\n\n\
             Withdraw {symbol} from your free OISY TRADE balance to your wallet.\n\n\
             **Amount:** `{amount}`\n\n\
             **Fees:** The {symbol} ledger transfer fee is deducted from the amount, so you \
             receive the amount minus the fee.",
            amount = format_token_amount(amount, token),
        )),
        DisplayMessageType::FieldsDisplay => ConsentMessage::FieldsDisplayMessage(FieldsDisplay {
            intent: format!("Withdraw {symbol}"),
            fields: vec![
                ("Amount".to_string(), token_amount_value(amount, token)),
                (
                    "Fees".to_string(),
                    Value::Text {
                        content: "Ledger fee, deducted from the amount".to_string(),
                    },
                ),
            ],
        }),
    }
}

fn build_add_limit_order_message(
    order: &LimitOrder,
    display_type: &DisplayMessageType,
) -> ConsentMessage {
    let side = order.side_label();
    let base = &order.market.base.symbol;
    let quote = &order.market.quote.symbol;
    let market = order.market.name();
    match display_type {
        DisplayMessageType::GenericDisplay => {
            let (action, preposition) = match order.side {
                order::Side::Buy => ("buy", "with"),
                order::Side::Sell => ("sell", "for"),
            };
            let time_in_force_details = match order.time_in_force {
                order::TimeInForce::GoodTilCanceled => {
                    "The order stays in the order book until it is filled or canceled."
                }
                order::TimeInForce::FillOrKill => {
                    "When the order is matched, it must be filled in full, otherwise it \
                     expires without any trade."
                }
            };
            ConsentMessage::GenericDisplayMessage(format!(
                "# {side} {base} on OISY TRADE\n\n\
                 Place a limit order to {action} {base} {preposition} {quote} on OISY TRADE, \
                 at the limit price or better. The order uses your OISY TRADE balance.\n\n\
                 **Market:** `{market}`\n\n\
                 **Side:** `{side}`\n\n\
                 **Quantity:** `{quantity}`\n\n\
                 **Limit price:** `{price}`\n\n\
                 **Reserved:** `{reserved}`\n\
                 Locked from your free OISY TRADE balance while the order is open; any amount \
                 not spent is released back to it.\n\n\
                 **Time in force:** `{time_in_force}`\n\
                 {time_in_force_details}\n\n\
                 **Trading fee:** `{fee}`\n\
                 Deducted from the {received} you receive, rounded up on each fill.",
                quantity = format_token_amount(&order.quantity, order.market.base),
                price = order.price_label(),
                reserved = format_token_amount(&order.reserved, order.spent_token()),
                time_in_force = order.time_in_force_label(),
                fee = order.fee_label(),
                received = order.received_token().symbol,
            ))
        }
        DisplayMessageType::FieldsDisplay => ConsentMessage::FieldsDisplayMessage(FieldsDisplay {
            intent: format!("{side} {base}"),
            fields: vec![
                ("Market".to_string(), Value::Text { content: market }),
                (
                    "Side".to_string(),
                    Value::Text {
                        content: side.to_string(),
                    },
                ),
                (
                    "Quantity".to_string(),
                    token_amount_value(&order.quantity, order.market.base),
                ),
                (
                    "Limit price".to_string(),
                    Value::Text {
                        content: order.price_label(),
                    },
                ),
                (
                    "Reserved".to_string(),
                    token_amount_value(&order.reserved, order.spent_token()),
                ),
                (
                    "Time in force".to_string(),
                    Value::Text {
                        content: order.time_in_force_label().to_string(),
                    },
                ),
                (
                    "Trading fee".to_string(),
                    Value::Text {
                        content: order.fee_label(),
                    },
                ),
            ],
        }),
    }
}

fn build_cancel_limit_order_message(
    order_id: &order::OrderId,
    market: &Market,
    display_type: &DisplayMessageType,
) -> ConsentMessage {
    let market = market.name();
    match display_type {
        DisplayMessageType::GenericDisplay => ConsentMessage::GenericDisplayMessage(format!(
            "# Cancel order on OISY TRADE\n\n\
             Cancel your limit order on the OISY TRADE {market} market. The unfilled part of \
             the order is removed from the order book and its reserved funds are released to \
             your free OISY TRADE balance.\n\n\
             **Market:** `{market}`\n\n\
             **Order ID:**\n`{order_id}`"
        )),
        DisplayMessageType::FieldsDisplay => ConsentMessage::FieldsDisplayMessage(FieldsDisplay {
            intent: "Cancel order".to_string(),
            fields: vec![
                ("Market".to_string(), Value::Text { content: market }),
                (
                    "Order ID".to_string(),
                    Value::Text {
                        content: order_id.to_string(),
                    },
                ),
            ],
        }),
    }
}

fn build_add_trading_account_message(
    trading: &Principal,
    display_type: &DisplayMessageType,
) -> ConsentMessage {
    match display_type {
        DisplayMessageType::GenericDisplay => ConsentMessage::GenericDisplayMessage(format!(
            "# Add trading account on OISY TRADE\n\n\
             Allow the principal below to place and cancel limit orders on your behalf, \
             using your OISY TRADE balances, until you remove it. It cannot deposit or \
             withdraw funds, but the orders it places can trade away your balances. Only \
             authorize a principal you trust.\n\n\
             **Trading account:**\n`{trading}`"
        )),
        DisplayMessageType::FieldsDisplay => ConsentMessage::FieldsDisplayMessage(FieldsDisplay {
            intent: "Add trading account".to_string(),
            fields: vec![
                (
                    "Trading account".to_string(),
                    Value::Text {
                        content: trading.to_string(),
                    },
                ),
                (
                    "Permission".to_string(),
                    Value::Text {
                        content: "Place and cancel orders with your balances until removed"
                            .to_string(),
                    },
                ),
            ],
        }),
    }
}

fn build_remove_trading_account_message(
    trading: &Principal,
    display_type: &DisplayMessageType,
) -> ConsentMessage {
    match display_type {
        DisplayMessageType::GenericDisplay => ConsentMessage::GenericDisplayMessage(format!(
            "# Remove trading account on OISY TRADE\n\n\
             Revoke the principal below as a trading account: it can no longer place or \
             cancel orders on your behalf. Open orders it already placed stay open.\n\n\
             **Trading account:**\n`{trading}`"
        )),
        DisplayMessageType::FieldsDisplay => ConsentMessage::FieldsDisplayMessage(FieldsDisplay {
            intent: "Remove trading account".to_string(),
            fields: vec![
                (
                    "Trading account".to_string(),
                    Value::Text {
                        content: trading.to_string(),
                    },
                ),
                (
                    "Open orders".to_string(),
                    Value::Text {
                        content: "Orders it placed stay open".to_string(),
                    },
                ),
            ],
        }),
    }
}

/// A `Value::TokenAmount` when the amount fits the `nat64` of the ICRC-21
/// `TokenAmount` value, otherwise the formatted amount as `Value::Text`.
/// Amounts are `nat` in OISY TRADE, and tokens with 18 decimals exceed `u64`
/// from about 18.4 whole tokens on.
fn token_amount_value(amount: &Nat, token: &TokenMetadata) -> Value {
    match u64::try_from(&amount.0) {
        Ok(amount) => Value::TokenAmount {
            decimals: token.decimals,
            amount,
            symbol: token.symbol.clone(),
        },
        Err(_) => Value::Text {
            content: format_token_amount(amount, token),
        },
    }
}

fn format_token_amount(amount: &Nat, token: &TokenMetadata) -> String {
    format!("{} {}", format_amount(amount, token.decimals), token.symbol)
}

/// Formats `amount` smallest units as a decimal number of whole units, without
/// trailing zeros, e.g. `150_000_000` with 8 decimals is `1.5`.
fn format_amount(amount: &Nat, decimals: u8) -> String {
    let decimals = decimals as usize;
    let digits = format!("{:0>width$}", amount.0, width = decimals + 1);
    let (whole, frac) = digits.split_at(digits.len() - decimals);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{frac}")
    }
}

/// Formats a rate in basis points as a percentage, e.g. `25` is `0.25%`.
fn format_bps(bps: u16) -> String {
    format!("{}%", format_amount(&Nat::from(bps), 2))
}
