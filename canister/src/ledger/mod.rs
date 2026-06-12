use crate::Runtime;
use oisy_trade_types::{
    DepositError, DepositRequest, DepositResponse, LedgerTransferError, LedgerTransferFromError,
    WithdrawError, WithdrawResponse,
};

pub(crate) struct WithdrawOutcome {
    pub result: Result<WithdrawResponse, WithdrawError>,
    /// Set when a `BadFee` response revealed a fee that differs from the
    /// cached value. The caller should persist this so that future withdrawals
    /// skip the BadFee round-trip. `None` when the cached fee was already
    /// correct or the ledger was unreachable.
    pub ledger_fee: Option<candid::Nat>,
}

/// Result of a single `icrc1_transfer` call, keeping `BadFee` separate so
/// the retry logic can act on it.
enum Icrc1TransferError {
    BadFee { expected_fee: candid::Nat },
    Other(WithdrawError),
}

pub async fn deposit(
    request: DepositRequest,
    runtime: &impl Runtime,
) -> Result<DepositResponse, DepositError> {
    use icrc_ledger_types::icrc1::account::Account;
    use icrc_ledger_types::icrc2::transfer_from::{TransferFromArgs, TransferFromError};

    // TODO(DEFI-2741): Consider adding a check for supported tokens to disallow users to deposit
    //  funds that are not supported by the OISY TRADE.
    let token = request.token_id;
    let amount = request.amount;
    let caller = runtime.msg_caller();

    let transfer_args = TransferFromArgs {
        spender_subaccount: None,
        from: Account {
            owner: caller,
            subaccount: None,
        },
        to: Account {
            owner: runtime.canister_self(),
            subaccount: None,
        },
        amount: amount.clone(),
        // TODO(DEFI-2741): Not strictly necessary to set a fee for deposits, since it is deducted
        //  from the from account, but for withdrawals we will need to know the fee to be able to
        //  deduct it from the amount, so for consistency we should consider setting it for deposits
        //  as well.
        fee: None,
        memo: None,
        created_at_time: None,
    };

    // TODO(DEFI-2745): Consider switching to bounded_wait calls.
    let response = runtime
        .call_unbounded_wait(token.ledger_id, "icrc2_transfer_from", (transfer_args,))
        .await
        .map_err(|e| DepositError::CallFailed {
            ledger: token.ledger_id,
            method: "icrc2_transfer_from".to_string(),
            reason: format!("{e}"),
        })?;

    let (result,): (Result<candid::Nat, TransferFromError>,) =
        response
            .candid_tuple()
            .map_err(|e| DepositError::CallFailed {
                ledger: token.ledger_id,
                method: "icrc2_transfer_from".to_string(),
                reason: e.to_string(),
            })?;

    let block_index = result.map_err(to_ledger_error)?;

    Ok(DepositResponse { block_index })
}

/// Transfer tokens from the OISY TRADE canister to `to` via `icrc1_transfer`.
///
/// Uses `cached_fee` for the first attempt. If the ledger rejects it with
/// `BadFee`, the correct fee is used for a single retry. The amount shall
/// be larger than zero (checked by caller).
pub(crate) async fn withdraw(
    token: &oisy_trade_types::TokenId,
    to: candid::Principal,
    amount: candid::Nat,
    cached_fee: candid::Nat,
    runtime: &impl Runtime,
) -> WithdrawOutcome {
    debug_assert_ne!(amount, 0u64, "withdrawal amount must be greater than zero");
    // When the cached fee exceeds the amount (e.g. the ledger fee was lowered
    // since the last withdrawal), cap it to amount - 1 so the subtraction
    // doesn't underflow. The ledger will reply with BadFee and the retry will
    // use the correct fee.
    let capped_fee = std::cmp::min(cached_fee, amount.clone() - 1u64);
    let transfer_amount = amount.clone() - capped_fee.clone();
    match icrc1_transfer(token, to, transfer_amount, capped_fee, runtime).await {
        Ok(response) => WithdrawOutcome {
            result: Ok(response),
            ledger_fee: None,
        },
        Err(Icrc1TransferError::BadFee { expected_fee }) => {
            if amount <= expected_fee {
                return WithdrawOutcome {
                    result: Err(WithdrawError::AmountTooSmall {
                        min_amount: expected_fee.clone() + 1u64,
                    }),
                    ledger_fee: Some(expected_fee),
                };
            }
            let retry_transfer_amount = amount - expected_fee.clone();
            match icrc1_transfer(
                token,
                to,
                retry_transfer_amount,
                expected_fee.clone(),
                runtime,
            )
            .await
            {
                Ok(response) => WithdrawOutcome {
                    result: Ok(response),
                    ledger_fee: Some(expected_fee),
                },
                Err(Icrc1TransferError::BadFee {
                    expected_fee: latest_fee,
                }) => WithdrawOutcome {
                    result: Err(WithdrawError::LedgerError(
                        LedgerTransferError::InternalError(
                            "ledger fee changed between retries".to_string(),
                        ),
                    )),
                    ledger_fee: Some(latest_fee),
                },
                Err(Icrc1TransferError::Other(e)) => WithdrawOutcome {
                    result: Err(e),
                    ledger_fee: Some(expected_fee),
                },
            }
        }
        Err(Icrc1TransferError::Other(e)) => WithdrawOutcome {
            result: Err(e),
            ledger_fee: None,
        },
    }
}

async fn icrc1_transfer(
    token: &oisy_trade_types::TokenId,
    to: candid::Principal,
    transfer_amount: candid::Nat,
    fee: candid::Nat,
    runtime: &impl Runtime,
) -> Result<WithdrawResponse, Icrc1TransferError> {
    use icrc_ledger_types::icrc1::account::Account;
    use icrc_ledger_types::icrc1::transfer::TransferArg;

    let transfer_args = TransferArg {
        from_subaccount: None,
        to: Account {
            owner: to,
            subaccount: None,
        },
        amount: transfer_amount,
        fee: Some(fee),
        memo: None,
        created_at_time: None,
    };

    let response = runtime
        .call_unbounded_wait(token.ledger_id, "icrc1_transfer", (transfer_args,))
        .await
        .map_err(|e| {
            Icrc1TransferError::Other(WithdrawError::CallFailed {
                ledger: token.ledger_id,
                method: "icrc1_transfer".to_string(),
                reason: format!("{e}"),
            })
        })?;

    let (result,): (Result<candid::Nat, icrc_ledger_types::icrc1::transfer::TransferError>,) =
        response.candid_tuple().map_err(|e| {
            Icrc1TransferError::Other(WithdrawError::CallFailed {
                ledger: token.ledger_id,
                method: "icrc1_transfer".to_string(),
                reason: e.to_string(),
            })
        })?;

    match result {
        Ok(block_index) => Ok(WithdrawResponse { block_index }),
        Err(icrc_ledger_types::icrc1::transfer::TransferError::BadFee { expected_fee }) => {
            Err(Icrc1TransferError::BadFee { expected_fee })
        }
        Err(e) => Err(Icrc1TransferError::Other(to_ledger_transfer_error(e))),
    }
}

fn to_ledger_transfer_error(e: icrc_ledger_types::icrc1::transfer::TransferError) -> WithdrawError {
    use icrc_ledger_types::icrc1::transfer::TransferError;
    match e {
        TransferError::InsufficientFunds { balance } => {
            WithdrawError::LedgerError(LedgerTransferError::InsufficientFunds { balance })
        }
        TransferError::TemporarilyUnavailable => {
            WithdrawError::LedgerError(LedgerTransferError::TemporarilyUnavailable)
        }
        TransferError::BadFee { .. } => {
            unreachable!("BUG: BadFee is handled by the caller before invoking this mapper")
        }
        TransferError::BadBurn { .. }
        | TransferError::CreatedInFuture { .. }
        | TransferError::Duplicate { .. }
        | TransferError::GenericError { .. }
        | TransferError::TooOld => {
            WithdrawError::LedgerError(LedgerTransferError::InternalError(format!("{e}")))
        }
    }
}

fn to_ledger_error(e: icrc_ledger_types::icrc2::transfer_from::TransferFromError) -> DepositError {
    use icrc_ledger_types::icrc2::transfer_from::TransferFromError;
    match e {
        TransferFromError::InsufficientFunds { balance } => {
            DepositError::LedgerError(LedgerTransferFromError::InsufficientFunds { balance })
        }
        TransferFromError::InsufficientAllowance { allowance } => {
            DepositError::LedgerError(LedgerTransferFromError::InsufficientAllowance { allowance })
        }
        TransferFromError::TemporarilyUnavailable => {
            DepositError::LedgerError(LedgerTransferFromError::TemporarilyUnavailable)
        }
        // These should never happen, but rather than trapping we return an internal error here.
        TransferFromError::BadFee { .. } => {
            DepositError::LedgerError(LedgerTransferFromError::InternalError(format!("{e}")))
        }
        TransferFromError::BadBurn { .. } => {
            DepositError::LedgerError(LedgerTransferFromError::InternalError(format!("{e}")))
        }
        TransferFromError::CreatedInFuture { .. } => {
            DepositError::LedgerError(LedgerTransferFromError::InternalError(format!("{e}")))
        }
        TransferFromError::Duplicate { .. } => {
            DepositError::LedgerError(LedgerTransferFromError::InternalError(format!("{e}")))
        }
        TransferFromError::GenericError { .. } => {
            DepositError::LedgerError(LedgerTransferFromError::InternalError(format!("{e}")))
        }
        TransferFromError::TooOld => {
            DepositError::LedgerError(LedgerTransferFromError::InternalError(format!("{e}")))
        }
    }
}
