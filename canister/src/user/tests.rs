use super::UserId;
use crate::test_fixtures::{principal, user_registry};

#[test]
fn get_or_register_is_stable_and_dense() {
    let mut registry = user_registry();
    // First-seen principals get dense ids in insertion order.
    assert_eq!(registry.get_or_register(principal(1)), UserId::new(0));
    assert_eq!(registry.get_or_register(principal(2)), UserId::new(1));
    assert_eq!(registry.get_or_register(principal(3)), UserId::new(2));
    // Re-registering returns the same id.
    assert_eq!(registry.get_or_register(principal(1)), UserId::new(0));
    assert_eq!(registry.get_or_register(principal(2)), UserId::new(1));
}

#[test]
fn lookup_does_not_assign() {
    let mut registry = user_registry();
    assert_eq!(registry.lookup(principal(1)), None);
    let id = registry.get_or_register(principal(1));
    assert_eq!(registry.lookup(principal(1)), Some(id));
    // A never-registered principal still has no id, and looking it up didn't
    // assign one (the next registration is still 1, not 2).
    assert_eq!(registry.lookup(principal(9)), None);
    assert_eq!(registry.get_or_register(principal(2)), UserId::new(1));
}

mod storable {
    use crate::test_fixtures::arbitrary::arb_user_id;
    use crate::user::UserId;
    use ic_stable_structures::Storable;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn should_roundtrip_through_stable_bytes(id in arb_user_id()) {
            prop_assert_eq!(UserId::from_bytes(id.to_bytes()), id);
        }
    }
}

mod trading_accounts {
    use crate::Timestamp;
    use crate::test_fixtures::arbitrary::{arb_principal, arb_timestamp};
    use crate::test_fixtures::{principal, user_registry};
    use crate::user::{
        FundingAccount, GrantError, MAX_TRADING_ACCOUNTS_PER_USER, RevokeError,
        TRADING_ACCOUNT_GRANT_COOLDOWN, TradingAccount, TradingAccountList, TradingGrant,
        UserRegistry,
    };
    use ic_stable_structures::{Storable, VectorMemory};
    use proptest::collection::vec;
    use proptest::prelude::proptest;
    use proptest::prop_assert_eq;

    /// The funding account under test.
    fn funding() -> candid::Principal {
        principal(1)
    }

    /// A fresh, unregistered prospective trading account.
    fn trading() -> candid::Principal {
        principal(2)
    }

    fn register(registry: &mut UserRegistry<VectorMemory>, p: candid::Principal) {
        registry.get_or_register(p);
    }

    fn record(
        registry: &mut UserRegistry<VectorMemory>,
        funding: candid::Principal,
        trading: candid::Principal,
        now: Timestamp,
    ) {
        registry.record_trading_account(FundingAccount(funding), TradingAccount(trading), now);
    }

    fn revoke(
        registry: &mut UserRegistry<VectorMemory>,
        funding: candid::Principal,
        trading: candid::Principal,
    ) {
        registry.record_revoke(FundingAccount(funding), TradingAccount(trading));
    }

    type Setup = Box<dyn Fn(&mut UserRegistry<VectorMemory>)>;

    struct PreconditionCase {
        desc: &'static str,
        setup: Setup,
        funding: candid::Principal,
        trading: candid::Principal,
        expected: Result<(), GrantError>,
    }

    #[test]
    fn should_enforce_add_trading_account_preconditions() {
        let cases = vec![
            PreconditionCase {
                desc: "registered funding, fresh trading principal",
                setup: Box::new(|r| register(r, funding())),
                funding: funding(),
                trading: trading(),
                expected: Ok(()),
            },
            PreconditionCase {
                desc: "granter is not a registered user",
                setup: Box::new(|_| {}),
                funding: funding(),
                trading: trading(),
                expected: Err(GrantError::GranterNotRegistered),
            },
            PreconditionCase {
                desc: "granter whitelists itself",
                setup: Box::new(|r| register(r, funding())),
                funding: funding(),
                trading: funding(),
                expected: Err(GrantError::SelfGrant),
            },
            PreconditionCase {
                desc: "trading principal is already a trading account of someone else",
                setup: Box::new(|r| {
                    register(r, funding());
                    register(r, principal(3));
                    record(r, principal(3), trading(), Timestamp::new(1));
                }),
                funding: funding(),
                trading: trading(),
                expected: Err(GrantError::AlreadyTradingAccount),
            },
            PreconditionCase {
                desc: "trading principal is already a trading account of the granter",
                setup: Box::new(|r| {
                    register(r, funding());
                    record(r, funding(), trading(), Timestamp::new(1));
                }),
                funding: funding(),
                trading: trading(),
                expected: Err(GrantError::AlreadyTradingAccount),
            },
            PreconditionCase {
                desc: "trading principal is already a registered user",
                setup: Box::new(|r| {
                    register(r, funding());
                    register(r, trading());
                }),
                funding: funding(),
                trading: trading(),
                expected: Err(GrantError::AlreadyRegisteredUser),
            },
            PreconditionCase {
                // An unregistered delegate granter: the trading-account check
                // runs before the registration check, so it is reported with
                // the specific reason rather than `GranterNotRegistered`.
                desc: "granter is itself a trading account (and unregistered)",
                setup: Box::new(|r| {
                    register(r, principal(3));
                    record(r, principal(3), funding(), Timestamp::new(1));
                }),
                funding: funding(),
                trading: trading(),
                expected: Err(GrantError::GranterIsTradingAccount),
            },
            PreconditionCase {
                desc: "granter is already at the trading-account cap",
                setup: Box::new(|r| {
                    register(r, funding());
                    for i in 0..MAX_TRADING_ACCOUNTS_PER_USER as u8 {
                        record(r, funding(), principal(10 + i), Timestamp::new(1));
                    }
                }),
                funding: funding(),
                trading: trading(),
                expected: Err(GrantError::TooManyTradingAccounts),
            },
        ];

        // A timestamp far past any prior grant so the R14 cooldown never fires;
        // these cases exercise the R7 identity/cap rules only.
        let now = Timestamp::new(u64::MAX);
        for case in cases {
            let mut registry = user_registry();
            (case.setup)(&mut registry);
            assert_eq!(
                registry.validate_trading_account(
                    FundingAccount(case.funding),
                    TradingAccount(case.trading),
                    now
                ),
                case.expected,
                "{}",
                case.desc
            );
        }
    }

    #[test]
    fn should_enforce_grant_cooldown() {
        let cooldown = TRADING_ACCOUNT_GRANT_COOLDOWN.as_nanos() as u64;
        let mut registry = user_registry();
        register(&mut registry, funding());

        record(
            &mut registry,
            funding(),
            principal(2),
            Timestamp::new(1_000),
        );

        // A second grant strictly within the cooldown is rejected as retryable,
        // and the check is independent of the specific new trading principal.
        assert_eq!(
            registry.validate_trading_account(
                FundingAccount(funding()),
                TradingAccount(principal(3)),
                Timestamp::new(1_000 + cooldown - 1)
            ),
            Err(GrantError::CooldownActive),
            "a grant within the cooldown is rejected"
        );

        // Exactly at the cooldown boundary the grant is allowed again.
        assert_eq!(
            registry.validate_trading_account(
                FundingAccount(funding()),
                TradingAccount(principal(3)),
                Timestamp::new(1_000 + cooldown)
            ),
            Ok(()),
            "a grant once the cooldown has elapsed is allowed"
        );
    }

    #[test]
    fn should_anchor_cooldown_even_after_revoking_the_last_key() {
        let cooldown = TRADING_ACCOUNT_GRANT_COOLDOWN.as_nanos() as u64;
        let mut registry = user_registry();
        register(&mut registry, funding());
        record(
            &mut registry,
            funding(),
            principal(2),
            Timestamp::new(1_000),
        );

        revoke(&mut registry, funding(), principal(2));
        assert_eq!(registry.trading_accounts_of(funding()), vec![]);

        // Revoking the last key must not reset the cooldown anchor: a re-grant
        // within the cooldown is still rejected.
        assert_eq!(
            registry.validate_trading_account(
                FundingAccount(funding()),
                TradingAccount(principal(3)),
                Timestamp::new(1_000 + cooldown - 1)
            ),
            Err(GrantError::CooldownActive),
            "revoke-all does not clear the cooldown anchor"
        );
    }

    #[test]
    fn should_revoke_removing_authority_from_both_maps() {
        let mut registry = user_registry();
        register(&mut registry, funding());
        record(&mut registry, funding(), principal(2), Timestamp::new(1));
        record(&mut registry, funding(), principal(3), Timestamp::new(2));

        revoke(&mut registry, funding(), principal(2));

        assert!(!registry.is_trading_account(&principal(2)));
        assert!(registry.is_trading_account(&principal(3)));
        assert_eq!(
            registry.trading_accounts_of(funding()),
            vec![principal(3)],
            "only the revoked key is dropped from the list"
        );
    }

    struct RevokeCase {
        desc: &'static str,
        setup: Setup,
        funding: candid::Principal,
        trading: candid::Principal,
        expected: Result<(), RevokeError>,
    }

    #[test]
    fn should_enforce_revoke_precondition() {
        let cases = vec![
            RevokeCase {
                desc: "revoking the caller's own trading account",
                setup: Box::new(|r| {
                    register(r, funding());
                    record(r, funding(), trading(), Timestamp::new(1));
                }),
                funding: funding(),
                trading: trading(),
                expected: Ok(()),
            },
            RevokeCase {
                desc: "revoking a principal that is not a trading account",
                setup: Box::new(|r| register(r, funding())),
                funding: funding(),
                trading: trading(),
                expected: Err(RevokeError::NotYourTradingAccount),
            },
            RevokeCase {
                desc: "revoking someone else's trading account",
                setup: Box::new(|r| {
                    register(r, funding());
                    register(r, principal(3));
                    record(r, principal(3), trading(), Timestamp::new(1));
                }),
                funding: funding(),
                trading: trading(),
                expected: Err(RevokeError::NotYourTradingAccount),
            },
        ];

        for case in cases {
            let mut registry = user_registry();
            (case.setup)(&mut registry);
            assert_eq!(
                registry
                    .validate_revoke(FundingAccount(case.funding), TradingAccount(case.trading)),
                case.expected,
                "{}",
                case.desc
            );
        }
    }

    #[test]
    fn should_record_and_list_trading_accounts() {
        let mut registry = user_registry();
        register(&mut registry, funding());
        assert_eq!(registry.trading_accounts_of(funding()), vec![]);

        record(&mut registry, funding(), principal(2), Timestamp::new(7));
        record(&mut registry, funding(), principal(3), Timestamp::new(9));

        assert_eq!(
            registry.trading_accounts_of(funding()),
            vec![principal(2), principal(3)]
        );
        assert!(registry.is_trading_account(&principal(2)));
        assert!(registry.is_trading_account(&principal(3)));
        assert!(!registry.is_trading_account(&funding()));
    }

    #[test]
    fn should_return_empty_list_for_unregistered_or_ungranted_principal() {
        let mut registry = user_registry();
        assert_eq!(registry.trading_accounts_of(principal(9)), vec![]);
        register(&mut registry, funding());
        assert_eq!(registry.trading_accounts_of(funding()), vec![]);
    }

    #[test]
    fn should_stamp_last_granted_at_on_each_add() {
        let mut registry = user_registry();
        let funding_id = registry.get_or_register(funding());

        record(&mut registry, funding(), principal(2), Timestamp::new(7));
        assert_eq!(
            registry
                .trading_accounts_by_funding
                .get(&funding_id)
                .unwrap()
                .last_granted_at,
            Timestamp::new(7)
        );

        record(&mut registry, funding(), principal(3), Timestamp::new(42));
        assert_eq!(
            registry
                .trading_accounts_by_funding
                .get(&funding_id)
                .unwrap()
                .last_granted_at,
            Timestamp::new(42)
        );
    }

    proptest! {
        #[test]
        fn should_roundtrip_trading_grant_through_stable_bytes(funding in arb_principal()) {
            let grant = TradingGrant { funding };
            prop_assert_eq!(TradingGrant::from_bytes(grant.to_bytes()), grant);
        }

        #[test]
        fn should_roundtrip_trading_account_list_through_stable_bytes(
            accounts in vec(arb_principal(), 0..=MAX_TRADING_ACCOUNTS_PER_USER),
            last_granted_at in arb_timestamp(),
        ) {
            let list = TradingAccountList { accounts, last_granted_at };
            prop_assert_eq!(TradingAccountList::from_bytes(list.to_bytes()), list);
        }
    }
}
