// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{Entry, IdList, Marker, MarkerId};

fn list(beats: &[f64]) -> IdList<MarkerId, Marker> {
    beats.iter().map(|&b| Marker::new(b, format!("{b}"))).collect()
}

fn unique(list: &IdList<MarkerId, Marker>) -> bool {
    let mut ids = list.ids().to_vec();
    ids.sort();
    ids.dedup();
    ids.len() == list.len()
}

/// Every way in gives a new id, so a copied value is a new object.
#[test]
fn a_copied_value_is_a_new_object() {
    let mut markers = list(&[0.0, 4.0]);
    let copy = markers[0].clone();
    let id = markers.push(copy);
    assert_ne!(Some(id), markers.id(0));
    markers.insert(1, markers[1].clone());
    markers.extend([markers[0].clone()]);
    assert_eq!(markers.len(), 5);
    assert!(unique(&markers));
}

/// A value changed in place, moved or sorted keeps its id.
#[test]
fn an_object_keeps_its_id_through_changes_and_moves() {
    let mut markers = list(&[0.0, 4.0, 8.0]);
    let ids = markers.ids().to_vec();
    markers[1].name = "Verse".into();
    assert_eq!(markers.ids(), ids);
    let taken = markers.take(0);
    markers.put(2, taken);
    assert_eq!(markers.ids(), [ids[1], ids[2], ids[0]]);
    markers.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    assert_eq!(markers.ids(), ids);
    assert_eq!(markers.position(ids[2]), Some(2));
}

/// An entry taken from a copy of a list and put into the list that still has its id is a new
/// object: an id is never in a list twice.
#[test]
fn an_id_is_never_in_a_list_twice() {
    let mut markers = list(&[0.0, 4.0]);
    let mut copy = markers.clone();
    let taken = copy.take(0);
    markers.put(2, taken);
    assert!(unique(&markers));
    let entries = copy.take_range(0..1);
    markers.splice(0..0, entries);
    assert!(unique(&markers));
    let fresh = IdList::from_entries(vec![Entry::new(Marker::new(1.0, "a")), Entry::new(Marker::new(2.0, "b"))]);
    assert!(unique(&fresh));
}

/// A removed object's id is not handed out again, and two lists with the same values are equal
/// whatever their ids.
#[test]
fn ids_are_not_reused_and_not_compared() {
    let mut markers = list(&[0.0, 4.0]);
    let gone = markers.id(1);
    markers.remove(1);
    markers.push(Marker::new(4.0, "4"));
    assert_ne!(markers.id(1), gone);
    assert_eq!(markers, list(&[0.0, 4.0]));
    assert_ne!(markers.ids(), list(&[0.0, 4.0]).ids());
}
