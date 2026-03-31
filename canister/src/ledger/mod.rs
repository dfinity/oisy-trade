use dex_types::{DepositError, DepositRequest, DepositResponse, LedgerTransferFromError};

pub async fn deposit(request: DepositRequest) -> Result<DepositResponse, DepositError> {
    use ic_cdk::call::Call;
    use icrc_ledger_types::icrc1::account::Account;
    use icrc_ledger_types::icrc2::transfer_from::{TransferFromArgs, TransferFromError};

    // TODO(DEFI-2741): Consider adding a check for supported tokens to disallow users to deposit
    //  funds that are not supported by the DEX.
    let token = request.token_id;
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
        // TODO(DEFI-2741): Not strictly necessary to set a fee for deposits, since it is deducted
        //  from the from account, but for withdrawals we will need to know the fee to be able to
        //  deduct it from the amount, so for consistency we should consider setting it for deposits
        //  as well.
        fee: None,
        memo: None,
        created_at_time: None,
    };

    // TODO(DEFI-2745): Consider switching to bounded_wait calls.
    let response = Call::unbounded_wait(token.ledger_id, "icrc2_transfer_from")
        .with_args(&(transfer_args,))
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
        TransferFromError::GenericError {
            error_code,
            message,
        } => DepositError::LedgerError(LedgerTransferFromError::GenericError {
            error_code,
            message,
        }),
        // These should never happen, but rather than trapping we return an internal error here.
        TransferFromError::BadFee { .. } => DepositError::InternalError(format!("{e}")),
        TransferFromError::BadBurn { .. } => DepositError::InternalError(format!("{e}")),
        TransferFromError::CreatedInFuture { .. } => DepositError::InternalError(format!("{e}")),
        TransferFromError::Duplicate { .. } => DepositError::InternalError(format!("{e}")),
        TransferFromError::TooOld => DepositError::InternalError(format!("{e}")),
    }
}
