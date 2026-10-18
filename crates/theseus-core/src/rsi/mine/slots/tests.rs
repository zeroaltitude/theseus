//! Slot classification (§2.2): constant at 80%, bound to an earlier slot or an
//! earlier result, else an input; a value the miner would not keep (digest 0)
//! is never a constant.

use super::*;

fn call<'a>(d: &'a [u64], o: &'a [u64]) -> Option<SlotCall<'a>> {
    Some(SlotCall {
        digests: d,
        outs: o,
    })
}

fn text(d: u64) -> Option<String> {
    (d < 100).then(|| format!("v{d}"))
}

#[test]
fn a_slot_the_same_in_eighty_percent_is_a_constant_if_its_text_is_kept() {
    let (same, other): (&[u64], &[u64]) = (&[7], &[8]);
    let col = vec![
        call(same, &[]),
        call(same, &[]),
        call(same, &[]),
        call(same, &[]),
        call(other, &[]),
    ];
    let r = classify(&[vec![SlotType::Reference]], &[col], &text);
    assert_eq!(r[0][0].kind, SlotKind::Constant);
    assert_eq!(r[0][0].constant.as_deref(), Some("v7"));
    // 3 of 5 is not enough.
    let col = vec![
        call(same, &[]),
        call(same, &[]),
        call(same, &[]),
        call(other, &[]),
        call(&[9], &[]),
    ];
    assert_eq!(
        classify(&[vec![SlotType::Reference]], &[col], &text)[0][0].kind,
        SlotKind::Input
    );
    // Agreed, but the text was not kept (a digest over 100 here): an input.
    let big: &[u64] = &[500];
    let col = vec![call(big, &[]), call(big, &[]), call(big, &[])];
    assert_eq!(
        classify(&[vec![SlotType::Reference]], &[col], &text)[0][0].kind,
        SlotKind::Input
    );
}

#[test]
fn a_withheld_value_is_never_a_constant_or_bound() {
    let zero: &[u64] = &[0];
    let col = vec![call(zero, &[]), call(zero, &[]), call(zero, &[])];
    assert_eq!(
        classify(&[vec![SlotType::Str]], &[col], &text)[0][0].kind,
        SlotKind::Input
    );
}

#[test]
fn a_slot_equal_to_an_earlier_one_is_bound_to_it() {
    // Step 0 takes a branch (varies); step 1 takes the same one.
    let a: [[u64; 1]; 3] = [[1000], [2000], [3000]];
    let c0: Vec<_> = a.iter().map(|d| call(d, &[])).collect();
    let c1: Vec<_> = a.iter().map(|d| call(d, &[])).collect();
    let r = classify(
        &[vec![SlotType::Reference], vec![SlotType::Reference]],
        &[c0, c1],
        &text,
    );
    assert_eq!(r[0][0].kind, SlotKind::Input);
    assert_eq!(r[1][0].kind, SlotKind::Bound);
    assert_eq!(r[1][0].bound, Some(Bound::Slot { step: 0, slot: 0 }));
}

#[test]
fn a_value_an_earlier_result_named_is_bound_to_that_result() {
    let outs: [[u64; 1]; 3] = [[1000], [2000], [3000]];
    let c0: Vec<_> = outs.iter().map(|o| call(&[], o)).collect();
    let c1: Vec<_> = outs.iter().map(|o| call(o, &[])).collect();
    let r = classify(&[vec![], vec![SlotType::Path]], &[c0, c1], &text);
    assert_eq!(r[1][0].kind, SlotKind::Bound);
    assert_eq!(r[1][0].bound, Some(Bound::Result { step: 0 }));
}

#[test]
fn two_steps_that_name_the_same_constant_are_not_bound() {
    let c: [u64; 1] = [7];
    let col = |_: usize| vec![call(&c, &[]), call(&c, &[]), call(&c, &[])];
    let r = classify(
        &[vec![SlotType::Reference], vec![SlotType::Reference]],
        &[col(0), col(1)],
        &text,
    );
    assert_eq!(r[1][0].kind, SlotKind::Constant);
}

#[test]
fn a_step_some_occurrences_skipped_is_classified_over_those_that_took_it() {
    let c: [u64; 1] = [7];
    let col = vec![call(&c, &[]), None, call(&c, &[]), None, call(&c, &[])];
    assert_eq!(
        classify(&[vec![SlotType::Reference]], &[col], &text)[0][0].kind,
        SlotKind::Constant
    );
}
