// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The objects a VST3 plug-in asks its host to make (`IHostApplication::createInstance`): a
//! message and its attribute list, which a plug-in's component and controller send each other
//! through their connection points. Many plug-ins make one as soon as they are connected and do
//! not check the answer, so a host that makes none crashes them.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_void};
use std::sync::Mutex;

use vst3::Steinberg::Vst::IAttributeList_::AttrID;
use vst3::Steinberg::Vst::{IAttributeList, IAttributeListTrait, IMessage, IMessageTrait, TChar};
use vst3::Steinberg::*;
use vst3::{Class, ComWrapper, Interface};

/// Longest attribute value kept, in bytes; a larger one is refused.
const MAX_VALUE: usize = 64 * 1024 * 1024;

enum Value {
    Int(i64),
    Float(f64),
    /// UTF-16, without the closing zero.
    String(Vec<TChar>),
    Binary(Vec<u8>),
}

/// An attribute list: values by name.
#[derive(Default)]
pub(crate) struct AttributeList {
    values: Mutex<HashMap<CString, Value>>,
}

impl Class for AttributeList {
    type Interfaces = (IAttributeList,);
}

/// The name an attribute is kept under, or `None` for a null pointer.
fn key(id: AttrID) -> Option<CString> {
    if id.is_null() {
        return None;
    }
    // SAFETY: a non-null attribute id is a zero-terminated string the plug-in owns for the call.
    Some(unsafe { CStr::from_ptr(id) }.to_owned())
}

impl AttributeList {
    fn set(&self, id: AttrID, value: Value) -> tresult {
        let Some(key) = key(id) else { return kInvalidArgument };
        let mut values = self.values.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        values.insert(key, value);
        kResultOk
    }

    fn with<R>(&self, id: AttrID, f: impl FnOnce(&Value) -> Option<R>) -> Option<R> {
        let key = key(id)?;
        let values = self.values.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        values.get(&key).and_then(f)
    }
}

impl IAttributeListTrait for AttributeList {
    unsafe fn setInt(&self, id: AttrID, value: int64) -> tresult {
        self.set(id, Value::Int(value))
    }

    unsafe fn getInt(&self, id: AttrID, value: *mut int64) -> tresult {
        let Some(found) = self.with(id, |v| if let Value::Int(i) = v { Some(*i) } else { None }) else {
            return kResultFalse;
        };
        // SAFETY: the plug-in passes a pointer to its own value to fill, or null.
        match unsafe { value.as_mut() } {
            Some(slot) => {
                *slot = found;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn setFloat(&self, id: AttrID, value: f64) -> tresult {
        self.set(id, Value::Float(value))
    }

    unsafe fn getFloat(&self, id: AttrID, value: *mut f64) -> tresult {
        let Some(found) = self.with(id, |v| if let Value::Float(f) = v { Some(*f) } else { None }) else {
            return kResultFalse;
        };
        // SAFETY: the plug-in passes a pointer to its own value to fill, or null.
        match unsafe { value.as_mut() } {
            Some(slot) => {
                *slot = found;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn setString(&self, id: AttrID, string: *const TChar) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let mut text = Vec::new();
        loop {
            if text.len() * 2 >= MAX_VALUE {
                return kInvalidArgument;
            }
            // SAFETY: a non-null string is zero-terminated (the call's contract); this reads up to
            // and including its zero, one unit at a time.
            let unit = unsafe { string.wrapping_add(text.len()).read() };
            if unit == 0 {
                break;
            }
            text.push(unit);
        }
        self.set(id, Value::String(text))
    }

    unsafe fn getString(&self, id: AttrID, string: *mut TChar, size_in_bytes: uint32) -> tresult {
        let Some(text) = self.with(id, |v| if let Value::String(s) = v { Some(s.clone()) } else { None }) else {
            return kResultFalse;
        };
        let room = (size_in_bytes as usize) / std::mem::size_of::<TChar>();
        if string.is_null() || room == 0 {
            return kInvalidArgument;
        }
        let copied = text.len().min(room - 1);
        for (index, unit) in text.iter().take(copied).chain(std::iter::once(&0)).enumerate() {
            // SAFETY: the plug-in's buffer holds `room` units and `index` stays below it (at most
            // `copied`, which is less than `room`).
            unsafe { string.wrapping_add(index).write(*unit) };
        }
        kResultOk
    }

    unsafe fn setBinary(&self, id: AttrID, data: *const c_void, size_in_bytes: uint32) -> tresult {
        let size = size_in_bytes as usize;
        if size > MAX_VALUE || (data.is_null() && size > 0) {
            return kInvalidArgument;
        }
        let bytes = if size == 0 {
            Vec::new()
        } else {
            // SAFETY: the plug-in's buffer holds `size` bytes for the call; they are copied.
            unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) }.to_vec()
        };
        self.set(id, Value::Binary(bytes))
    }

    unsafe fn getBinary(&self, id: AttrID, data: *mut *const c_void, size_in_bytes: *mut uint32) -> tresult {
        // The pointer handed out stays valid while the value is not replaced, as the SDK's own
        // host attribute list does: the bytes live in the list until it is set again or freed.
        let Some((pointer, size)) = self.with(id, |v| match v {
            Value::Binary(bytes) => Some((bytes.as_ptr().cast::<c_void>(), u32::try_from(bytes.len()).ok()?)),
            _ => None,
        }) else {
            return kResultFalse;
        };
        // SAFETY: the plug-in passes pointers to its own pointer and size to fill, or null.
        let (Some(data), Some(size_in_bytes)) = (unsafe { data.as_mut() }, unsafe { size_in_bytes.as_mut() }) else {
            return kInvalidArgument;
        };
        *data = pointer;
        *size_in_bytes = size;
        kResultOk
    }
}

/// A message: an id and an attribute list.
pub(crate) struct Message {
    id: Mutex<Option<CString>>,
    attributes: ComWrapper<AttributeList>,
}

impl Class for Message {
    type Interfaces = (IMessage,);
}

impl Message {
    fn new() -> Self {
        Self { id: Mutex::new(None), attributes: ComWrapper::new(AttributeList::default()) }
    }
}

impl IMessageTrait for Message {
    unsafe fn getMessageID(&self) -> FIDString {
        // The text lives in the message until the id is set again, as the SDK's host message's.
        let id = self.id.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        id.as_ref().map_or(std::ptr::null(), |id| id.as_ptr())
    }

    unsafe fn setMessageID(&self, id: FIDString) {
        let value = if id.is_null() {
            None
        } else {
            // SAFETY: a non-null id is a zero-terminated string the plug-in owns for the call.
            Some(unsafe { CStr::from_ptr(id) }.to_owned())
        };
        *self.id.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = value;
    }

    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        // Not counted for the caller: the list belongs to the message, as in the SDK.
        self.attributes.to_com_ptr::<IAttributeList>().map_or(std::ptr::null_mut(), |list| list.as_ptr())
    }
}

/// Whether a class or interface id the plug-in passed is `I`'s.
fn is<I: Interface>(id: *const TUID) -> bool {
    // SAFETY: the plug-in passes pointers to 16-byte ids, or null.
    let Some(id) = (unsafe { id.as_ref() }) else { return false };
    id.iter().zip(I::IID.iter()).all(|(a, b)| *a as u8 == *b)
}

/// What the host makes for `IHostApplication::createInstance`: a message or an attribute list,
/// with one reference for the caller; null for anything else.
pub(crate) fn create(cid: *const TUID, iid: *const TUID) -> *mut c_void {
    if is::<IMessage>(cid) && is::<IMessage>(iid) {
        return ComWrapper::new(Message::new())
            .to_com_ptr::<IMessage>()
            .map_or(std::ptr::null_mut(), |m| m.into_raw().cast());
    }
    if is::<IAttributeList>(cid) && is::<IAttributeList>(iid) {
        return ComWrapper::new(AttributeList::default())
            .to_com_ptr::<IAttributeList>()
            .map_or(std::ptr::null_mut(), |l| l.into_raw().cast());
    }
    std::ptr::null_mut()
}

#[cfg(test)]
mod tests {
    use std::ffi::c_char;

    use super::*;
    use vst3::ComPtr;

    fn name(text: &str) -> CString {
        CString::new(text).unwrap_or_default()
    }

    #[test]
    fn a_message_keeps_its_id_and_attributes() {
        let cid: TUID = IMessage::IID.map(|b| b as c_char);
        let made = create(&cid, &cid);
        assert!(!made.is_null());
        // SAFETY: the pointer `create` just returned, with its one reference taken over here.
        let message = unsafe { ComPtr::<IMessage>::from_raw(made.cast()) };
        let Some(message) = message else { panic!("no message") };
        let id = name("parameters");
        // SAFETY: a live message and a zero-terminated id.
        unsafe { message.setMessageID(id.as_ptr()) };
        // SAFETY: as above.
        let back = unsafe { message.getMessageID() };
        // SAFETY: the id the message keeps, zero-terminated, alive while it is not set again.
        let back = unsafe { CStr::from_ptr(back) };
        assert_eq!(back, id.as_c_str());
        // SAFETY: a live message.
        let list = unsafe { message.getAttributes() };
        // SAFETY: the message's own list, alive while the message is.
        let Some(list) = (unsafe { vst3::ComRef::<IAttributeList>::from_raw(list) }) else { panic!("no list") };
        let (int, bytes, text) = (name("count"), name("blob"), name("label"));
        // SAFETY: a live list and zero-terminated names; values the list copies.
        unsafe { list.setInt(int.as_ptr(), -7) };
        let data = [1u8, 2, 3];
        // SAFETY: as above, three bytes.
        unsafe { list.setBinary(bytes.as_ptr(), data.as_ptr().cast(), 3) };
        let label: Vec<TChar> = "Gain".encode_utf16().map(|u| u as TChar).chain([0]).collect();
        // SAFETY: as above, a zero-terminated string.
        unsafe { list.setString(text.as_ptr(), label.as_ptr()) };
        let mut value = 0;
        // SAFETY: a live list and a value to fill.
        assert_eq!(unsafe { list.getInt(int.as_ptr(), &mut value) }, kResultOk);
        assert_eq!(value, -7);
        let (mut pointer, mut size) = (std::ptr::null(), 0u32);
        // SAFETY: a live list and a pointer and size to fill.
        assert_eq!(unsafe { list.getBinary(bytes.as_ptr(), &mut pointer, &mut size) }, kResultOk);
        // SAFETY: the list's own bytes, `size` long, alive while it is.
        assert_eq!(unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), size as usize) }, data);
        let mut out = [0 as TChar; 3];
        // SAFETY: a live list and a buffer of 3 units (6 bytes): the text is cut to fit.
        assert_eq!(unsafe { list.getString(text.as_ptr(), out.as_mut_ptr(), 6) }, kResultOk);
        assert_eq!(out, ['G' as TChar, 'a' as TChar, 0]);
        // Nothing under another name or of another kind.
        // SAFETY: as above.
        assert_eq!(unsafe { list.getFloat(int.as_ptr(), &mut 0.0) }, kResultFalse);
        assert!(create(&cid, &[0; 16]).is_null());
    }
}
