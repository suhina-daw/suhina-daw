// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The host side of ARA ([ADR 0028]): a document controller for one VST3 instance, built from
//! the [`AraDocument`] the engine sends with `load`, with the host's interfaces the plug-in calls
//! back (audio access, archiving, content access, model updates, playback control).
//!
//! The document controller is made and edited on this process's main thread, ARA's model
//! thread. The plug-in reads audio on threads of its own through [`read_samples`], from the
//! sources' shared memory ([`suhina_ipc::AudioSource`]), which never changes once mapped.
//!
//! The three VST3 interfaces of `ARAVST3.h` (`IMainFactory`, `IPlugInEntryPoint`,
//! `IPlugInEntryPoint2`) are declared here by hand; the C API comes from [`crate::ara_sys`].
//!
//! [ADR 0028]: https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0028-ara-hosting.md

use std::ffi::{CString, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use suhina_ipc::AudioSource;
use suhina_plugins::protocol::AraDocument;
use vst3::ComPtr;
use vst3::Steinberg::Vst::IComponent;
use vst3::Steinberg::{FUnknown, FUnknownVtbl, TUID, kResultOk};

use crate::ara_sys::*;

/// `ARA::IPlugInEntryPoint` (`ARAVST3.h`).
const ENTRY_POINT_IID: TUID = vst3::uid(0x1281_4E54, 0xA1CE_4076, 0x82B9_6813, 0x1695_0BD6);
/// `ARA::IPlugInEntryPoint2` (`ARAVST3.h`, ARA 2.0).
const ENTRY_POINT2_IID: TUID = vst3::uid(0xCD9A_5913, 0xC9EB_46D7, 0x96CA_53AD, 0xD1DB_89F5);

#[repr(C)]
struct EntryPointVtbl {
    base: FUnknownVtbl,
    get_factory: unsafe extern "system" fn(this: *mut c_void) -> *const ARAFactory,
    bind_to_document_controller: unsafe extern "system" fn(
        this: *mut c_void,
        controller: ARADocumentControllerRef,
    ) -> *const ARAPlugInExtensionInstance,
}

#[repr(C)]
struct EntryPoint2Vtbl {
    base: FUnknownVtbl,
    bind_to_document_controller_with_roles: unsafe extern "system" fn(
        this: *mut c_void,
        controller: ARADocumentControllerRef,
        known: ARAPlugInInstanceRoleFlags,
        assigned: ARAPlugInInstanceRoleFlags,
    ) -> *const ARAPlugInExtensionInstance,
}

/// A COM interface the component handed out; released when dropped.
struct Interface {
    object: *mut c_void,
}

impl Interface {
    /// Asks `component` for the interface `iid`.
    fn query(component: &ComPtr<IComponent>, iid: &TUID) -> Option<Self> {
        let unknown = component.as_ptr().cast::<FUnknown>();
        let mut object: *mut c_void = std::ptr::null_mut();
        // SAFETY: `unknown` is a live COM object (the component), whose first field is its
        // virtual table; queryInterface fills `object` with an owned reference or leaves it null.
        let table = unsafe { (*unknown).vtbl };
        // SAFETY: as above, the table it points to.
        let vtbl = unsafe { &*table };
        // SAFETY: the component's own table, called on it; `object` lives for the call.
        let result = unsafe { (vtbl.queryInterface)(unknown, iid.as_ptr().cast_mut().cast(), &mut object) };
        // Made only when it was handed: a dropped `Interface` releases its object.
        (result == kResultOk && !object.is_null()).then(|| Self { object })
    }

    /// The object's virtual table, as the interface `V` it was asked for.
    fn vtbl<V>(&self) -> *const V {
        // SAFETY: a COM object's first field is its virtual table pointer; the caller names the
        // table of the interface this object was queried as.
        unsafe { *self.object.cast::<*const V>() }
    }
}

impl Drop for Interface {
    fn drop(&mut self) {
        // SAFETY: the table of the object `query` was handed.
        let vtbl = unsafe { &*self.vtbl::<FUnknownVtbl>() };
        // SAFETY: the reference `query` was handed, released once.
        let _ = unsafe { (vtbl.release)(self.object.cast()) };
    }
}

/// The factories this process has initialised, by address: ARA asks for one
/// `initializeARAWithConfiguration` per factory and process, before any document.
static INITIALISED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// What the plug-in reads one source from.
struct Source {
    audio: AudioSource,
}

/// A reader the plug-in made for a source.
struct Reader {
    source: *const Source,
    /// It asked for 64-bit samples.
    wide: bool,
}

/// A document's archive, read or written: the plug-in's edits, kept in the project with its
/// state ([`wrap_state`]).
struct Archive {
    bytes: Vec<u8>,
    id: CString,
}

/// Most bytes an archive the plug-in writes may grow to.
const MAX_ARCHIVE: usize = 512 * 1024 * 1024;
/// Most compatible archive ids read from a factory.
const MAX_COMPATIBLE_IDS: usize = 64;
/// Longest text read from the plug-in (an archive id).
const MAX_TEXT: usize = 1024;

/// A C text the plug-in hands, up to [`MAX_TEXT`] bytes; `None` for null or not UTF-8.
fn text_of(text: *const std::ffi::c_char) -> Option<String> {
    if text.is_null() {
        return None;
    }
    let mut bytes = Vec::new();
    for index in 0..MAX_TEXT {
        // SAFETY: a NUL-terminated text of the plug-in's; reading stops at its NUL.
        let byte = unsafe { text.wrapping_add(index).read() } as u8;
        if byte == 0 {
            return String::from_utf8(bytes).ok();
        }
        bytes.push(byte);
    }
    None
}

/// What starts a VST3 state that carries the plug-in's ARA edits as well.
const STATE_MAGIC: &[u8; 16] = b"SUHINA-ARA-STATE";

/// The state a project keeps for a plug-in bound to an ARA document: the component's own
/// state and the document archive (its id and bytes), in one blob. Without an archive it is the
/// component's state as it is.
pub(crate) fn wrap_state(component: Vec<u8>, archive: Option<(String, Vec<u8>)>) -> Vec<u8> {
    let Some((id, bytes)) = archive else { return component };
    let mut out = Vec::with_capacity(component.len() + bytes.len() + id.len() + 40);
    out.extend_from_slice(STATE_MAGIC);
    out.extend_from_slice(&1u32.to_le_bytes());
    for part in [component.as_slice(), id.as_bytes(), bytes.as_slice()] {
        out.extend_from_slice(&(part.len() as u64).to_le_bytes());
        out.extend_from_slice(part);
    }
    out
}

/// Splits a state [`wrap_state`] made into the component's state and the archive; any other
/// state is the component's alone. A blob that starts like one but does not hold together is
/// taken as the component's, untouched.
pub(crate) fn unwrap_state(data: &[u8]) -> (&[u8], Option<(String, &[u8])>) {
    type Parts<'a> = (&'a [u8], Option<(String, &'a [u8])>);
    let parts = || -> Option<Parts<'_>> {
        let rest = data.strip_prefix(STATE_MAGIC.as_slice())?;
        let (version, mut rest) = rest.split_at_checked(4)?;
        if version != 1u32.to_le_bytes() {
            return None;
        }
        let mut take = || -> Option<&[u8]> {
            let (len, after) = rest.split_at_checked(8)?;
            let len = usize::try_from(u64::from_le_bytes(len.try_into().ok()?)).ok()?;
            let (part, after) = after.split_at_checked(len)?;
            rest = after;
            Some(part)
        };
        let (component, id, bytes) = (take()?, take()?, take()?);
        let id = String::from_utf8(id.to_vec()).ok()?;
        Some((component, Some((id, bytes))))
    };
    parts().unwrap_or((data, None))
}

/// The host's half of the document: what the plug-in's callbacks reach through their host refs.
/// Boxed, so its address (handed to the plug-in) stays put.
struct Host {
    instance: ARADocumentControllerHostInstance,
    sources: Vec<Source>,
    /// The plug-in said its document data changed (ARA 2.3), so the project has unsaved edits.
    changed: AtomicBool,
}

/// An ARA document controller and the objects made in it, bound to one plug-in instance.
pub(crate) struct Document {
    controller: *const ARADocumentControllerInstance,
    host: Box<Host>,
    context: ARAMusicalContextRef,
    sequence: ARARegionSequenceRef,
    sources: Vec<ARAAudioSourceRef>,
    modifications: Vec<ARAAudioModificationRef>,
    regions: Vec<ARAPlaybackRegionRef>,
    /// The persistent id of each region, in the same order (`clip-<id>`).
    region_ids: Vec<String>,
    /// The plug-in's editor view, told which region the host has selected; `None` when the
    /// plug-in has none.
    editor_view: Option<(ARAEditorViewRef, *const ARAEditorViewInterface)>,
    /// The plug-in's factory, valid while its module is loaded.
    factory: *const ARAFactory,
    /// The texts handed to the plug-in, kept for as long as the document.
    _texts: Vec<CString>,
    _entry: Interface,
}

/// The document controller's interface and reference. The interface is a copy of the
/// plug-in's table cut to the size it says it has, so an older plug-in's shorter table reads
/// as functions it does not have.
struct Controller {
    interface: ARADocumentControllerInterface,
    reference: ARADocumentControllerRef,
}

impl Document {
    /// Binds `component` (a VST3 plug-in's component, initialised, not yet active) to a new
    /// document built from `document`, in every role: playback renderer, editor renderer and
    /// editor view. Fails when the plug-in does not support ARA 2, or an object cannot be made.
    pub(crate) fn bind(component: &ComPtr<IComponent>, document: &AraDocument) -> Result<Self, String> {
        let entry = Interface::query(component, &ENTRY_POINT_IID).ok_or("the plug-in does not support ARA")?;
        // SAFETY: the table of the object queried as the entry point.
        let entry_vtbl = unsafe { &*entry.vtbl::<EntryPointVtbl>() };
        // SAFETY: the entry point's own table, called on the object it belongs to.
        let factory = unsafe { (entry_vtbl.get_factory)(entry.object) };
        // SAFETY: the plug-in hands its factory, valid while its module is loaded, or null.
        let factory = unsafe { factory.as_ref() }.ok_or("the plug-in has no ARA factory")?;
        let wanted = kARAAPIGeneration_2_0_Final as ARAAPIGeneration;
        if factory.lowestSupportedApiGeneration > wanted || factory.highestSupportedApiGeneration < wanted {
            return Err("the plug-in does not support ARA 2".into());
        }
        initialise(factory)?;

        let mut texts = Vec::new();
        let mut text = |value: &str| {
            let owned = CString::new(value.replace('\0', " ")).unwrap_or_default();
            let pointer = owned.as_ptr();
            texts.push(owned);
            pointer
        };
        let sources = document
            .sources
            .iter()
            .map(|s| AudioSource::open(&s.address).map(|audio| Source { audio }))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("an audio source could not be opened: {e}"))?;
        let mut host = Box::new(Host {
            instance: ARADocumentControllerHostInstance {
                structSize: size_of::<ARADocumentControllerHostInstance>(),
                audioAccessControllerHostRef: std::ptr::null_mut(),
                audioAccessControllerInterface: &AUDIO_ACCESS,
                archivingControllerHostRef: std::ptr::null_mut(),
                archivingControllerInterface: &ARCHIVING,
                contentAccessControllerHostRef: std::ptr::null_mut(),
                contentAccessControllerInterface: &CONTENT_ACCESS,
                modelUpdateControllerHostRef: std::ptr::null_mut(),
                modelUpdateControllerInterface: &MODEL_UPDATE,
                playbackControllerHostRef: std::ptr::null_mut(),
                playbackControllerInterface: &PLAYBACK,
            },
            sources,
            changed: AtomicBool::new(false),
        });
        let host_ref: *mut c_void = std::ptr::from_mut::<Host>(&mut host).cast();
        host.instance.audioAccessControllerHostRef = host_ref.cast();
        host.instance.archivingControllerHostRef = host_ref.cast();
        host.instance.contentAccessControllerHostRef = host_ref.cast();
        host.instance.modelUpdateControllerHostRef = host_ref.cast();
        host.instance.playbackControllerHostRef = host_ref.cast();

        let properties =
            ARADocumentProperties { structSize: size_of::<ARADocumentProperties>(), name: text(&document.name) };
        let create = factory.createDocumentControllerWithDocument.ok_or("the ARA factory makes no documents")?;
        // SAFETY: the host instance and its tables outlive the document controller (both are
        // owned by the `Document` this returns); the properties live for the call.
        let controller = unsafe { create(&host.instance, &properties) };
        let mut made = Self {
            controller,
            host,
            context: std::ptr::null_mut(),
            sequence: std::ptr::null_mut(),
            sources: Vec::new(),
            modifications: Vec::new(),
            regions: Vec::new(),
            region_ids: Vec::new(),
            editor_view: None,
            factory,
            _texts: Vec::new(),
            _entry: entry,
        };
        let dc = made.controller().ok_or("the plug-in made no document controller")?;
        let (Some(begin), Some(end)) = (dc.interface.beginEditing, dc.interface.endEditing) else {
            return Err("the document controller cannot be edited".into());
        };
        // SAFETY: a live document controller, on its model thread (this one).
        unsafe { begin(dc.reference) };
        let built = made.build(&dc, document, &mut text);
        // SAFETY: as above, closing the edit `begin` opened.
        unsafe { end(dc.reference) };
        built?;
        made._texts = texts;

        // Every role, through the newer entry point when the plug-in has it.
        let roles =
            (kARAPlaybackRendererRole | kARAEditorRendererRole | kARAEditorViewRole) as ARAPlugInInstanceRoleFlags;
        let extension = match Interface::query(component, &ENTRY_POINT2_IID) {
            Some(entry2) => {
                // SAFETY: the table of the object queried as the second entry point.
                let vtbl = unsafe { &*entry2.vtbl::<EntryPoint2Vtbl>() };
                // SAFETY: the second entry point's own table, on its object; the reference is
                // the live document controller's.
                unsafe { (vtbl.bind_to_document_controller_with_roles)(entry2.object, dc.reference, roles, roles) }
            }
            // SAFETY: as above, through the first entry point (ARA 1's binding, every role).
            None => unsafe { (entry_vtbl.bind_to_document_controller)(made._entry.object, dc.reference) },
        };
        // SAFETY: the plug-in hands its extension instance, valid while it lives, or null.
        let extension = unsafe { extension.as_ref() }.ok_or("the plug-in did not bind to the document")?;
        made.editor_view = (!extension.editorViewRef.is_null() && !extension.editorViewInterface.is_null())
            .then_some((extension.editorViewRef, extension.editorViewInterface));
        // SAFETY: the renderers' tables, valid with the extension instance, or null.
        let playback = unsafe { extension.playbackRendererInterface.as_ref() };
        // SAFETY: as above.
        let editor = unsafe { extension.editorRendererInterface.as_ref() };
        for &region in &made.regions {
            if let Some(add) = playback.and_then(|p| p.addPlaybackRegion) {
                // SAFETY: a live renderer and a region of its document, on the model thread.
                unsafe { add(extension.playbackRendererRef, region) };
            }
            if let Some(add) = editor.and_then(|e| e.addPlaybackRegion) {
                // SAFETY: as above, for the editor renderer.
                unsafe { add(extension.editorRendererRef, region) };
            }
        }
        Ok(made)
    }

    /// Tells the plug-in's editor view which region the host has selected (`clip-<id>`), so its
    /// editor shows it; `None`, or an id the document does not hold, selects nothing. Returns
    /// whether the plug-in was told.
    pub(crate) fn select(&self, region: Option<&str>) -> bool {
        let Some((view, interface)) = self.editor_view else { return false };
        // SAFETY: the plug-in's editor view table, valid with its extension instance.
        let interface = unsafe { &*interface };
        let Some(notify) = interface.notifySelection else { return false };
        let selected: Vec<ARAPlaybackRegionRef> = region
            .and_then(|id| self.region_ids.iter().position(|r| r == id))
            .and_then(|index| self.regions.get(index).copied())
            .into_iter()
            .collect();
        let sequences = if selected.is_empty() { Vec::new() } else { vec![self.sequence] };
        let selection = ARAViewSelection {
            structSize: size_of::<ARAViewSelection>(),
            playbackRegionRefsCount: selected.len(),
            playbackRegionRefs: selected.as_ptr(),
            regionSequenceRefsCount: sequences.len(),
            regionSequenceRefs: sequences.as_ptr(),
            timeRange: std::ptr::null(),
        };
        // SAFETY: the plug-in's editor view, on the model thread; the selection and the lists it
        // points to live for the call.
        unsafe { notify(view, &selection) };
        true
    }

    /// Whether the plug-in said its edits changed since this was last asked (ARA 2.3).
    #[allow(dead_code, reason = "read once the project shows unsaved edits made in the plug-in (#593)")]
    pub(crate) fn take_changed(&self) -> bool {
        self.host.changed.swap(false, Ordering::AcqRel)
    }

    /// The plug-in's edits as an archive: its document archive id and the bytes. `None` when it
    /// cannot store them.
    pub(crate) fn store(&self) -> Option<(String, Vec<u8>)> {
        let dc = self.controller()?;
        // SAFETY: the factory the document was made with, valid while its module is loaded.
        let factory = unsafe { self.factory.as_ref() }?;
        let id = text_of(factory.documentArchiveID)?;
        let store = dc.interface.storeObjectsToArchive?;
        let mut archive = Archive { bytes: Vec::new(), id: CString::new(id.clone()).ok()? };
        let writer: ARAArchiveWriterHostRef = std::ptr::from_mut(&mut archive).cast();
        // SAFETY: a live document controller, not being edited, on its model thread; the
        // archive lives for the call. No filter stores the whole document.
        let stored = unsafe { store(dc.reference, writer, std::ptr::null()) };
        (stored != 0).then_some((id, archive.bytes))
    }

    /// Gives the plug-in back the edits [`Document::store`] took, for the objects with the same
    /// persistent ids (the same clips). Refused when the archive is of a kind the plug-in does
    /// not read (another plug-in's, or a newer version's).
    pub(crate) fn restore(&self, id: &str, bytes: &[u8]) -> Result<(), String> {
        let dc = self.controller().ok_or("no document controller")?;
        // SAFETY: as in `store`.
        let factory = unsafe { self.factory.as_ref() }.ok_or("no ARA factory")?;
        let mut known = vec![text_of(factory.documentArchiveID)];
        let count = factory.compatibleDocumentArchiveIDsCount.min(MAX_COMPATIBLE_IDS);
        if !factory.compatibleDocumentArchiveIDs.is_null() {
            for index in 0..count {
                // SAFETY: the factory's list holds `compatibleDocumentArchiveIDsCount` ids.
                known.push(text_of(unsafe { factory.compatibleDocumentArchiveIDs.wrapping_add(index).read() }));
            }
        }
        if !known.iter().flatten().any(|k| k == id) {
            return Err("the plug-in does not read these saved edits".into());
        }
        let (Some(begin), Some(end), Some(restore)) =
            (dc.interface.beginEditing, dc.interface.endEditing, dc.interface.restoreObjectsFromArchive)
        else {
            return Err("the plug-in cannot restore edits".into());
        };
        let mut archive = Archive { bytes: bytes.to_vec(), id: CString::new(id).map_err(|_| "a bad archive id")? };
        let reader: ARAArchiveReaderHostRef = std::ptr::from_mut(&mut archive).cast();
        // SAFETY: a live document controller on its model thread, inside an edit; the archive
        // lives for the call. No filter restores every object whose id matches.
        unsafe { begin(dc.reference) };
        // SAFETY: as above.
        let restored = unsafe { restore(dc.reference, reader, std::ptr::null()) };
        // SAFETY: as above, closing the edit.
        unsafe { end(dc.reference) };
        if restored == 0 { Err("the plug-in could not read its saved edits".into()) } else { Ok(()) }
    }

    fn controller(&self) -> Option<Controller> {
        // SAFETY: the plug-in's controller instance, valid until it is destroyed in `drop`, or
        // null.
        let instance = unsafe { self.controller.as_ref() }?;
        let table = instance.documentControllerInterface;
        if table.is_null() {
            return None;
        }
        // SAFETY: a table of null function pointers and a zero size is a valid value.
        let mut interface: ARADocumentControllerInterface = unsafe { std::mem::zeroed() };
        // SAFETY: the plug-in's table, valid as long as the instance; its first field is its
        // size, read unaligned as the struct is packed.
        let size = unsafe { table.cast::<usize>().read_unaligned() };
        let size = size.min(size_of::<ARADocumentControllerInterface>());
        // SAFETY: `size` bytes of the plug-in's table, no more than either struct holds.
        unsafe {
            std::ptr::copy_nonoverlapping(table.cast::<u8>(), std::ptr::from_mut(&mut interface).cast::<u8>(), size)
        };
        Some(Controller { interface, reference: instance.documentControllerRef })
    }

    /// Makes the musical context, the region sequence, the sources, their modifications and
    /// the playback regions, inside the edit `bind` opened.
    fn build(
        &mut self,
        dc: &Controller,
        document: &AraDocument,
        text: &mut impl FnMut(&str) -> *const std::ffi::c_char,
    ) -> Result<(), String> {
        let host_ref: *mut c_void = std::ptr::from_mut::<Host>(&mut self.host).cast();
        let i = &dc.interface;
        let missing = |what: &str| format!("the document controller cannot make {what}");
        let make_context = i.createMusicalContext.ok_or_else(|| missing("a musical context"))?;
        let context = ARAMusicalContextProperties {
            structSize: size_of::<ARAMusicalContextProperties>(),
            name: text(&document.name),
            orderIndex: 0,
            color: std::ptr::null(),
        };
        // SAFETY: a live controller inside an edit, on its model thread; the properties live
        // for the call, the host ref for the document.
        self.context = unsafe { make_context(dc.reference, host_ref.cast(), &context) };
        let make_sequence = i.createRegionSequence.ok_or_else(|| missing("a region sequence"))?;
        let sequence = ARARegionSequenceProperties {
            structSize: size_of::<ARARegionSequenceProperties>(),
            name: text(&document.sequence),
            orderIndex: 0,
            musicalContextRef: self.context,
            color: std::ptr::null(),
        };
        // SAFETY: as above.
        self.sequence = unsafe { make_sequence(dc.reference, host_ref.cast(), &sequence) };
        let make_source = i.createAudioSource.ok_or_else(|| missing("an audio source"))?;
        for (index, described) in document.sources.iter().enumerate() {
            let Some(source) = self.host.sources.get(index) else { break };
            // Not moved again: the list is not changed once the document is built.
            let source_ref: *const Source = source;
            let properties = ARAAudioSourceProperties {
                structSize: size_of::<ARAAudioSourceProperties>(),
                name: text(&described.name),
                persistentID: text(&described.id),
                sampleCount: i64::try_from(source.audio.frames()).unwrap_or(i64::MAX),
                sampleRate: source.audio.sample_rate(),
                channelCount: i32::try_from(source.audio.channels()).unwrap_or(2),
                merits64BitSamples: 0,
                channelArrangementDataType: kARAChannelArrangementUndefined as ARAChannelArrangementDataType,
                channelArrangement: std::ptr::null(),
            };
            // SAFETY: as above; the source's host ref is its boxed `Source`, which lives as
            // long as the document.
            let made = unsafe { make_source(dc.reference, source_ref.cast_mut().cast(), &properties) };
            if made.is_null() {
                return Err(format!("the plug-in refused the audio source {:?}", described.name));
            }
            if let Some(enable) = i.enableAudioSourceSamplesAccess {
                // SAFETY: as above, for a source just made.
                unsafe { enable(dc.reference, made, 1) };
            }
            self.sources.push(made);
        }
        let make_modification = i.createAudioModification.ok_or_else(|| missing("an audio modification"))?;
        let make_region = i.createPlaybackRegion.ok_or_else(|| missing("a playback region"))?;
        let mut by_id: Vec<(&str, ARAAudioModificationRef)> = Vec::new();
        for region in &document.regions {
            let Some(&source) = self.sources.get(region.source) else { continue };
            let modification = match by_id.iter().find(|(id, _)| *id == region.modification) {
                Some(&(_, made)) => made,
                None => {
                    let properties = ARAAudioModificationProperties {
                        structSize: size_of::<ARAAudioModificationProperties>(),
                        name: text(&region.name),
                        persistentID: text(&region.modification),
                    };
                    // SAFETY: as above, for a source of this document.
                    let made = unsafe { make_modification(dc.reference, source, host_ref.cast(), &properties) };
                    if made.is_null() {
                        return Err(format!("the plug-in refused the audio modification of {:?}", region.name));
                    }
                    self.modifications.push(made);
                    by_id.push((&region.modification, made));
                    made
                }
            };
            let properties = ARAPlaybackRegionProperties {
                structSize: size_of::<ARAPlaybackRegionProperties>(),
                transformationFlags: 0,
                startInModificationTime: region.offset,
                durationInModificationTime: region.duration,
                startInPlaybackTime: region.start,
                durationInPlaybackTime: region.duration,
                musicalContextRef: self.context,
                regionSequenceRef: self.sequence,
                name: text(&region.name),
                color: std::ptr::null(),
            };
            // SAFETY: as above, for a modification of this document.
            let made = unsafe { make_region(dc.reference, modification, host_ref.cast(), &properties) };
            if made.is_null() {
                return Err(format!("the plug-in refused the playback region {:?}", region.name));
            }
            self.regions.push(made);
            self.region_ids.push(region.id.clone());
        }
        Ok(())
    }
}

impl Drop for Document {
    /// Destroys the document's objects (regions, modifications, sources, sequence, context) in
    /// one edit, then the document controller, as ARA requires.
    fn drop(&mut self) {
        let Some(dc) = self.controller() else { return };
        let i = &dc.interface;
        if let (Some(begin), Some(end)) = (i.beginEditing, i.endEditing) {
            // SAFETY: a live document controller, on its model thread.
            unsafe { begin(dc.reference) };
            for &region in &self.regions {
                if let Some(destroy) = i.destroyPlaybackRegion {
                    // SAFETY: a region made in this document, destroyed once.
                    unsafe { destroy(dc.reference, region) };
                }
            }
            for &modification in &self.modifications {
                if let Some(destroy) = i.destroyAudioModification {
                    // SAFETY: as above, for a modification.
                    unsafe { destroy(dc.reference, modification) };
                }
            }
            for &source in &self.sources {
                if let Some(enable) = i.enableAudioSourceSamplesAccess {
                    // SAFETY: as above, for a source; its reading stops before it goes.
                    unsafe { enable(dc.reference, source, 0) };
                }
                if let Some(destroy) = i.destroyAudioSource {
                    // SAFETY: as above.
                    unsafe { destroy(dc.reference, source) };
                }
            }
            if !self.sequence.is_null()
                && let Some(destroy) = i.destroyRegionSequence
            {
                // SAFETY: as above, for the sequence.
                unsafe { destroy(dc.reference, self.sequence) };
            }
            if !self.context.is_null()
                && let Some(destroy) = i.destroyMusicalContext
            {
                // SAFETY: as above, for the context.
                unsafe { destroy(dc.reference, self.context) };
            }
            // SAFETY: closing the edit opened above.
            unsafe { end(dc.reference) };
        }
        if let Some(destroy) = i.destroyDocumentController {
            // SAFETY: the controller, destroyed once, after every object of its document.
            unsafe { destroy(dc.reference) };
        }
    }
}

/// Initialises `factory` for this process, once.
fn initialise(factory: &ARAFactory) -> Result<(), String> {
    let key = std::ptr::from_ref(factory) as usize;
    let mut done = INITIALISED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if done.contains(&key) {
        return Ok(());
    }
    let init = factory.initializeARAWithConfiguration.ok_or("the ARA factory cannot be initialised")?;
    let desired = factory.highestSupportedApiGeneration.min(kARAAPIGeneration_2_3_Final as ARAAPIGeneration);
    let config = ARAInterfaceConfiguration {
        structSize: size_of::<ARAInterfaceConfiguration>(),
        desiredApiGeneration: desired,
        assertFunctionAddress: std::ptr::null_mut(),
    };
    // SAFETY: the factory's own function, called once per process with a configuration that
    // lives for the call.
    unsafe { init(&config) };
    done.push(key);
    Ok(())
}

// ---- What the plug-in calls back ---------------------------------------------------------------

static AUDIO_ACCESS: ARAAudioAccessControllerInterface = ARAAudioAccessControllerInterface {
    structSize: size_of::<ARAAudioAccessControllerInterface>(),
    createAudioReaderForSource: Some(create_reader),
    readAudioSamples: Some(read_samples),
    destroyAudioReader: Some(destroy_reader),
};

unsafe extern "C" fn create_reader(
    _host: ARAAudioAccessControllerHostRef,
    source: ARAAudioSourceHostRef,
    wide: ARABool,
) -> ARAAudioReaderHostRef {
    let reader = Box::new(Reader { source: source.cast::<Source>().cast_const(), wide: wide != 0 });
    Box::into_raw(reader).cast()
}

/// Samples read into a stack buffer at a time, so reading never allocates.
const CHUNK: usize = 4096;

/// ARA's `readAudioSamples`: fills each channel's buffer with `count` samples from `position`,
/// silence outside the source. Called on the plug-in's own (not real-time) threads.
unsafe extern "C" fn read_samples(
    _host: ARAAudioAccessControllerHostRef,
    reader: ARAAudioReaderHostRef,
    position: ARASamplePosition,
    count: ARASampleCount,
    buffers: *const *mut c_void,
) -> ARABool {
    // SAFETY: a reader `create_reader` made and the plug-in has not destroyed.
    let Some(reader) = (unsafe { reader.cast::<Reader>().as_ref() }) else { return 0 };
    // SAFETY: the reader's source lives as long as the document, which outlives its readers.
    let Some(source) = (unsafe { reader.source.as_ref() }) else { return 0 };
    let Ok(count) = usize::try_from(count) else { return 0 };
    if buffers.is_null() || count > isize::MAX as usize / 8 {
        return 0;
    }
    let mut chunk = [0.0f32; CHUNK];
    for channel in 0..source.audio.channels() as usize {
        // SAFETY: the plug-in passes one buffer per channel of the source.
        let buffer = unsafe { buffers.wrapping_add(channel).read() };
        if buffer.is_null() {
            return 0;
        }
        let mut done = 0;
        while done < count {
            let len = (count - done).min(CHUNK);
            let at = position.saturating_add(done as i64);
            source.audio.read(channel as u32, at, &mut chunk[..len]);
            if reader.wide {
                // SAFETY: the plug-in's buffer holds `count` doubles (it asked for 64-bit
                // samples); this writes `len` of them from `done`.
                let out = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<f64>().wrapping_add(done), len) };
                for (o, v) in out.iter_mut().zip(&chunk[..len]) {
                    *o = f64::from(*v);
                }
            } else {
                // SAFETY: as above, for floats.
                let out = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<f32>().wrapping_add(done), len) };
                out.copy_from_slice(&chunk[..len]);
            }
            done += len;
        }
    }
    1
}

unsafe extern "C" fn destroy_reader(_host: ARAAudioAccessControllerHostRef, reader: ARAAudioReaderHostRef) {
    if !reader.is_null() {
        // SAFETY: a reader `create_reader` made, destroyed once by the plug-in.
        drop(unsafe { Box::from_raw(reader.cast::<Reader>()) });
    }
}

static ARCHIVING: ARAArchivingControllerInterface = ARAArchivingControllerInterface {
    structSize: size_of::<ARAArchivingControllerInterface>(),
    getArchiveSize: Some(archive_size),
    readBytesFromArchive: Some(read_archive),
    writeBytesToArchive: Some(write_archive),
    notifyDocumentArchivingProgress: Some(progress),
    notifyDocumentUnarchivingProgress: Some(progress),
    getDocumentArchiveID: Some(archive_id),
};

unsafe extern "C" fn archive_size(_host: ARAArchivingControllerHostRef, reader: ARAArchiveReaderHostRef) -> ARASize {
    // SAFETY: an archive the host handed for reading, live for the call that reads it.
    unsafe { reader.cast::<Archive>().as_ref() }.map_or(0, |a| a.bytes.len())
}

unsafe extern "C" fn read_archive(
    _host: ARAArchivingControllerHostRef,
    reader: ARAArchiveReaderHostRef,
    position: ARASize,
    length: ARASize,
    buffer: *mut ARAByte,
) -> ARABool {
    // SAFETY: as in `archive_size`.
    let Some(archive) = (unsafe { reader.cast::<Archive>().as_ref() }) else { return 0 };
    let Some(bytes) = position.checked_add(length).and_then(|end| archive.bytes.get(position..end)) else { return 0 };
    if buffer.is_null() {
        return 0;
    }
    // SAFETY: the plug-in's buffer holds `length` bytes; the archive's are separate.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, length) };
    1
}

unsafe extern "C" fn write_archive(
    _host: ARAArchivingControllerHostRef,
    writer: ARAArchiveWriterHostRef,
    position: ARASize,
    length: ARASize,
    buffer: *const ARAByte,
) -> ARABool {
    // SAFETY: an archive the host handed for writing, live for the call that writes it.
    let Some(archive) = (unsafe { writer.cast::<Archive>().as_mut() }) else { return 0 };
    let Some(end) = position.checked_add(length).filter(|&end| end <= MAX_ARCHIVE) else { return 0 };
    if buffer.is_null() {
        return 0;
    }
    if archive.bytes.len() < end {
        archive.bytes.resize(end, 0);
    }
    // SAFETY: the plug-in's buffer holds `length` bytes; the archive now has room for them.
    unsafe { std::ptr::copy_nonoverlapping(buffer, archive.bytes.as_mut_ptr().wrapping_add(position), length) };
    1
}

unsafe extern "C" fn progress(_host: ARAArchivingControllerHostRef, _value: f32) {}

unsafe extern "C" fn archive_id(
    _host: ARAArchivingControllerHostRef,
    reader: ARAArchiveReaderHostRef,
) -> ARAPersistentID {
    // SAFETY: as in `archive_size`.
    unsafe { reader.cast::<Archive>().as_ref() }.map_or(std::ptr::null(), |a| a.id.as_ptr())
}

static CONTENT_ACCESS: ARAContentAccessControllerInterface = ARAContentAccessControllerInterface {
    structSize: size_of::<ARAContentAccessControllerInterface>(),
    isMusicalContextContentAvailable: Some(no_context_content),
    getMusicalContextContentGrade: Some(context_grade),
    createMusicalContextContentReader: Some(no_context_reader),
    isAudioSourceContentAvailable: Some(no_source_content),
    getAudioSourceContentGrade: Some(source_grade),
    createAudioSourceContentReader: Some(no_source_reader),
    getContentReaderEventCount: Some(no_events),
    getContentReaderDataForEvent: Some(no_event),
    destroyContentReader: Some(no_reader_to_destroy),
};

// Suhina describes no content (tempo, chords, notes) to the plug-in yet; it analyses its own.
unsafe extern "C" fn no_context_content(
    _host: ARAContentAccessControllerHostRef,
    _context: ARAMusicalContextHostRef,
    _kind: ARAContentType,
) -> ARABool {
    0
}

unsafe extern "C" fn context_grade(
    _host: ARAContentAccessControllerHostRef,
    _context: ARAMusicalContextHostRef,
    _kind: ARAContentType,
) -> ARAContentGrade {
    kARAContentGradeInitial as ARAContentGrade
}

unsafe extern "C" fn no_context_reader(
    _host: ARAContentAccessControllerHostRef,
    _context: ARAMusicalContextHostRef,
    _kind: ARAContentType,
    _range: *const ARAContentTimeRange,
) -> ARAContentReaderHostRef {
    std::ptr::null_mut()
}

unsafe extern "C" fn no_source_content(
    _host: ARAContentAccessControllerHostRef,
    _source: ARAAudioSourceHostRef,
    _kind: ARAContentType,
) -> ARABool {
    0
}

unsafe extern "C" fn source_grade(
    _host: ARAContentAccessControllerHostRef,
    _source: ARAAudioSourceHostRef,
    _kind: ARAContentType,
) -> ARAContentGrade {
    kARAContentGradeInitial as ARAContentGrade
}

unsafe extern "C" fn no_source_reader(
    _host: ARAContentAccessControllerHostRef,
    _source: ARAAudioSourceHostRef,
    _kind: ARAContentType,
    _range: *const ARAContentTimeRange,
) -> ARAContentReaderHostRef {
    std::ptr::null_mut()
}

unsafe extern "C" fn no_events(_host: ARAContentAccessControllerHostRef, _reader: ARAContentReaderHostRef) -> ARAInt32 {
    0
}

unsafe extern "C" fn no_event(
    _host: ARAContentAccessControllerHostRef,
    _reader: ARAContentReaderHostRef,
    _index: ARAInt32,
) -> *const c_void {
    std::ptr::null()
}

unsafe extern "C" fn no_reader_to_destroy(_host: ARAContentAccessControllerHostRef, _reader: ARAContentReaderHostRef) {}

static MODEL_UPDATE: ARAModelUpdateControllerInterface = ARAModelUpdateControllerInterface {
    structSize: size_of::<ARAModelUpdateControllerInterface>(),
    notifyAudioSourceAnalysisProgress: Some(analysis_progress),
    notifyAudioSourceContentChanged: Some(source_changed),
    notifyAudioModificationContentChanged: Some(modification_changed),
    notifyPlaybackRegionContentChanged: Some(region_changed),
    notifyDocumentDataChanged: Some(document_changed),
};

unsafe extern "C" fn analysis_progress(
    _host: ARAModelUpdateControllerHostRef,
    _source: ARAAudioSourceHostRef,
    _state: ARAAnalysisProgressState,
    _value: f32,
) {
}

unsafe extern "C" fn source_changed(
    _host: ARAModelUpdateControllerHostRef,
    _source: ARAAudioSourceHostRef,
    _range: *const ARAContentTimeRange,
    _flags: ARAContentUpdateFlags,
) {
}

unsafe extern "C" fn modification_changed(
    host: ARAModelUpdateControllerHostRef,
    _modification: ARAAudioModificationHostRef,
    _range: *const ARAContentTimeRange,
    _flags: ARAContentUpdateFlags,
) {
    // An edit in the plug-in: the project has unsaved changes.
    // SAFETY: the host ref is the document's boxed `Host`, live while the plug-in calls back.
    if let Some(host) = unsafe { host.cast::<Host>().as_ref() } {
        host.changed.store(true, Ordering::Release);
    }
}

unsafe extern "C" fn region_changed(
    _host: ARAModelUpdateControllerHostRef,
    _region: ARAPlaybackRegionHostRef,
    _range: *const ARAContentTimeRange,
    _flags: ARAContentUpdateFlags,
) {
}

unsafe extern "C" fn document_changed(host: ARAModelUpdateControllerHostRef) {
    // SAFETY: as in `modification_changed`.
    if let Some(host) = unsafe { host.cast::<Host>().as_ref() } {
        host.changed.store(true, Ordering::Release);
    }
}

static PLAYBACK: ARAPlaybackControllerInterface = ARAPlaybackControllerInterface {
    structSize: size_of::<ARAPlaybackControllerInterface>(),
    requestStartPlayback: Some(no_playback_request),
    requestStopPlayback: Some(no_playback_request),
    requestSetPlaybackPosition: Some(no_position_request),
    requestSetCycleRange: Some(no_cycle_request),
    requestEnableCycle: Some(no_cycle_switch),
};

// The plug-in's own transport buttons do nothing yet: Suhina's transport plays the song.
unsafe extern "C" fn no_playback_request(_host: ARAPlaybackControllerHostRef) {}

unsafe extern "C" fn no_position_request(_host: ARAPlaybackControllerHostRef, _at: ARATimePosition) {}

unsafe extern "C" fn no_cycle_request(
    _host: ARAPlaybackControllerHostRef,
    _start: ARATimePosition,
    _length: ARATimeDuration,
) {
}

unsafe extern "C" fn no_cycle_switch(_host: ARAPlaybackControllerHostRef, _enable: ARABool) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_fills_each_channel_with_silence_outside() {
        let audio = AudioSource::from_interleaved(&[1.0, -1.0, 2.0, -2.0], 2, 48_000.0).unwrap();
        let source = Box::new(Source { audio: AudioSource::open(&audio.address()).unwrap() });
        let source_ref: *const Source = &*source;
        // SAFETY: a reader of a live source, used and destroyed here.
        let reader = unsafe { create_reader(std::ptr::null_mut(), source_ref.cast_mut().cast(), 0) };
        let (mut left, mut right) = ([9.0f32; 4], [9.0f32; 4]);
        let buffers = [left.as_mut_ptr().cast::<c_void>(), right.as_mut_ptr().cast()];
        // SAFETY: two buffers of four floats for a stereo source.
        let ok = unsafe { read_samples(std::ptr::null_mut(), reader, -1, 4, buffers.as_ptr()) };
        assert_eq!(ok, 1);
        assert_eq!((left, right), ([0.0, 1.0, 2.0, 0.0], [0.0, -1.0, -2.0, 0.0]));
        // A negative count and a missing buffer list are refused.
        // SAFETY: as above.
        assert_eq!(unsafe { read_samples(std::ptr::null_mut(), reader, 0, -1, buffers.as_ptr()) }, 0);
        // SAFETY: as above.
        assert_eq!(unsafe { read_samples(std::ptr::null_mut(), reader, 0, 4, std::ptr::null()) }, 0);
        // SAFETY: the reader made above, destroyed once.
        unsafe { destroy_reader(std::ptr::null_mut(), reader) };

        // Doubles, for a plug-in that asks for 64-bit samples.
        // SAFETY: as above.
        let wide = unsafe { create_reader(std::ptr::null_mut(), source_ref.cast_mut().cast(), 1) };
        let (mut l64, mut r64) = ([9.0f64; 2], [9.0f64; 2]);
        let buffers = [l64.as_mut_ptr().cast::<c_void>(), r64.as_mut_ptr().cast()];
        // SAFETY: two buffers of two doubles.
        assert_eq!(unsafe { read_samples(std::ptr::null_mut(), wide, 1, 2, buffers.as_ptr()) }, 1);
        assert_eq!((l64, r64), ([2.0, 0.0], [-2.0, 0.0]));
        // SAFETY: as above.
        unsafe { destroy_reader(std::ptr::null_mut(), wide) };
    }

    #[test]
    fn a_state_carries_the_archive_and_reads_back() {
        let wrapped = wrap_state(vec![1, 2, 3], Some(("com.example.doc".into(), vec![9; 5])));
        let (component, archive) = unwrap_state(&wrapped);
        assert_eq!(component, [1, 2, 3]);
        assert_eq!(archive, Some(("com.example.doc".to_string(), [9u8; 5].as_slice())));
        // Without an archive, and any state of the plug-in's own, it is the component's.
        assert_eq!(wrap_state(vec![4, 5], None), [4, 5]);
        assert_eq!(unwrap_state(&[4, 5]), ([4u8, 5].as_slice(), None));
        // Cut short, it is not split.
        let cut = &wrapped[..wrapped.len() - 1];
        assert_eq!(unwrap_state(cut), (cut, None));
        let mut huge = wrapped.clone();
        huge[20..28].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(unwrap_state(&huge).1, None);
    }

    #[test]
    fn archives_are_read_and_written_within_bounds() {
        let mut archive = Archive { bytes: Vec::new(), id: CString::new("com.example.doc").unwrap() };
        let writer: ARAArchiveWriterHostRef = std::ptr::from_mut(&mut archive).cast();
        // SAFETY: an archive live for the calls; buffers of the lengths given.
        let write = |at: usize, bytes: &[u8], len: usize| unsafe {
            write_archive(std::ptr::null_mut(), writer, at, len, bytes.as_ptr())
        };
        assert_eq!(write(2, b"abc", 3), 1);
        assert_eq!(write(usize::MAX, b"ab", 2), 0);
        assert_eq!(write(0, b"a", MAX_ARCHIVE + 1), 0);
        assert_eq!(archive.bytes, [0, 0, b'a', b'b', b'c']);
        let reader: ARAArchiveReaderHostRef = std::ptr::from_mut(&mut archive).cast();
        let mut out = [0u8; 3];
        // SAFETY: as above.
        assert_eq!(unsafe { archive_size(std::ptr::null_mut(), reader) }, 5);
        // SAFETY: as above.
        assert_eq!(unsafe { read_archive(std::ptr::null_mut(), reader, 2, 3, out.as_mut_ptr()) }, 1);
        // SAFETY: as above.
        assert_eq!(unsafe { read_archive(std::ptr::null_mut(), reader, 4, 3, out.as_mut_ptr()) }, 0);
        // SAFETY: as above.
        assert!(!unsafe { archive_id(std::ptr::null_mut(), reader) }.is_null());
        assert_eq!(&out, b"abc");
    }
}
