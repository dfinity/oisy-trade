use crate::order::{Price, Quantity, Side};
use dex_types::{
    DepositError, DepositRequest, DepositResponse, LedgerTransferFromError, LimitOrderRequest,
    LimitOrderResponse, OrderStatus, Token,
};

pub mod order;
pub mod state;

#[cfg(test)]
mod test_fixtures;
#[cfg(test)]
mod tests;

pub fn add_limit_order(_request: LimitOrderRequest) -> LimitOrderResponse {
    let order_id = state::with_state_mut(|s| {
        // TODO DEFI-2723: use value from request
        s.add_limit_order(order::PendingOrder {
            side: Side::Buy,
            price: Price::ZERO,
            quantity: Quantity::ZERO,
        })
    });
    LimitOrderResponse {
        order_id: u64::from(order_id),
    }
}

pub fn get_order_status(order_id: dex_types::OrderId) -> OrderStatus {
    state::with_state(|s| s.get_order_status(order::OrderId::from(order_id)))
}

pub async fn deposit(request: DepositRequest) -> Result<DepositResponse, DepositError> {
    use ic_cdk::call::Call;
    use icrc_ledger_types::icrc1::account::Account;
    use icrc_ledger_types::icrc2::transfer_from::{TransferFromArgs, TransferFromError};

    let token = request.token;
    let amount = request.amount;
    let caller = ic_cdk::api::msg_caller();

    let transfer_args = TransferFromArgs {
        spender_subaccount: None,
        from: Account {
            owner: caller,
            subaccount: None,
        },
        to: Account {
            owner: ic_cdk::api::canister_self(),
            subaccount: None,
        },
        amount: amount.clone(),
        fee: None,
        memo: None,
        created_at_time: Some(ic_cdk::api::time()),
    };

    let response = Call::unbounded_wait(token.ledger_canister_id, "icrc2_transfer_from")
        .with_args(&(transfer_args,))
        .await
        .map_err(|e| DepositError::CallFailed {
            ledger: token.ledger_canister_id,
            method: "icrc2_transfer_from".to_string(),
            reason: format!("{e}"),
        })?;

    let (result,): (Result<candid::Nat, TransferFromError>,) =
        response.candid_tuple().map_err(|e| DepositError::CallFailed {
            ledger: token.ledger_canister_id,
            method: "icrc2_transfer_from".to_string(),
            reason: e.to_string(),
        })?;

    let block_index = result.map_err(|e| DepositError::LedgerError(to_ledger_error(e)))?;

    state::with_state_mut(|s| s.deposit(caller, token.ledger_canister_id, amount));

    Ok(DepositResponse { block_index })
}

fn to_ledger_error(
    e: icrc_ledger_types::icrc2::transfer_from::TransferFromError,
) -> LedgerTransferFromError {
    use icrc_ledger_types::icrc2::transfer_from::TransferFromError;
    match e {
        TransferFromError::BadFee { expected_fee } => {
            LedgerTransferFromError::BadFee { expected_fee }
        }
        TransferFromError::BadBurn { min_burn_amount } => {
            LedgerTransferFromError::BadBurn { min_burn_amount }
        }
        TransferFromError::InsufficientFunds { balance } => {
            LedgerTransferFromError::InsufficientFunds { balance }
        }
        TransferFromError::InsufficientAllowance { allowance } => {
            LedgerTransferFromError::InsufficientAllowance { allowance }
        }
        TransferFromError::TooOld => LedgerTransferFromError::TooOld,
        TransferFromError::CreatedInFuture { ledger_time } => {
            LedgerTransferFromError::CreatedInFuture { ledger_time }
        }
        TransferFromError::Duplicate { duplicate_of } => {
            LedgerTransferFromError::Duplicate { duplicate_of }
        }
        TransferFromError::TemporarilyUnavailable => {
            LedgerTransferFromError::TemporarilyUnavailable
        }
        TransferFromError::GenericError {
            error_code,
            message,
        } => LedgerTransferFromError::GenericError {
            error_code,
            message,
        },
    }
}

pub fn get_balance(token: Token) -> candid::Nat {
    let caller = ic_cdk::api::msg_caller();
    state::with_state(|s| s.get_balance(caller, token.ledger_canister_id))
}
