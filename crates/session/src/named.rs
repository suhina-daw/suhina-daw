// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The item of an [`IdList`] a command names: its index (from 0), or `{"id": n}` with its id,
//! found where it is now (#763 › 1). Notes, automation points, tempo changes and markers are
//! named alike; each caller words its own errors from the [`Miss`].

use std::collections::HashMap;

use serde_json::Value;
use suhina_model::{IdList, ListId};

/// Why an item did not name an entry.
pub(crate) enum Miss {
    /// `{"id": …}` with no entry of that id (or an id that is not a whole number).
    Id(Value),
    /// Neither a whole number nor `{"id": …}`.
    NotANumber,
    /// An index past the end.
    Range { index: u64, count: usize },
}

/// Finds the entries a command names in one list. Ids are looked up in a map built on the first
/// one, so naming many entries by id takes one pass over the list, not one for each.
pub(crate) struct Named<'a, I: ListId, T> {
    list: &'a IdList<I, T>,
    make: fn(u64) -> I,
    by_id: Option<HashMap<I, usize>>,
}

impl<'a, I: ListId, T> Named<'a, I, T> {
    /// `make` turns an id number from the command into the list's id type.
    pub(crate) fn new(list: &'a IdList<I, T>, make: fn(u64) -> I) -> Self {
        Self { list, make, by_id: None }
    }

    /// The index `item` names.
    pub(crate) fn at(&mut self, item: &Value) -> Result<usize, Miss> {
        let (list, make) = (self.list, self.make);
        let by_id = &mut self.by_id;
        find(list, make, item, |id| {
            let map =
                by_id.get_or_insert_with(|| list.ids().iter().copied().enumerate().map(|(i, id)| (id, i)).collect());
            map.get(&id).copied()
        })
    }
}

/// The index `item` names in `list`, for one item: no map is built.
pub(crate) fn find_one<I: ListId, T>(list: &IdList<I, T>, make: fn(u64) -> I, item: &Value) -> Result<usize, Miss> {
    find(list, make, item, |id| list.position(id))
}

fn find<I: ListId, T>(
    list: &IdList<I, T>,
    make: fn(u64) -> I,
    item: &Value,
    position: impl FnOnce(I) -> Option<usize>,
) -> Result<usize, Miss> {
    if let Some(id) = item.get("id") {
        return id.as_u64().map(make).and_then(position).ok_or_else(|| Miss::Id(id.clone()));
    }
    let count = list.len();
    let index = item.as_u64().ok_or(Miss::NotANumber)?;
    usize::try_from(index).ok().filter(|i| *i < count).ok_or(Miss::Range { index, count })
}
