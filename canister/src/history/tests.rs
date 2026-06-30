use super::{CursorNotFound, History, SeqEnvelope};
use crate::ids::{Seq, SeqMarker};
use crate::user::UserId;
use ic_stable_structures::{Storable, VectorMemory};
use proptest::prelude::{any, prop_assert_eq, proptest};
use std::ops::Bound;

struct TestKeyMarker;
impl SeqMarker for TestKeyMarker {
    const NAME: &'static str = "TestKey";
}
/// A minimal `Copy + Ord + Storable` primary key for exercising the generic
/// store without pulling in an order/trade type.
type TestKey = Seq<TestKeyMarker>;

type TestStore = History<VectorMemory, TestKey, u64>;

fn store() -> TestStore {
    History::new(VectorMemory::default(), VectorMemory::default())
}

const ALICE: UserId = UserId::new(1);
const BOB: UserId = UserId::new(2);

/// A `page_by_user` scenario: a list of `(user, key)` insertions in order, the
/// user and cursor to page from, the requested length, and the expected keys
/// (as raw `u64`s) newest-first — or `None` when [`CursorNotFound`] is expected.
struct PageCase {
    desc: &'static str,
    inserts: Vec<(UserId, u64)>,
    user: UserId,
    after: Option<u64>,
    length: usize,
    expected: Option<Vec<u64>>,
}

#[test]
fn should_page_by_user() {
    let cases = vec![
        PageCase {
            desc: "newest first, no cursor",
            inserts: vec![(ALICE, 10), (ALICE, 20), (ALICE, 30)],
            user: ALICE,
            after: None,
            length: 10,
            expected: Some(vec![30, 20, 10]),
        },
        PageCase {
            desc: "page continues after cursor with next-older",
            inserts: vec![(ALICE, 10), (ALICE, 20), (ALICE, 30)],
            user: ALICE,
            after: Some(20),
            length: 10,
            expected: Some(vec![10]),
        },
        PageCase {
            desc: "length clamps the page",
            inserts: vec![(ALICE, 10), (ALICE, 20), (ALICE, 30)],
            user: ALICE,
            after: None,
            length: 2,
            expected: Some(vec![30, 20]),
        },
        PageCase {
            desc: "valid cursor with no older records is an empty page",
            inserts: vec![(ALICE, 10), (ALICE, 20)],
            user: ALICE,
            after: Some(10),
            length: 10,
            expected: Some(vec![]),
        },
        PageCase {
            desc: "isolates owners: only the queried user's records",
            inserts: vec![(ALICE, 10), (BOB, 20), (ALICE, 30)],
            user: ALICE,
            after: None,
            length: 10,
            expected: Some(vec![30, 10]),
        },
        PageCase {
            desc: "ordered by insertion sequence, not key value",
            inserts: vec![(ALICE, 30), (ALICE, 10), (ALICE, 20)],
            user: ALICE,
            after: None,
            length: 10,
            expected: Some(vec![20, 10, 30]),
        },
        PageCase {
            desc: "unknown cursor is not found",
            inserts: vec![(ALICE, 10)],
            user: ALICE,
            after: Some(999),
            length: 10,
            expected: None,
        },
        PageCase {
            desc: "foreign cursor (another user's record) is not found",
            inserts: vec![(ALICE, 10), (BOB, 20)],
            user: ALICE,
            after: Some(20),
            length: 10,
            expected: None,
        },
        PageCase {
            desc: "unknown user yields an empty page",
            inserts: vec![(ALICE, 10)],
            user: BOB,
            after: None,
            length: 10,
            expected: Some(vec![]),
        },
    ];

    for case in cases {
        let mut store = store();
        for (i, (user, key)) in case.inserts.iter().enumerate() {
            store.insert(*user, TestKey::new(*key), i as u64);
        }

        let result = store.page_by_user(case.user, case.after.map(TestKey::new), case.length);

        match case.expected {
            None => assert_eq!(
                result,
                Err(CursorNotFound),
                "BUG ({}): expected cursor not found",
                case.desc
            ),
            Some(keys) => {
                let got: Vec<u64> = result
                    .unwrap_or_else(|_| panic!("BUG ({}): unexpected CursorNotFound", case.desc))
                    .into_iter()
                    .map(TestKey::get)
                    .collect();
                assert_eq!(got, keys, "BUG ({}): page differs from expected", case.desc);
            }
        }
    }
}

#[test]
fn should_range_primary_newest_first_within_bounds() {
    let mut store = store();
    for key in [10u64, 20, 30, 40] {
        store.insert(ALICE, TestKey::new(key), key);
    }

    let inclusive: Vec<u64> = store
        .range_primary(TestKey::new(20), Bound::Included(TestKey::new(40)), 10)
        .into_iter()
        .map(|(key, _)| key.get())
        .collect();
    assert_eq!(inclusive, vec![40, 30, 20]);

    let excluded: Vec<u64> = store
        .range_primary(TestKey::new(20), Bound::Excluded(TestKey::new(40)), 10)
        .into_iter()
        .map(|(key, _)| key.get())
        .collect();
    assert_eq!(excluded, vec![30, 20]);

    let clamped: Vec<u64> = store
        .range_primary(TestKey::new(10), Bound::Included(TestKey::new(40)), 2)
        .into_iter()
        .map(|(key, _)| key.get())
        .collect();
    assert_eq!(clamped, vec![40, 30]);
}

proptest! {
    #[test]
    fn should_roundtrip_seq_envelope_through_storable(seq in any::<u64>(), record in any::<u64>()) {
        let envelope = SeqEnvelope { seq, record };
        prop_assert_eq!(SeqEnvelope::<u64>::from_bytes(envelope.to_bytes()), envelope);
    }

    /// A single user's records page back in exact insertion order, newest-first,
    /// regardless of the key values chosen.
    #[test]
    fn should_page_a_users_full_history_newest_first(keys in proptest::collection::vec(any::<u64>(), 0..50)) {
        let unique: Vec<u64> = dedup_preserving_order(keys);
        let mut store = store();
        for key in &unique {
            store.insert(ALICE, TestKey::new(*key), *key);
        }

        let paged: Vec<u64> = store
            .page_by_user(ALICE, None, unique.len() + 1)
            .expect("no cursor should never be not-found")
            .into_iter()
            .map(TestKey::get)
            .collect();

        let mut expected = unique;
        expected.reverse();
        prop_assert_eq!(paged, expected);
    }

    /// Paging the whole history one record at a time, threading each page's last
    /// key as the next cursor, visits every record exactly once newest-first.
    #[test]
    fn should_walk_full_history_via_cursor(count in 0u64..30) {
        let mut store = store();
        for key in 0..count {
            store.insert(ALICE, TestKey::new(key), key);
        }

        let mut walked = vec![];
        let mut after = None;
        loop {
            let page = store.page_by_user(ALICE, after, 1).expect("valid cursor");
            let Some(key) = page.first().copied() else { break };
            walked.push(key.get());
            after = Some(key);
        }

        let expected: Vec<u64> = (0..count).rev().collect();
        prop_assert_eq!(walked, expected);
    }
}

fn dedup_preserving_order(keys: Vec<u64>) -> Vec<u64> {
    let mut seen = std::collections::BTreeSet::new();
    keys.into_iter().filter(|k| seen.insert(*k)).collect()
}
