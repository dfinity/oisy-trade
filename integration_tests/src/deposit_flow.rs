use crate::Setup;
use candid::{Nat, Principal};
use dex_types::{DepositRequest, TokenId};

/// Builder for the deposit flow in integration tests.
///
/// Each step is opt-in and amounts are explicit.
pub struct DepositFlow<'a> {
    setup: &'a Setup,
    user: Principal,
    token_id: TokenId,
    mint_amount: Option<Nat>,
    approve_amount: Option<Nat>,
    deposit_amount: Option<Nat>,
}

impl<'a> DepositFlow<'a> {
    pub fn new(setup: &'a Setup, user: Principal, token_id: TokenId) -> Self {
        Self {
            setup,
            user,
            token_id,
            mint_amount: None,
            approve_amount: None,
            deposit_amount: None,
        }
    }

    pub fn mint(mut self, amount: impl Into<Nat>) -> Self {
        self.mint_amount = Some(amount.into());
        self
    }

    pub fn approve(mut self, amount: impl Into<Nat>) -> Self {
        self.approve_amount = Some(amount.into());
        self
    }

    pub fn deposit(mut self, amount: impl Into<Nat>) -> Self {
        self.deposit_amount = Some(amount.into());
        self
    }

    pub async fn execute(self) {
        let ledger = self.setup.ledger_for(&self.token_id);

        if let Some(amount) = self.mint_amount {
            ledger
                .icrc1_transfer(
                    self.setup.controller(),
                    icrc_ledger_types::icrc1::account::Account {
                        owner: self.user,
                        subaccount: None,
                    },
                    amount,
                )
                .await;
        }

        if let Some(amount) = self.approve_amount {
            ledger
                .icrc2_approve(self.user, self.setup.dex_account(), amount)
                .await;
        }

        if let Some(amount) = self.deposit_amount {
            self.setup
                .dex_client_with_caller(self.user)
                .deposit(DepositRequest {
                    token_id: self.token_id.clone(),
                    amount,
                })
                .await
                .expect("deposit failed");
        }
    }
}
