use super::{CursorNotFound, History, InsertionSeq, SeqRecord};
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

#[test]
fn should_insert_and_get() {
    let mut store = store();
    store.insert_once(ALICE, TestKey::new(10), 100);
    assert_eq!(store.get(&TestKey::new(10)), Some(100));
    assert_eq!(store.len(), 1);
    assert!(store.contains_key(&TestKey::new(10)));
}

#[test]
fn should_return_none_for_missing_key() {
    let store = store();
    assert_eq!(store.get(&TestKey::new(10)), None);
    assert!(!store.contains_key(&TestKey::new(10)));
}

#[test]
#[should_panic(expected = "duplicate history key")]
fn should_panic_on_duplicate_key() {
    let mut store = store();
    store.insert_once(ALICE, TestKey::new(10), 100);
    store.insert_once(BOB, TestKey::new(10), 200);
}

#[test]
fn should_write_back_only_a_changed_record() {
    let mut store = store();
    store.insert_once(ALICE, TestKey::new(10), 100);

    store.modify(&TestKey::new(10), |record| {
        *record = 999;
        true
    });
    assert_eq!(store.get(&TestKey::new(10)), Some(999));

    store.modify(&TestKey::new(10), |record| {
        *record = 0; // mutated but reported unchanged: must not persist.
        false
    });
    assert_eq!(store.get(&TestKey::new(10)), Some(999));
}

#[test]
#[should_panic(expected = "missing")]
fn should_panic_when_modifying_a_missing_key() {
    let mut store = store();
    store.modify(&TestKey::new(10), |_| true);
}

/// A `page_by_user` scenario: a list of `(user, key)` insertions in order, the
/// user and cursor to page from, the requested length, and the expected keys
/// (as raw `u64`s) newest-first — or `Err(CursorNotFound)` when the cursor is
/// not found.
struct PageCase {
    desc: &'static str,
    inserts: Vec<(UserId, u64)>,
    user: UserId,
    after: Option<u64>,
    length: usize,
    expected: Result<Vec<u64>, CursorNotFound>,
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
            expected: Ok(vec![30, 20, 10]),
        },
        PageCase {
            desc: "page continues after cursor with next-older",
            inserts: vec![(ALICE, 10), (ALICE, 20), (ALICE, 30)],
            user: ALICE,
            after: Some(20),
            length: 10,
            expected: Ok(vec![10]),
        },
        PageCase {
            desc: "length clamps the page",
            inserts: vec![(ALICE, 10), (ALICE, 20), (ALICE, 30)],
            user: ALICE,
            after: None,
            length: 2,
            expected: Ok(vec![30, 20]),
        },
        PageCase {
            desc: "valid cursor with no older records is an empty page",
            inserts: vec![(ALICE, 10), (ALICE, 20)],
            user: ALICE,
            after: Some(10),
            length: 10,
            expected: Ok(vec![]),
        },
        PageCase {
            desc: "isolates owners: only the queried user's records",
            inserts: vec![(ALICE, 10), (BOB, 20), (ALICE, 30)],
            user: ALICE,
            after: None,
            length: 10,
            expected: Ok(vec![30, 10]),
        },
        PageCase {
            desc: "ordered by insertion sequence, not key value",
            inserts: vec![(ALICE, 30), (ALICE, 10), (ALICE, 20)],
            user: ALICE,
            after: None,
            length: 10,
            expected: Ok(vec![20, 10, 30]),
        },
        PageCase {
            desc: "unknown cursor is not found",
            inserts: vec![(ALICE, 10)],
            user: ALICE,
            after: Some(999),
            length: 10,
            expected: Err(CursorNotFound),
        },
        PageCase {
            desc: "foreign cursor (another user's record) is not found",
            inserts: vec![(ALICE, 10), (BOB, 20)],
            user: ALICE,
            after: Some(20),
            length: 10,
            expected: Err(CursorNotFound),
        },
        PageCase {
            desc: "unknown user yields an empty page",
            inserts: vec![(ALICE, 10)],
            user: BOB,
            after: None,
            length: 10,
            expected: Ok(vec![]),
        },
    ];

    for case in cases {
        let mut store = store();
        for (i, (user, key)) in case.inserts.iter().enumerate() {
            store.insert_once(*user, TestKey::new(*key), i as u64);
        }

        let got = store
            .page_by_user(case.user, case.after.map(TestKey::new), case.length)
            .map(|page| page.into_iter().map(TestKey::get).collect::<Vec<u64>>());

        assert_eq!(
            got, case.expected,
            "BUG ({}): page differs from expected",
            case.desc
        );
    }
}

/// A `range_primary` scenario over a fixed `[10, 20, 30, 40]` store: the lower
/// bound, the upper bound, the requested length, and the expected keys (as raw
/// `u64`s) newest-first.
struct RangeCase {
    desc: &'static str,
    lower: u64,
    upper: Bound<u64>,
    length: usize,
    expected: Vec<u64>,
}

#[test]
fn should_range_primary_newest_first_within_bounds() {
    let cases = vec![
        RangeCase {
            desc: "inclusive upper bound",
            lower: 20,
            upper: Bound::Included(40),
            length: 10,
            expected: vec![40, 30, 20],
        },
        RangeCase {
            desc: "excluded upper bound",
            lower: 20,
            upper: Bound::Excluded(40),
            length: 10,
            expected: vec![30, 20],
        },
        RangeCase {
            desc: "length clamps the range",
            lower: 10,
            upper: Bound::Included(40),
            length: 2,
            expected: vec![40, 30],
        },
    ];

    for case in cases {
        let mut store = store();
        for key in [10u64, 20, 30, 40] {
            store.insert_once(ALICE, TestKey::new(key), key);
        }

        let got: Vec<u64> = store
            .range_primary(
                TestKey::new(case.lower),
                case.upper.map(TestKey::new),
                case.length,
            )
            .into_iter()
            .map(|(key, _)| key.get())
            .collect();
        assert_eq!(
            got, case.expected,
            "BUG ({}): range differs from expected",
            case.desc
        );
    }
}

proptest! {
    #[test]
    fn should_roundtrip_seq_record_through_storable(seq in any::<u64>(), record in any::<u64>()) {
        let seq_record = SeqRecord { seq: InsertionSeq::new(seq), record };
        prop_assert_eq!(SeqRecord::<u64>::from_bytes(seq_record.to_bytes()), seq_record);
    }

    /// A single user's records page back in exact insertion order, newest-first,
    /// regardless of the key values chosen.
    #[test]
    fn should_page_a_users_full_history_newest_first(keys in proptest::collection::vec(any::<u64>(), 0..50)) {
        let unique: Vec<u64> = dedup_preserving_order(keys);
        let mut store = store();
        for key in &unique {
            store.insert_once(ALICE, TestKey::new(*key), *key);
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
            store.insert_once(ALICE, TestKey::new(key), key);
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
