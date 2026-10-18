use std::hint::black_box;

use glib::{variant::ToVariant, Variant};

const VALUES: [&str; 5] = ["alpha", "", "\u{00e5}\u{4e2d}", "delta", "omega"];

fn string_array() -> Variant {
    Variant::array_from_iter::<String>(black_box(VALUES).into_iter().map(|s| s.to_variant()))
}

#[test]
#[allow(
    clippy::iter_nth_zero,
    reason = "Regression coverage must exercise nth(0)."
)]
fn variant_str_iter_next_last_and_nth() {
    let variant = string_array();
    let mut iter = black_box(&variant).array_iter_str().unwrap();
    for expected in VALUES {
        assert_eq!(iter.next(), Some(expected));
    }
    assert_eq!(iter.next(), None);
    assert_eq!(iter.last(), None);

    assert_eq!(variant.array_iter_str().unwrap().last(), Some("omega"));
    let mut iter = variant.array_iter_str().unwrap();
    assert_eq!(iter.nth(1), Some(""));
    assert_eq!(iter.nth(0), Some(VALUES[2]));
    assert_eq!(iter.last(), Some("omega"));

    let mut iter = variant.array_iter_str().unwrap();
    assert_eq!(iter.nth(usize::MAX), None);
    assert_eq!(iter.next(), None);
}

#[test]
fn variant_str_iter_reverse_and_mixed_ends() {
    let variant = string_array();
    let actual: Vec<_> = black_box(&variant)
        .array_iter_str()
        .unwrap()
        .rev()
        .collect();
    assert_eq!(actual, VALUES.into_iter().rev().collect::<Vec<_>>());

    let mut iter = variant.array_iter_str().unwrap();
    assert_eq!(iter.next_back(), Some("omega"));
    assert_eq!(iter.next(), Some("alpha"));
    assert_eq!(iter.nth_back(1), Some(VALUES[2]));
    assert_eq!(iter.len(), 1);
    assert_eq!(iter.next_back(), Some(""));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);

    let mut iter = variant.array_iter_str().unwrap();
    assert_eq!(iter.nth_back(usize::MAX), None);
    assert_eq!(iter.next_back(), None);
}

#[test]
#[allow(
    clippy::iter_nth_zero,
    reason = "Regression coverage must exercise nth(0)."
)]
fn variant_str_iter_empty_array() {
    let variant = Variant::array_from_iter::<String>(std::iter::empty());
    let mut iter = black_box(&variant).array_iter_str().unwrap();
    assert_eq!(iter.len(), 0);
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
    assert_eq!(iter.nth(0), None);
    assert_eq!(iter.nth_back(0), None);
    assert_eq!(iter.last(), None);
}
