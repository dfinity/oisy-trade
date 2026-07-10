use super::token::split_net_fee;
use super::{Balance, BalanceKey};
use crate::order::{Quantity, TokenId};
use crate::user::UserId;
use ic_stable_structures::{Memory, StableBTreeMap};
use std::collections::BTreeMap;

/// In-heap write-back buffer over the balance map for the balance operations
/// of one settling event, opened by [`TokenBalance::settling_batch`].
///
/// The taker of a large sweep is party to every fill, so its two balance rows
/// would otherwise be read-modify-written on each fill. The buffer collapses
/// that to a single stable read per row on first touch and a single write-back
/// per dirty row on [`flush`](Self::flush), while the fee pool keeps accruing
/// on the heap as in [`TokenBalance::transfer`].
///
/// [`TokenBalance::settling_batch`]: super::TokenBalance::settling_batch
/// [`TokenBalance::transfer`]: super::TokenBalance::transfer
#[must_use = "BalanceSettlingBatch buffers balance mutations that are only applied by `flush()`; \
              dropping it without flushing silently discards them"]
pub struct BalanceSettlingBatch<'a, M: Memory> {
    balances: &'a mut StableBTreeMap<BalanceKey, Balance, M>,
    fee_balances: &'a mut BTreeMap<TokenId, Quantity>,
    buffer: BTreeMap<BalanceKey, BufferedBalance>,
}

struct BufferedBalance {
    balance: Balance,
    existed: bool,
}

impl<'a, M: Memory> BalanceSettlingBatch<'a, M> {
    pub(super) fn new(
        balances: &'a mut StableBTreeMap<BalanceKey, Balance, M>,
        fee_balances: &'a mut BTreeMap<TokenId, Quantity>,
    ) -> Self {
        Self {
            balances,
            fee_balances,
            buffer: BTreeMap::new(),
        }
    }

    /// Buffered counterpart of [`TokenBalance::transfer`]: debits `gross` from
    /// the debtor's reserved, credits `gross - fee` to the creditor's free, and
    /// accrues `fee` to the token's fee pool. A self-transfer lands the credit
    /// on the just-debited buffered row.
    ///
    /// [`TokenBalance::transfer`]: super::TokenBalance::transfer
    pub fn transfer(
        &mut self,
        debtor: UserId,
        creditor: UserId,
        token: &TokenId,
        gross: Quantity,
        fee: Quantity,
    ) {
        bench_scopes!("balances", "balances::transfer");
        let net = split_net_fee(self.fee_balances, token, gross, fee);

        self.load_existing(
            BalanceKey::new(*token, debtor),
            "BUG: debtor balance missing",
        )
        .debit_reserved(&gross);
        self.load_or_create(BalanceKey::new(*token, creditor))
            .deposit(net);
    }

    /// Buffered counterpart of [`TokenBalance::unreserve`]: moves `amount` from
    /// the user's reserved to their free balance.
    ///
    /// [`TokenBalance::unreserve`]: super::TokenBalance::unreserve
    pub fn unreserve(&mut self, user: UserId, token: &TokenId, amount: Quantity) {
        bench_scopes!("balances", "balances::unreserve");
        self.load_existing(
            BalanceKey::new(*token, user),
            "BUG: user balance missing for unreserve",
        )
        .unreserve(amount);
    }

    /// Write each buffered row back to the stable map exactly once, eliding
    /// rows that neither existed before the batch nor hold a non-zero balance —
    /// matching the empty-row elision of [`TokenBalance::update`].
    ///
    /// [`TokenBalance::update`]: super::TokenBalance
    pub fn flush(self) {
        bench_scopes!("balances", "balances::flush");
        for (key, buffered) in self.buffer {
            if buffered.existed || !buffered.balance.is_zero() {
                self.balances.insert(key, buffered.balance);
            }
        }
    }

    /// Buffer a row that must already exist in the balance map, or have been
    /// created earlier in this settling event, mirroring the `expect(...)` of
    /// the debtor read in [`TokenBalance::transfer`] and the target read in
    /// [`TokenBalance::unreserve`]. On the row's first touch this batch, traps
    /// with `msg` if it is absent from the stable map.
    ///
    /// [`TokenBalance::transfer`]: super::TokenBalance::transfer
    /// [`TokenBalance::unreserve`]: super::TokenBalance::unreserve
    fn load_existing(&mut self, key: BalanceKey, msg: &'static str) -> &mut Balance {
        let entry = self.buffer.entry(key).or_insert_with(|| BufferedBalance {
            existed: true,
            balance: self.balances.get(&key).expect(msg),
        });
        &mut entry.balance
    }

    /// Buffer a row that may not yet exist, mirroring the creditor credit in
    /// [`TokenBalance::transfer`], which creates the entry on demand.
    ///
    /// [`TokenBalance::transfer`]: super::TokenBalance::transfer
    fn load_or_create(&mut self, key: BalanceKey) -> &mut Balance {
        let entry = self.buffer.entry(key).or_insert_with(|| {
            let prev = self.balances.get(&key);
            BufferedBalance {
                existed: prev.is_some(),
                balance: prev.unwrap_or_default(),
            }
        });
        &mut entry.balance
    }
}
