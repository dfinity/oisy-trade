use super::{UserOrderKey, UserOrders};
use crate::order::{OrderBookId, OrderId, OrderSeq};
use candid::Principal;
use ic_stable_structures::{Storable, VectorMemory};

fn principal(byte: u8) -> Principal {
    Principal::from_slice(&[byte])
}

fn order_id(book: u64, seq: u64) -> OrderId {
    OrderId::new(OrderBookId::new(book), OrderSeq::new(seq))
}

fn index() -> UserOrders<VectorMemory> {
    UserOrders::new(VectorMemory::default())
}

#[test]
fn key_round_trips_through_storable() {
    for (owner, seq) in [
        (principal(1), 0u64),
        (principal(0xFF), 42),
        (Principal::anonymous(), u64::MAX),
        (Principal::management_canister(), 7),
    ] {
        let key = UserOrderKey::from_seq(owner, seq);
        let decoded = UserOrderKey::from_bytes(key.to_bytes());
        assert_eq!(decoded, key);
        assert_eq!(decoded.owner, owner);
        assert_eq!(decoded.rev_seq, u64::MAX - seq);
    }
}

#[test]
fn key_bytes_sort_same_owner_newest_first() {
    // A higher insertion seq (newer) must encode to smaller bytes, so a
    // forward range scan yields the most recent order first.
    let owner = principal(7);
    let newer = UserOrderKey::from_seq(owner, 10).into_bytes();
    let older = UserOrderKey::from_seq(owner, 3).into_bytes();
    assert!(newer < older);
}

#[test]
fn key_bytes_group_by_owner() {
    // A different owner's keys never fall between this owner's newest/oldest
    // bounds, so a range scan over one owner can't leak another's orders.
    let a = principal(1);
    let b = principal(2);
    let a_newest = UserOrderKey::newest(a).into_bytes();
    let a_oldest = UserOrderKey::oldest(a).into_bytes();
    let b_any = UserOrderKey::from_seq(b, 5).into_bytes();
    assert!(b_any < a_newest || b_any > a_oldest);
}

#[test]
fn page_returns_orders_newest_first() {
    let mut index = index();
    let owner = principal(1);
    index.insert(owner, 0, order_id(0, 100));
    index.insert(owner, 1, order_id(0, 101));
    index.insert(owner, 2, order_id(0, 102));

    assert_eq!(
        index.page(owner, 0, 10),
        vec![order_id(0, 102), order_id(0, 101), order_id(0, 100)]
    );
}

#[test]
fn page_paginates_with_skip_and_take() {
    let mut index = index();
    let owner = principal(1);
    for seq in 0..5 {
        index.insert(owner, seq, order_id(0, seq));
    }
    // Newest first: seq 4, 3, 2, 1, 0.
    assert_eq!(
        index.page(owner, 0, 2),
        vec![order_id(0, 4), order_id(0, 3)]
    );
    assert_eq!(
        index.page(owner, 2, 2),
        vec![order_id(0, 2), order_id(0, 1)]
    );
    assert_eq!(index.page(owner, 4, 2), vec![order_id(0, 0)]);
    assert_eq!(index.page(owner, 5, 2), Vec::<OrderId>::new());
}

#[test]
fn page_isolates_owners() {
    let mut index = index();
    let alice = principal(1);
    let bob = principal(2);
    // Interleaved global sequence: alice, bob, alice.
    index.insert(alice, 0, order_id(0, 10));
    index.insert(bob, 1, order_id(0, 20));
    index.insert(alice, 2, order_id(0, 11));

    assert_eq!(
        index.page(alice, 0, 10),
        vec![order_id(0, 11), order_id(0, 10)]
    );
    assert_eq!(index.page(bob, 0, 10), vec![order_id(0, 20)]);
    assert_eq!(index.page(principal(3), 0, 10), Vec::<OrderId>::new());
}

#[test]
fn page_orders_across_books_by_global_seq() {
    // Same owner trading on two books: ordering follows the global insertion
    // seq, not the per-book OrderId.
    let mut index = index();
    let owner = principal(1);
    index.insert(owner, 0, order_id(0, 5));
    index.insert(owner, 1, order_id(1, 0));
    index.insert(owner, 2, order_id(0, 6));

    assert_eq!(
        index.page(owner, 0, 10),
        vec![order_id(0, 6), order_id(1, 0), order_id(0, 5)]
    );
}

#[test]
#[should_panic(expected = "duplicate user-order index entry")]
fn insert_panics_on_duplicate_key() {
    let mut index = index();
    let owner = principal(1);
    index.insert(owner, 0, order_id(0, 1));
    index.insert(owner, 0, order_id(0, 2));
}
