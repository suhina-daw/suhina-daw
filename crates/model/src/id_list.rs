// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A list that holds the ids of what is in it (#763 › 1): markers, automation points, tempo
//! changes and a content's notes (#1182).
//!
//! The id is not part of the value. A marker, a point or a tempo change is its beat, value and
//! so on; which one it is belongs to the list it is in. So copying a value copies no id, and the
//! only ways into a list give a new one ([`IdList::push`], [`IdList::insert`], collecting values),
//! or keep the id of an [`Entry`] taken out of a list ([`IdList::take`] then [`IdList::put`]),
//! which cannot be copied. An id is therefore never in a list twice, by construction, and nothing
//! has to look for duplicates after an edit.
//!
//! Ids come from one counter for the whole program that only goes up, so an id is never handed
//! out twice, across edits, undo and redo and every open project. They live only while the
//! program runs: nothing refers to one of these objects across a save (ADR 0025 › Decided since),
//! so the file holds none, and a project read from a file gets new ones. A command names one
//! within the list it names (a lane's point, the arrangement's marker), so a list copied whole (a
//! duplicated track's lanes) keeping its ids is no clash.

use std::fmt;
use std::ops::{Deref, Index, IndexMut, Range};
use std::slice::SliceIndex;
use std::sync::atomic::{AtomicU64, Ordering};

/// The next id any [`IdList`] hands out. Starts at 1, so 0 is never an id.
static NEXT: AtomicU64 = AtomicU64::new(1);

/// The id of what an [`IdList`] holds. Implemented by [`crate::MarkerId`], [`crate::PointId`],
/// [`crate::TempoPointId`] and [`crate::NoteId`].
pub trait ListId: Copy + Eq + std::hash::Hash + fmt::Debug {
    /// An id never handed out before.
    fn fresh() -> Self;
}

macro_rules! list_id {
    ($($id:ident),*) => {$(
        impl ListId for crate::$id {
            fn fresh() -> Self {
                Self(NEXT.fetch_add(1, Ordering::Relaxed))
            }
        }
    )*};
}

list_id!(MarkerId, PointId, TempoPointId, NoteId);

/// A value taken out of an [`IdList`] with its id, to be put back where it belongs
/// ([`IdList::put`]): how an edit moves an object without it becoming another. It cannot be
/// copied, so its id goes back at most once.
#[derive(Debug)]
pub struct Entry<I, T> {
    id: I,
    pub value: T,
}

impl<I: ListId, T> Entry<I, T> {
    /// A new object: `value` with an id never handed out before.
    pub fn new(value: T) -> Self {
        Self { id: I::fresh(), value }
    }

    pub fn id(&self) -> I {
        self.id
    }
}

/// Values in order, each with an id of its own (see the module). Reads go through [`Deref`] to a
/// slice of the values, and a value is changed in place through [`IndexMut`] or
/// [`IdList::iter_mut`]; neither can reach an id.
///
/// Two lists are equal when their values are: the ids are not compared, so an edit that draws a
/// line again as it was changes nothing.
#[derive(Clone)]
pub struct IdList<I, T> {
    ids: Vec<I>,
    values: Vec<T>,
}

impl<I, T> Default for IdList<I, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I, T> IdList<I, T> {
    /// An empty list; `const`, so an empty list can be a `static`.
    pub const fn new() -> Self {
        Self { ids: Vec::new(), values: Vec::new() }
    }
}

impl<I: ListId, T> IdList<I, T> {
    /// The id of the value at `index`.
    pub fn id(&self, index: usize) -> Option<I> {
        self.ids.get(index).copied()
    }

    /// Every id, in the order of the values.
    pub fn ids(&self) -> &[I] {
        &self.ids
    }

    /// Where the value with `id` is now.
    pub fn position(&self, id: I) -> Option<usize> {
        self.ids.iter().position(|&i| i == id)
    }

    /// Each value with its id.
    pub fn entries(&self) -> impl Iterator<Item = (I, &T)> {
        self.ids.iter().copied().zip(&self.values)
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.values.iter_mut()
    }

    /// The value at `index`, to change; its id stays.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.values.get_mut(index)
    }

    /// The last value, to change; its id stays.
    pub fn last_mut(&mut self) -> Option<&mut T> {
        self.values.last_mut()
    }

    /// Adds a new object at the end and returns its id.
    pub fn push(&mut self, value: T) -> I {
        let id = I::fresh();
        self.ids.push(id);
        self.values.push(value);
        id
    }

    /// Adds a new object at `index` and returns its id.
    pub fn insert(&mut self, index: usize, value: T) -> I {
        let id = I::fresh();
        self.ids.insert(index, id);
        self.values.insert(index, value);
        id
    }

    /// Removes the object at `index`; its id is not handed out again.
    pub fn remove(&mut self, index: usize) -> T {
        self.ids.remove(index);
        self.values.remove(index)
    }

    /// Takes the object at `index` out with its id, to be [put](Self::put) back.
    pub fn take(&mut self, index: usize) -> Entry<I, T> {
        Entry { id: self.ids.remove(index), value: self.values.remove(index) }
    }

    /// Puts `entry` at `index`, keeping its id, unless an object in the list has that id already
    /// (an entry taken from a copy of this list): then it is a new object and gets a new one.
    pub fn put(&mut self, index: usize, entry: Entry<I, T>) {
        let id = if self.ids.contains(&entry.id) { I::fresh() } else { entry.id };
        self.ids.insert(index, id);
        self.values.insert(index, entry.value);
    }

    /// Takes the objects in `range` out with their ids.
    pub fn take_range(&mut self, range: Range<usize>) -> Vec<Entry<I, T>> {
        let ids = self.ids.drain(range.clone());
        ids.zip(self.values.drain(range)).map(|(id, value)| Entry { id, value }).collect()
    }

    /// Replaces the objects in `range` with `entries`, as [`put`](Self::put) would put each.
    pub fn splice(&mut self, range: Range<usize>, entries: Vec<Entry<I, T>>) {
        self.ids.drain(range.clone());
        self.values.drain(range.clone());
        let mut used: std::collections::HashSet<I> = self.ids.iter().copied().collect();
        let (ids, values): (Vec<I>, Vec<T>) = entries
            .into_iter()
            .map(|Entry { id, value }| {
                let id = if used.insert(id) { id } else { I::fresh() };
                used.insert(id);
                (id, value)
            })
            .unzip();
        self.ids.splice(range.start..range.start, ids);
        self.values.splice(range.start..range.start, values);
    }

    /// A list of `entries`, as [`put`](Self::put) would put each.
    pub fn from_entries(entries: Vec<Entry<I, T>>) -> Self {
        let mut list = Self::new();
        list.splice(0..0, entries);
        list
    }

    /// Keeps the objects for which `keep` is true, in order.
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        let flags: Vec<bool> = self.values.iter().map(&mut keep).collect();
        let mut flag = flags.iter();
        self.ids.retain(|_| flag.next().copied().unwrap_or(false));
        let mut flag = flags.iter();
        self.values.retain(|_| flag.next().copied().unwrap_or(false));
    }

    /// Orders the objects by `compare`, each keeping its id; a stable sort.
    pub fn sort_by(&mut self, mut compare: impl FnMut(&T, &T) -> std::cmp::Ordering) {
        let mut order: Vec<usize> = (0..self.values.len()).collect();
        order.sort_by(|&a, &b| compare(&self.values[a], &self.values[b]));
        let mut entries: Vec<Option<Entry<I, T>>> =
            self.take_range(0..self.values.len()).into_iter().map(Some).collect();
        for index in order {
            if let Some(Entry { id, value }) = entries[index].take() {
                self.ids.push(id);
                self.values.push(value);
            }
        }
    }

    pub fn clear(&mut self) {
        self.ids.clear();
        self.values.clear();
    }

    pub fn truncate(&mut self, len: usize) {
        self.ids.truncate(len);
        self.values.truncate(len);
    }

    /// The values, without their ids.
    pub fn into_values(self) -> Vec<T> {
        self.values
    }
}

impl<I: ListId, T> Extend<T> for IdList<I, T> {
    /// Adds each value as a new object.
    fn extend<V: IntoIterator<Item = T>>(&mut self, values: V) {
        for value in values {
            self.push(value);
        }
    }
}

impl<I: ListId, T> FromIterator<T> for IdList<I, T> {
    /// A list of new objects.
    fn from_iter<V: IntoIterator<Item = T>>(values: V) -> Self {
        let mut list = Self::new();
        list.extend(values);
        list
    }
}

impl<I: ListId, T> From<Vec<T>> for IdList<I, T> {
    /// A list of new objects.
    fn from(values: Vec<T>) -> Self {
        values.into_iter().collect()
    }
}

impl<I, T> Deref for IdList<I, T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values
    }
}

impl<I, T, X: SliceIndex<[T]>> Index<X> for IdList<I, T> {
    type Output = X::Output;
    /// The value at `index`, or the values in a range, as a slice is indexed.
    fn index(&self, index: X) -> &X::Output {
        &self.values[index]
    }
}

impl<I, T, X: SliceIndex<[T]>> IndexMut<X> for IdList<I, T> {
    /// The value at `index` (or the values in a range), to change; the ids stay.
    fn index_mut(&mut self, index: X) -> &mut X::Output {
        &mut self.values[index]
    }
}

impl<'a, I, T> IntoIterator for &'a IdList<I, T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

impl<'a, I, T> IntoIterator for &'a mut IdList<I, T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    /// Each value, to change; the ids stay.
    fn into_iter(self) -> Self::IntoIter {
        self.values.iter_mut()
    }
}

impl<I, T: PartialEq> PartialEq for IdList<I, T> {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}

impl<I: fmt::Debug, T: fmt::Debug> fmt::Debug for IdList<I, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.ids.iter().zip(&self.values)).finish()
    }
}

#[cfg(test)]
#[path = "id_list_tests.rs"]
mod tests;
