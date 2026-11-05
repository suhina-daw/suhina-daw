// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn a_copy_shares_until_one_side_changes() {
    let mut a = Shared::new(vec![1, 2, 3]);
    let b = a.clone();
    assert!(Shared::ptr_eq(&a, &b));
    a.push(4);
    assert!(!Shared::ptr_eq(&a, &b));
    assert_eq!(*a, vec![1, 2, 3, 4]);
    assert_eq!(*b, vec![1, 2, 3]);
}

#[test]
fn a_value_held_once_changes_in_place() {
    let mut a = Shared::new(vec![1]);
    let before = std::ptr::from_ref::<Vec<i32>>(&a);
    a.push(2);
    assert!(std::ptr::eq(before, std::ptr::from_ref::<Vec<i32>>(&a)));
}

#[test]
fn equal_values_are_equal_whether_shared_or_not() {
    let a = Shared::new(vec![1, 2]);
    assert_eq!(a, a.clone());
    assert_eq!(a, Shared::new(vec![1, 2]));
    assert_ne!(a, Shared::new(vec![2, 1]));
}

#[test]
fn bytes_share_and_compare_by_content() {
    let a = Bytes::from(vec![7u8; 16]);
    let b = a.clone();
    assert_eq!(a, b);
    assert_eq!(a, Bytes::from(&[7u8; 16][..]));
    assert_ne!(a, Bytes::from(vec![7u8; 15]));
    assert_eq!(Vec::from(b), vec![7u8; 16]);
    assert_eq!(format!("{a:?}"), "Bytes(16 bytes)");
}
