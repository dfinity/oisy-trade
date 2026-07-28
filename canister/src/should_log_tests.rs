use crate::{should_log_deposit_error, should_log_withdraw_error};
use oisy_trade_types::{
    DepositError, DepositInternalError, DepositRequestError, DepositTemporaryError, WithdrawError,
    WithdrawInternalError, WithdrawRequestError, WithdrawTemporaryError,
};

#[test]
fn should_not_log_deposit_request_errors() {
    struct TestCase {
        desc: &'static str,
        error: DepositError,
        expected: bool,
    }

    let cases = vec![
        TestCase {
            desc: "out-of-range amount is a request error and must not be rendered",
            error: DepositError::request(DepositRequestError::AmountExceedsMaximum),
            expected: false,
        },
        TestCase {
            desc: "in-flight guard rejection is a user action",
            error: DepositError::temporary(DepositTemporaryError::OperationInProgress),
            expected: false,
        },
        TestCase {
            desc: "ledger unavailability is a temporary operational error",
            error: DepositError::temporary(DepositTemporaryError::LedgerTemporarilyUnavailable),
            expected: true,
        },
        TestCase {
            desc: "internal errors are always logged",
            error: DepositError::internal(DepositInternalError::LedgerError {
                reason: "boom".to_string(),
            }),
            expected: true,
        },
    ];

    for case in cases {
        assert_eq!(
            should_log_deposit_error(&case.error),
            case.expected,
            "{}",
            case.desc
        );
    }
}

#[test]
fn should_not_log_withdraw_request_errors() {
    struct TestCase {
        desc: &'static str,
        error: WithdrawError,
        expected: bool,
    }

    let cases = vec![
        TestCase {
            desc: "out-of-range amount is a request error and must not be rendered",
            error: WithdrawError::request(WithdrawRequestError::AmountExceedsMaximum),
            expected: false,
        },
        TestCase {
            desc: "in-flight guard rejection is a user action",
            error: WithdrawError::temporary(WithdrawTemporaryError::OperationInProgress),
            expected: false,
        },
        TestCase {
            desc: "ledger unavailability is a temporary operational error",
            error: WithdrawError::temporary(WithdrawTemporaryError::LedgerTemporarilyUnavailable),
            expected: true,
        },
        TestCase {
            desc: "internal errors are always logged",
            error: WithdrawError::internal(WithdrawInternalError::LedgerError {
                reason: "boom".to_string(),
            }),
            expected: true,
        },
    ];

    for case in cases {
        assert_eq!(
            should_log_withdraw_error(&case.error),
            case.expected,
            "{}",
            case.desc
        );
    }
}
