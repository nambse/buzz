use super::*;
use crate::reviewed_memory::conversation::tests::{fixture, pin};

#[test]
fn central_candidate_selection_uses_priority_before_uuid_and_sends_only_eight_pins() {
    let (a, _, mut selected) = fixture(true);
    let text = "Approved remote fact";
    let candidates = (0..12)
        .map(|n| Candidate {
            pin: pin(&selected, 100 + n, (n % 3) as u8, text),
            content: text.into(),
        })
        .rev()
        .collect();
    choose(&mut selected, &a, candidates).unwrap();
    assert_eq!(
        selected
            .records
            .iter()
            .map(ReviewedSelectionPin::fact_id)
            .collect::<Vec<_>>(),
        [100, 103, 106, 109, 101, 104, 107, 110]
            .into_iter()
            .map(Uuid::from_u128)
            .collect::<Vec<_>>()
    );
    assert!(selected.truncated);
    // Exact common remote metadata remains available without local text.
    assert_eq!(
        selected.records[0].common_pin().fact_id,
        Uuid::from_u128(100)
    );
}

#[test]
fn central_candidate_selection_enforces_content_and_rendered_budgets_before_remote_read() {
    let (a, _, mut selected) = fixture(true);
    let large = "x".repeat(4096);
    let escaped = "\"".repeat(4096);
    let candidates = vec![
        Candidate {
            pin: pin(&selected, 1, 0, &escaped),
            content: escaped,
        },
        Candidate {
            pin: pin(&selected, 2, 0, &large),
            content: large.clone(),
        },
        Candidate {
            pin: pin(&selected, 3, 1, &large),
            content: large,
        },
        Candidate {
            pin: pin(&selected, 4, 2, "extra"),
            content: "extra".into(),
        },
    ];
    choose(&mut selected, &a, candidates).unwrap();
    assert_eq!(
        selected
            .records
            .iter()
            .map(ReviewedSelectionPin::fact_id)
            .collect::<Vec<_>>(),
        [2, 3].into_iter().map(Uuid::from_u128).collect::<Vec<_>>()
    );
    assert!(selected.truncated);
    let (a, _, mut selected) = fixture(false);
    let text = "fact";
    let bad = Candidate {
        pin: pin(&selected, 1, 2, text),
        content: text.into(),
    };
    assert!(
        choose(&mut selected, &a, vec![bad]).is_err(),
        "Office may not gain project scope"
    );
}
