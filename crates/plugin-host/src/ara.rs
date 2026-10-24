// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The host side of ARA ([ADR 0028]): a document controller per plug-in and project, which the
//! VST3 instances on the project's tracks share, each its track as a region sequence, built from
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

use std::cell::{Cell, RefCell};
use std::ffi::{CString, c_void};
use std::rc::{Rc, Weak};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use suhina_ipc::AudioSource;
use suhina_plugins::protocol::{AraDocument, AraNote, AraPlayback, AraRegion, MAX_ARA_NOTES};
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

/// Whether `component` supports ARA: it answers `IPlugInEntryPoint`. Asking makes no document
/// and binds nothing.
pub(crate) fn supported(component: &ComPtr<IComponent>) -> bool {
    Interface::query(component, &ENTRY_POINT_IID).is_some()
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
    /// Boxed, so each stays where it is when more are added: the plug-in's readers point at
    /// them.
    #[allow(clippy::vec_box, reason = "readers hold pointers to the sources, which must not move")]
    sources: Vec<Box<Source>>,
    /// The plug-in said its document data changed (ARA 2.3), so the project has unsaved edits.
    changed: AtomicBool,
    /// Counts what can change an archive: the plug-in's edits and analyses, and the host's own
    /// edits and restores. A track's archive stored at the same count is given again rather than
    /// stored anew (storing a large one held the plug-in's editor up for seconds).
    archive_changes: AtomicU64,
    /// Counts the times the plug-in said a region's, modification's or source's notes may have
    /// changed, or the host changed the regions: each track's notes are read again after the
    /// next model update when it moved on (#720).
    notes_changed: AtomicU64,
    /// The sources being analysed (by their host ref, the boxed `Source`) and how far each has
    /// come, 0 to 1; one is taken out when its analysis completes.
    analysis: Mutex<Vec<(usize, f32)>>,
    /// The song's tempo map and time signatures, which the plug-in reads through the musical
    /// context's content readers, so its timeline counts bars as the song does. Changed only
    /// inside an edit, on the model thread, so what a reader handed out stays put.
    tempo: Mutex<Vec<ARAContentTempoEntry>>,
    bars: Mutex<Vec<ARAContentBarSignature>>,
}

impl Host {
    /// Takes the tempo map and time signatures of `document`; whether they changed.
    fn set_context(&self, document: &AraDocument) -> bool {
        let tempo: Vec<ARAContentTempoEntry> = document
            .tempo_entries()
            .into_iter()
            .map(|(seconds, quarters)| ARAContentTempoEntry { timePosition: seconds, quarterPosition: quarters })
            .collect();
        let bars: Vec<ARAContentBarSignature> = document
            .bar_signatures()
            .into_iter()
            .map(|(numerator, denominator, at)| ARAContentBarSignature {
                numerator: i32::try_from(numerator).unwrap_or(4),
                denominator: i32::try_from(denominator).unwrap_or(4),
                position: at,
            })
            .collect();
        let same_tempo = {
            let now = self.tempo.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            now.len() == tempo.len()
                && now.iter().zip(&tempo).all(|(a, b)| {
                    let (a_time, a_quarter, b_time, b_quarter) =
                        (a.timePosition, a.quarterPosition, b.timePosition, b.quarterPosition);
                    a_time == b_time && a_quarter == b_quarter
                })
        };
        let same_bars = {
            let now = self.bars.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            now.len() == bars.len()
                && now.iter().zip(&bars).all(|(a, b)| {
                    let (an, ad, ap, bn, bd, bp) =
                        (a.numerator, a.denominator, a.position, b.numerator, b.denominator, b.position);
                    an == bn && ad == bd && ap == bp
                })
        };
        if same_tempo && same_bars {
            return false;
        }
        *self.tempo.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = tempo;
        *self.bars.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = bars;
        true
    }

    fn notes_may_have_changed(&self) {
        self.notes_changed.fetch_add(1, Ordering::AcqRel);
    }

    /// Something an archive holds may have changed.
    fn archive_may_have_changed(&self) {
        self.archive_changes.fetch_add(1, Ordering::AcqRel);
    }
}

/// How often the host lets the plug-in send the model changes it has queued, when it is not
/// editing: the ARA API asks for it periodically; 250 ms keeps an edit's echo in the plug-in's
/// editor prompt without a busy loop.
const MODEL_UPDATES_EVERY: Duration = Duration::from_millis(250);

/// How often the notes of regions that have none yet are looked for again (#720): a plug-in may
/// finish its analysis without saying the content changed.
const NOTES_AGAIN_EVERY: Duration = Duration::from_secs(1);

thread_local! {
    /// The documents this process holds, by the plug-in's factory and the engine's document
    /// key ([`AraDocument::document`]): the instances of one plug-in in one project share one
    /// (ADR 0028 decision 2), each track a region sequence of it, so the plug-in sees every
    /// track. Weak: a document goes with the last instance bound to it.
    static SHARED: RefCell<Vec<Weak<RefCell<Shared>>>> = const { RefCell::new(Vec::new()) };
}

/// An ARA document controller and the objects made in it that the tracks share: the musical
/// context, the audio sources and their modifications. Each instance bound to it adds a region
/// sequence of its own ([`Document`]).
struct Shared {
    /// The plug-in's factory (by address) and the engine's document key; an empty key is a
    /// document of one instance, never shared.
    key: (usize, String),
    controller: *const ARADocumentControllerInstance,
    /// When `notifyModelUpdates` was last called.
    last_model_update: Cell<Instant>,
    host: Box<Host>,
    context: ARAMusicalContextRef,
    sources: Vec<ARAAudioSourceRef>,
    /// The persistent id of each source, in the same order (its audio pool key).
    source_ids: Vec<String>,
    modifications: Vec<ARAAudioModificationRef>,
    /// The persistent id of each modification, in the same order (`content-<id>`).
    modification_ids: Vec<String>,
    /// The plug-in's factory, valid while its module is loaded.
    factory: *const ARAFactory,
    /// The sources whose notes the host asked the plug-in to find (once each).
    analysis_asked: RefCell<Vec<ARAAudioSourceRef>>,
    /// The texts handed to the plug-in, kept for as long as the document.
    texts: Vec<CString>,
}

/// The document controller's interface and reference. The interface is a copy of the
/// plug-in's table cut to the size it says it has, so an older plug-in's shorter table reads
/// as functions it does not have.
struct Controller {
    interface: ARADocumentControllerInterface,
    reference: ARADocumentControllerRef,
}

impl Shared {
    /// The document of `factory` that `document` names, made (with its musical context) when
    /// this process has none yet.
    fn get_or_make(factory: &ARAFactory, document: &AraDocument) -> Result<Rc<RefCell<Self>>, String> {
        let key = (std::ptr::from_ref(factory) as usize, document.document.clone());
        if !key.1.is_empty() {
            let found = SHARED.with(|all| {
                let mut all = all.borrow_mut();
                all.retain(|weak| weak.strong_count() > 0);
                all.iter().filter_map(Weak::upgrade).find(|shared| shared.borrow().key == key)
            });
            if let Some(shared) = found {
                suhina_plugins::trace!("ara document {:?} shared with another track", key.1);
                return Ok(shared);
            }
        }
        let shared = Rc::new(RefCell::new(Self::make(factory, document, key)?));
        if !shared.borrow().key.1.is_empty() {
            SHARED.with(|all| all.borrow_mut().push(Rc::downgrade(&shared)));
        }
        Ok(shared)
    }

    fn make(factory: &ARAFactory, document: &AraDocument, key: (usize, String)) -> Result<Self, String> {
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
            sources: Vec::new(),
            changed: AtomicBool::new(false),
            archive_changes: AtomicU64::new(0),
            notes_changed: AtomicU64::new(0),
            analysis: Mutex::new(Vec::new()),
            tempo: Mutex::new(Vec::new()),
            bars: Mutex::new(Vec::new()),
        });
        host.set_context(document);
        let host_ref: *mut c_void = std::ptr::from_mut::<Host>(&mut host).cast();
        host.instance.audioAccessControllerHostRef = host_ref.cast();
        host.instance.archivingControllerHostRef = host_ref.cast();
        host.instance.contentAccessControllerHostRef = host_ref.cast();
        host.instance.modelUpdateControllerHostRef = host_ref.cast();
        host.instance.playbackControllerHostRef = host_ref.cast();
        let mut made = Self {
            key,
            controller: std::ptr::null(),
            last_model_update: Cell::new(Instant::now()),
            host,
            context: std::ptr::null_mut(),
            sources: Vec::new(),
            source_ids: Vec::new(),
            modifications: Vec::new(),
            modification_ids: Vec::new(),
            factory,
            analysis_asked: RefCell::new(Vec::new()),
            texts: Vec::new(),
        };
        let properties =
            ARADocumentProperties { structSize: size_of::<ARADocumentProperties>(), name: made.text(&document.name) };
        let create = factory.createDocumentControllerWithDocument.ok_or("the ARA factory makes no documents")?;
        // SAFETY: the host instance and its tables outlive the document controller (both are
        // owned by the `Shared` this returns); the properties live for the call.
        made.controller = unsafe { create(&made.host.instance, &properties) };
        let dc = made.controller().ok_or("the plug-in made no document controller")?;
        let make_context =
            dc.interface.createMusicalContext.ok_or("the document controller cannot make a musical context")?;
        let context = ARAMusicalContextProperties {
            structSize: size_of::<ARAMusicalContextProperties>(),
            name: made.text(&document.name),
            orderIndex: 0,
            color: std::ptr::null(),
        };
        let host_ref: *mut c_void = std::ptr::from_mut::<Host>(&mut made.host).cast();
        made.edit_with(&dc, |_| {
            // SAFETY: a live controller inside an edit, on its model thread; the properties live
            // for the call, the host ref for the document.
            unsafe { make_context(dc.reference, host_ref.cast(), &context) }
        })
        .map(|context| made.context = context)?;
        Ok(made)
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

    /// Runs `work` inside one edit of the document (`beginEditing` .. `endEditing`).
    fn edit_with<T>(&mut self, dc: &Controller, work: impl FnOnce(&mut Self) -> T) -> Result<T, String> {
        let (Some(begin), Some(end)) = (dc.interface.beginEditing, dc.interface.endEditing) else {
            return Err("the document controller cannot be edited".into());
        };
        // SAFETY: a live document controller, on its model thread (this one).
        unsafe { begin(dc.reference) };
        let done = work(self);
        // SAFETY: as above, closing the edit `begin` opened.
        unsafe { end(dc.reference) };
        Ok(done)
    }

    /// Keeps `value` for as long as the document and returns it as a C string for the plug-in.
    fn text(&mut self, value: &str) -> *const std::ffi::c_char {
        let owned = CString::new(value.replace('\0', " ")).unwrap_or_default();
        let pointer = owned.as_ptr();
        self.texts.push(owned);
        pointer
    }

    /// Lets the plug-in send the model changes it has queued (`notifyModelUpdates`): what it
    /// worked out after an edit (its analysis, the regions it changed) reaches its editor now
    /// rather than on its own timer. Called outside an edit, on the model thread.
    fn notify_model_updates(&self) {
        self.last_model_update.set(Instant::now());
        let Some(dc) = self.controller() else { return };
        let Some(notify) = dc.interface.notifyModelUpdates else { return };
        let started = Instant::now();
        // SAFETY: a live document controller, not being edited, on its model thread.
        unsafe { notify(dc.reference) };
        slow_call("notifyModelUpdates", started);
    }

    /// Opens the sources of `document` the document does not have yet, before an edit, so a
    /// source that cannot be read changes nothing.
    fn open_new_sources(&self, document: &AraDocument) -> Result<Vec<(String, String, Box<Source>)>, String> {
        let mut new_sources: Vec<(String, String, Box<Source>)> = Vec::new();
        for described in &document.sources {
            if !self.source_ids.contains(&described.id) && !new_sources.iter().any(|(id, _, _)| *id == described.id) {
                let audio = AudioSource::open(&described.address)
                    .map_err(|e| format!("an audio source could not be opened: {e}"))?;
                new_sources.push((described.id.clone(), described.name.clone(), Box::new(Source { audio })));
            }
        }
        Ok(new_sources)
    }

    /// Takes a tempo or time signature change of `document` (the plug-in reads the musical
    /// context again) and makes the sources opened by [`Shared::open_new_sources`], inside an
    /// edit.
    fn take_context_and_sources(
        &mut self,
        dc: &Controller,
        document: &AraDocument,
        new_sources: Vec<(String, String, Box<Source>)>,
    ) -> Result<(), String> {
        if self.host.set_context(document)
            && !self.context.is_null()
            && let Some(update) = dc.interface.updateMusicalContextContent
        {
            suhina_plugins::trace!("ara musical context changed");
            // SAFETY: a live controller inside an edit, for this document's context; a null
            // range is the whole of it.
            unsafe {
                update(
                    dc.reference,
                    self.context,
                    std::ptr::null(),
                    kARAContentUpdateEverythingChanged as ARAContentUpdateFlags,
                )
            };
        }
        for (id, name, source) in new_sources {
            self.host.sources.push(source);
            if let Err(why) = self.add_source(dc, self.host.sources.len() - 1, &id, &name) {
                // No source was made for it: `host.sources` stays in step with `source_ids`,
                // which the analysis pairs them by.
                let _refused = self.host.sources.pop();
                return Err(why);
            }
        }
        Ok(())
    }

    /// Makes the audio source of `self.host.sources[index]`, persistent id `id`, inside an
    /// edit, and turns on its sample access.
    fn add_source(&mut self, dc: &Controller, index: usize, id: &str, name: &str) -> Result<(), String> {
        let make = dc.interface.createAudioSource.ok_or("the document controller cannot make an audio source")?;
        let source_ref: *const Source = &**self.host.sources.get(index).ok_or("an audio source is missing")?;
        // SAFETY: the box lives as long as the document and does not move.
        let audio = unsafe { &(*source_ref).audio };
        let properties = ARAAudioSourceProperties {
            structSize: size_of::<ARAAudioSourceProperties>(),
            name: self.text(name),
            persistentID: self.text(id),
            sampleCount: i64::try_from(audio.frames()).unwrap_or(i64::MAX),
            sampleRate: audio.sample_rate(),
            channelCount: i32::try_from(audio.channels()).unwrap_or(2),
            merits64BitSamples: 0,
            channelArrangementDataType: kARAChannelArrangementUndefined as ARAChannelArrangementDataType,
            channelArrangement: std::ptr::null(),
        };
        // SAFETY: a live controller inside an edit, on its model thread; the source's host ref
        // is its boxed `Source`, which lives as long as the document.
        let made = unsafe { make(dc.reference, source_ref.cast_mut().cast(), &properties) };
        if made.is_null() {
            return Err(format!("the plug-in refused the audio source {name:?}"));
        }
        if let Some(enable) = dc.interface.enableAudioSourceSamplesAccess {
            // SAFETY: as above, for a source just made.
            unsafe { enable(dc.reference, made, 1) };
        }
        self.sources.push(made);
        self.source_ids.push(id.to_string());
        Ok(())
    }

    /// The modification `region` plays, made inside an edit the first time it is asked for;
    /// `source` is the source's index in this document. Tracks that play the same content
    /// share it, and so its edits, as the project's clips share their content (ADR 0001).
    fn modification_for(
        &mut self,
        dc: &Controller,
        region: &AraRegion,
        source: usize,
    ) -> Result<ARAAudioModificationRef, String> {
        if let Some(index) = self.modification_ids.iter().position(|id| *id == region.modification) {
            return self.modifications.get(index).copied().ok_or_else(|| "a modification is missing".into());
        }
        let source = *self.sources.get(source).ok_or("a region names a source the document lacks")?;
        let host_ref: *mut c_void = std::ptr::from_mut::<Host>(&mut self.host).cast();
        let properties = ARAAudioModificationProperties {
            structSize: size_of::<ARAAudioModificationProperties>(),
            name: self.text(&region.name),
            persistentID: self.text(&region.modification),
        };
        // A copy of a modification this document has starts with its edits (ADR 0028 decision
        // 5): a pasted or split clip, or one that got its own content. A plug-in that cannot
        // clone, or a clone it refuses, gets a new modification instead.
        let original = region
            .clone_of
            .as_ref()
            .and_then(|of| self.modification_ids.iter().position(|id| id == of))
            .and_then(|index| self.modifications.get(index).copied());
        if let (Some(original), Some(clone)) = (original, dc.interface.cloneAudioModification) {
            // SAFETY: a live controller inside an edit, for a modification of this document.
            let made = unsafe { clone(dc.reference, original, host_ref.cast(), &properties) };
            if !made.is_null() {
                self.modifications.push(made);
                self.modification_ids.push(region.modification.clone());
                return Ok(made);
            }
        }
        let make =
            dc.interface.createAudioModification.ok_or("the document controller cannot make an audio modification")?;
        // SAFETY: a live controller inside an edit, for a source of this document.
        let made = unsafe { make(dc.reference, source, host_ref.cast(), &properties) };
        if made.is_null() {
            return Err(format!("the plug-in refused the audio modification of {:?}", region.name));
        }
        self.modifications.push(made);
        self.modification_ids.push(region.modification.clone());
        Ok(made)
    }

    /// Asks the plug-in to find the notes of each source it has not analysed yet, once, when it
    /// says it can (ADR 0028): the notes then show on the clips without the editor being opened.
    /// The work is the plug-in's, in this process; it says when it is done with a content change.
    fn ask_for_notes(&self, dc: &Controller) {
        let started = Instant::now();
        self.ask_for_notes_now(dc);
        slow_call("asking for notes", started);
    }

    fn ask_for_notes_now(&self, dc: &Controller) {
        let notes_type = kARAContentTypeNotes as ARAContentType;
        // SAFETY: the plug-in's factory, valid while its module is loaded.
        let Some(factory) = (unsafe { self.factory.as_ref() }) else { return };
        let types = factory.analyzeableContentTypes;
        let count = factory.analyzeableContentTypesCount.min(64);
        if types.is_null() {
            return;
        }
        // SAFETY: the factory's list of `count` content types, valid while its module is loaded.
        let listed = unsafe { std::slice::from_raw_parts(types, count) };
        if !listed.contains(&notes_type) {
            return;
        }
        let (Some(incomplete), Some(request)) =
            (dc.interface.isAudioSourceContentAnalysisIncomplete, dc.interface.requestAudioSourceContentAnalysis)
        else {
            return;
        };
        let mut asked = self.analysis_asked.borrow_mut();
        for source in &self.sources {
            if asked.contains(source) {
                continue;
            }
            asked.push(*source);
            // SAFETY: a live controller, not being edited, for a source of this document.
            if unsafe { incomplete(dc.reference, *source, notes_type) } != 0 {
                // SAFETY: as above, with a list of one content type.
                unsafe { request(dc.reference, *source, 1, &notes_type) };
            }
        }
    }

    /// Each source with its key in `host.analysis` (its boxed `Source`) and the plug-in's ref.
    fn source_keys(&self) -> Vec<(usize, ARAAudioSourceRef)> {
        self.host
            .sources
            .iter()
            .map(|source| std::ptr::from_ref::<Source>(source) as usize)
            .zip(self.sources.iter().copied())
            .collect()
    }

    /// Takes out of the analyses going on those the plug-in says are complete (#816): a plug-in
    /// that finds a source's analysis done, restored from an archive, says it *started* and
    /// never that it *completed*. True when one was taken out.
    fn settle_analysis(&self, dc: &Controller) -> bool {
        let Some(incomplete) = dc.interface.isAudioSourceContentAnalysisIncomplete else { return false };
        let notes_type = kARAContentTypeNotes as ARAContentType;
        let settled = settle_analysis(&self.host.analysis, &self.source_keys(), |source| {
            // SAFETY: a live controller, not being edited, for a source of this document.
            unsafe { incomplete(dc.reference, source, notes_type) != 0 }
        });
        if settled {
            self.host.notes_may_have_changed();
        }
        settled
    }

    /// The plug-in's document archive id.
    fn archive_id(&self) -> Option<String> {
        // SAFETY: the factory the document was made with, valid while its module is loaded.
        let factory = unsafe { self.factory.as_ref() }?;
        text_of(factory.documentArchiveID)
    }

    /// Stores the sources and modifications with the persistent ids given (and the document's
    /// own data): one track's edits, in a document the tracks share. `None` when the plug-in
    /// cannot store them.
    fn store(&self, source_ids: &[String], modification_ids: &[String]) -> Option<(String, Vec<u8>)> {
        // What the plug-in has queued goes into the archive too.
        self.notify_model_updates();
        let dc = self.controller()?;
        let id = self.archive_id()?;
        let store = dc.interface.storeObjectsToArchive?;
        let sources: Vec<ARAAudioSourceRef> = self
            .source_ids
            .iter()
            .zip(&self.sources)
            .filter(|(id, _)| source_ids.contains(id))
            .map(|(_, source)| *source)
            .collect();
        let modifications: Vec<ARAAudioModificationRef> = self
            .modification_ids
            .iter()
            .zip(&self.modifications)
            .filter(|(id, _)| modification_ids.contains(id))
            .map(|(_, modification)| *modification)
            .collect();
        let filter = ARAStoreObjectsFilter {
            structSize: size_of::<ARAStoreObjectsFilter>(),
            documentData: 1,
            audioSourceRefsCount: sources.len(),
            audioSourceRefs: sources.as_ptr(),
            audioModificationRefsCount: modifications.len(),
            audioModificationRefs: modifications.as_ptr(),
        };
        let mut archive = Archive { bytes: Vec::new(), id: CString::new(id.clone()).ok()? };
        let writer: ARAArchiveWriterHostRef = std::ptr::from_mut(&mut archive).cast();
        // SAFETY: a live document controller, not being edited, on its model thread; the
        // archive, the filter and the lists it points at live for the call.
        let stored = unsafe { store(dc.reference, writer, &filter) };
        (stored != 0).then_some((id, archive.bytes))
    }

    /// Whether the plug-in reads archives of kind `id`: its own, or one it lists as compatible.
    fn check_archive(&self, id: &str) -> Result<(), String> {
        // SAFETY: as in `archive_id`.
        let factory = unsafe { self.factory.as_ref() }.ok_or("no ARA factory")?;
        let mut known = vec![text_of(factory.documentArchiveID)];
        let count = factory.compatibleDocumentArchiveIDsCount.min(MAX_COMPATIBLE_IDS);
        if !factory.compatibleDocumentArchiveIDs.is_null() {
            for index in 0..count {
                // SAFETY: the factory's list holds `compatibleDocumentArchiveIDsCount` ids.
                known.push(text_of(unsafe { factory.compatibleDocumentArchiveIDs.wrapping_add(index).read() }));
            }
        }
        if known.iter().flatten().any(|k| k == id) {
            Ok(())
        } else {
            Err("the plug-in does not read these saved edits".into())
        }
    }

    /// Restores an archive in one edit, with `filter` or (null) every object whose persistent
    /// id matches. Whether the plug-in read it.
    fn restore_with(&mut self, id: &str, bytes: &[u8], filter: *const ARARestoreObjectsFilter) -> Result<bool, String> {
        self.check_archive(id)?;
        let dc = self.controller().ok_or("no document controller")?;
        let restore = dc.interface.restoreObjectsFromArchive.ok_or("the plug-in cannot restore edits")?;
        let mut archive = Archive { bytes: bytes.to_vec(), id: CString::new(id).map_err(|_| "a bad archive id")? };
        let reader: ARAArchiveReaderHostRef = std::ptr::from_mut(&mut archive).cast();
        let restored = self.edit_with(&dc, |_| {
            // SAFETY: a live document controller on its model thread, inside an edit; the
            // archive and the filter live for the call.
            unsafe { restore(dc.reference, reader, filter) }
        })?;
        self.host.archive_may_have_changed();
        self.notify_model_updates();
        Ok(restored != 0)
    }
}

impl Drop for Shared {
    /// Destroys the document's shared objects (modifications, sources, context) in one edit,
    /// once every track's sequence and regions are gone, then the document controller, as ARA
    /// requires.
    fn drop(&mut self) {
        let Some(dc) = self.controller() else { return };
        let i = &dc.interface;
        if let (Some(begin), Some(end)) = (i.beginEditing, i.endEditing) {
            // SAFETY: a live document controller, on its model thread.
            unsafe { begin(dc.reference) };
            for &modification in &self.modifications {
                if let Some(destroy) = i.destroyAudioModification {
                    // SAFETY: a modification made in this document, destroyed once.
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

/// An archive as stored: the plug-in's document archive id and the bytes.
type Stored = (String, Vec<u8>);

/// One plug-in instance's part of an ARA document: its track as a region sequence, the track's
/// clips as playback regions, and the instance bound to the document, its renderers holding
/// those regions. The document itself ([`Shared`]) is shared with the instances of the same
/// plug-in on the project's other tracks.
pub(crate) struct Document {
    shared: Rc<RefCell<Shared>>,
    sequence: ARARegionSequenceRef,
    /// The sequence's name and place, as last told.
    sequence_described: (String, u32),
    regions: Vec<ARAPlaybackRegionRef>,
    /// Each region as it was last described, in the same order (its id is `clip-<id>`, its
    /// `source` an index into the document's sources).
    region_described: Vec<AraRegion>,
    /// The sources and modifications this track's clips played since it was bound, by
    /// persistent id: what its state stores ([`Document::store`]).
    owned_sources: Vec<String>,
    owned_modifications: Vec<String>,
    /// The plug-in's extension instance, whose renderers play and show the regions.
    extension: *const ARAPlugInExtensionInstance,
    /// The plug-in's editor view, told which region the host has selected; `None` when the
    /// plug-in has none.
    editor_view: Option<(ARAEditorViewRef, *const ARAEditorViewInterface)>,
    /// When the regions' notes were last read, and the document's note change count then.
    last_notes_read: Cell<Instant>,
    notes_seen: Cell<u64>,
    /// The notes last sent for each region (its id and a digest of them), so only a change is
    /// sent again.
    notes_sent: RefCell<Vec<(String, u64)>>,
    /// Regions' notes read and not yet taken by the main loop: id, notes, whether cut.
    notes_out: RefCell<Vec<(String, Vec<AraNote>, bool)>>,
    /// The track's archive as last stored, and the document's change count then.
    stored: RefCell<Option<(u64, Stored)>>,
    /// The analysis progress last sent, in whole percents; `None` while none was going on.
    analysis_sent: Cell<Option<u32>>,
    /// When the plug-in was last asked whether the analyses going on are complete.
    last_settled: Cell<Instant>,
    _entry: Interface,
}

impl Document {
    /// Binds `component` (a VST3 plug-in's component, initialised, not yet active) to the
    /// document `document` names (made when this process has none yet), with the track it
    /// describes as a region sequence of its own, in every role: playback renderer, editor
    /// renderer and editor view. Fails when the plug-in does not support ARA 2, or an object
    /// cannot be made.
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
        let shared = Shared::get_or_make(factory, document)?;
        let mut made = Self {
            shared,
            sequence: std::ptr::null_mut(),
            sequence_described: (document.sequence.clone(), document.order),
            regions: Vec::new(),
            region_described: Vec::new(),
            owned_sources: Vec::new(),
            owned_modifications: Vec::new(),
            extension: std::ptr::null(),
            editor_view: None,
            last_notes_read: Cell::new(Instant::now()),
            // Read once the regions are made.
            notes_seen: Cell::new(u64::MAX),
            notes_sent: RefCell::new(Vec::new()),
            notes_out: RefCell::new(Vec::new()),
            stored: RefCell::new(None),
            analysis_sent: Cell::new(None),
            last_settled: Cell::new(Instant::now()),
            _entry: entry,
        };
        let new_sources = made.shared.borrow().open_new_sources(document)?;
        let dc = made.shared.borrow().controller().ok_or("the plug-in has no document controller")?;
        let shared = Rc::clone(&made.shared);
        let built = shared.borrow_mut().edit_with(&dc, |s| {
            s.take_context_and_sources(&dc, document, new_sources)?;
            made.make_sequence(s, &dc)?;
            let regions = made.regions_in(s, document)?;
            for region in &regions {
                made.add_region(s, &dc, region)?;
            }
            Ok::<(), String>(())
        })?;
        built?;

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
        made.extension = extension;
        let all = Edited { added: made.regions.clone(), removed: Vec::new() };
        made.renderers(&all);
        made.shared.borrow().notify_model_updates();
        Ok(made)
    }

    /// Makes the track's region sequence, inside an edit.
    fn make_sequence(&mut self, s: &mut Shared, dc: &Controller) -> Result<(), String> {
        let make = dc.interface.createRegionSequence.ok_or("the document controller cannot make a region sequence")?;
        let host_ref: *mut c_void = std::ptr::from_mut::<Host>(&mut s.host).cast();
        let properties = ARARegionSequenceProperties {
            structSize: size_of::<ARARegionSequenceProperties>(),
            name: s.text(&self.sequence_described.0),
            orderIndex: i32::try_from(self.sequence_described.1).unwrap_or(i32::MAX),
            musicalContextRef: s.context,
            color: std::ptr::null(),
        };
        // SAFETY: a live controller inside an edit, on its model thread; the properties live
        // for the call, the host ref for the document.
        self.sequence = unsafe { make(dc.reference, host_ref.cast(), &properties) };
        if self.sequence.is_null() {
            return Err("the plug-in refused the track's region sequence".into());
        }
        Ok(())
    }

    /// The regions of `document` with their sources as indices into the document's sources.
    fn regions_in(&self, s: &Shared, document: &AraDocument) -> Result<Vec<AraRegion>, String> {
        let mut regions = Vec::with_capacity(document.regions.len());
        for region in &document.regions {
            let id = &document.sources.get(region.source).ok_or("a region names a source the document lacks")?.id;
            let source = s.source_ids.iter().position(|x| x == id).ok_or("a source is missing")?;
            regions.push(AraRegion { source, ..region.clone() });
        }
        Ok(regions)
    }

    /// A region's properties as ARA takes them.
    fn region_properties(&self, s: &mut Shared, region: &AraRegion) -> ARAPlaybackRegionProperties {
        suhina_plugins::trace!(
            "ara region {} {:?}: start {:.3}s duration {:.3}s offset {:.3}s modification {} clone_of {:?}",
            region.id,
            region.name,
            region.start,
            region.duration,
            region.offset,
            region.modification,
            region.clone_of
        );
        ARAPlaybackRegionProperties {
            structSize: size_of::<ARAPlaybackRegionProperties>(),
            transformationFlags: 0,
            startInModificationTime: region.offset,
            durationInModificationTime: region.duration,
            startInPlaybackTime: region.start,
            durationInPlaybackTime: region.duration,
            musicalContextRef: s.context,
            regionSequenceRef: self.sequence,
            name: s.text(&region.name),
            color: std::ptr::null(),
        }
    }

    /// Makes the playback region `region` (its source an index into the document's sources)
    /// inside an edit; returns it.
    fn add_region(
        &mut self,
        s: &mut Shared,
        dc: &Controller,
        region: &AraRegion,
    ) -> Result<ARAPlaybackRegionRef, String> {
        let modification = s.modification_for(dc, region, region.source)?;
        let make = dc.interface.createPlaybackRegion.ok_or("the document controller cannot make a playback region")?;
        let host_ref: *mut c_void = std::ptr::from_mut::<Host>(&mut s.host).cast();
        let properties = self.region_properties(s, region);
        // SAFETY: a live controller inside an edit, for a modification of this document.
        let made = unsafe { make(dc.reference, modification, host_ref.cast(), &properties) };
        if made.is_null() {
            return Err(format!("the plug-in refused the playback region {:?}", region.name));
        }
        self.regions.push(made);
        if let Some(id) = s.source_ids.get(region.source)
            && !self.owned_sources.contains(id)
        {
            self.owned_sources.push(id.clone());
        }
        if !self.owned_modifications.contains(&region.modification) {
            self.owned_modifications.push(region.modification.clone());
        }
        // Kept without `clone_of`: a later document names no copy for it, and the region is the same.
        self.region_described.push(AraRegion { clone_of: None, ..region.clone() });
        Ok(made)
    }

    /// The track this part of the document is, by name.
    pub(crate) fn track(&self) -> &str {
        &self.sequence_described.0
    }

    /// Tells the plug-in's editor view which region the host has selected (`clip-<id>`), so its
    /// editor shows it; `None`, or an id the track does not hold, selects nothing. Returns
    /// whether the plug-in was told.
    pub(crate) fn select(&self, region: Option<&str>) -> bool {
        let Some((view, interface)) = self.editor_view else { return false };
        // SAFETY: the plug-in's editor view table, valid with its extension instance.
        let interface = unsafe { &*interface };
        let Some(notify) = interface.notifySelection else { return false };
        let selected: Vec<ARAPlaybackRegionRef> = region
            .and_then(|id| self.region_described.iter().position(|r| r.id == id))
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
        self.shared.borrow().host.changed.swap(false, Ordering::AcqRel)
    }

    /// Lets the plug-in send the model changes it has queued ([`Shared::notify_model_updates`]).
    pub(crate) fn notify_model_updates(&self) {
        self.shared.borrow().notify_model_updates();
    }

    /// Lets the document send its queued model changes when [`MODEL_UPDATES_EVERY`] has passed
    /// since any of its tracks last did, and reads this track's notes when they may have
    /// changed. The host's main loop calls it between messages.
    pub(crate) fn tick(&self) {
        let due = self.shared.borrow().last_model_update.get().elapsed() >= MODEL_UPDATES_EVERY;
        if due {
            self.notify_model_updates();
        }
        // What the plug-in said changed reached the host inside that call; the notes are read
        // now, outside it, as the API asks. While a region still has no notes (its analysis
        // running, or a plug-in that says nothing when it ends) they are looked for again now
        // and then.
        let changes = self.shared.borrow().host.notes_changed.load(Ordering::Acquire);
        let waiting = self.notes_sent.borrow().len() < self.region_described.len();
        let again = waiting && self.last_notes_read.get().elapsed() >= NOTES_AGAIN_EVERY;
        if changes != self.notes_seen.get() || again {
            self.notes_seen.set(changes);
            self.last_notes_read.set(Instant::now());
            self.read_notes();
        }
        // While this track shows an analysis, the plug-in is asked now and then whether it is
        // done, in case it never says so (#816).
        if self.analysis_sent.get().is_some() && self.last_settled.get().elapsed() >= NOTES_AGAIN_EVERY {
            self.last_settled.set(Instant::now());
            let shared = self.shared.borrow();
            if let Some(dc) = shared.controller() {
                shared.settle_analysis(&dc);
            }
        }
    }

    /// Reads every region's notes and keeps for [`Document::take_notes`] those that differ from
    /// what was sent; a region gone is forgotten, so its notes are sent again if it comes back.
    fn read_notes(&self) {
        let shared = self.shared.borrow();
        let Some(dc) = shared.controller() else { return };
        shared.ask_for_notes(&dc);
        shared.settle_analysis(&dc);
        let mut sent = self.notes_sent.borrow_mut();
        sent.retain(|(id, _)| self.region_described.iter().any(|r| r.id == *id));
        let mut out = self.notes_out.borrow_mut();
        for (region, described) in self.regions.iter().zip(&self.region_described) {
            let started = Instant::now();
            let read = region_notes(&dc, *region, described.start);
            slow_call(&format!("reading the notes of {}", described.id), started);
            let Some((notes, truncated)) = read else {
                suhina_plugins::trace!("ara notes {}: cannot be read", described.id);
                continue;
            };
            suhina_plugins::trace!(
                "ara notes {}: {} notes, first at {:?}s, last ends {:?}s (from region start)",
                described.id,
                notes.len(),
                notes.first().map(|n| n.start),
                notes.iter().map(|n| n.start + n.length).reduce(f64::max)
            );
            let digest = notes_digest(&notes, truncated);
            match sent.iter_mut().find(|(id, _)| *id == described.id) {
                Some((_, last)) if *last == digest => continue,
                Some((_, last)) => *last = digest,
                // A region with no notes is not sent until it has some.
                None if notes.is_empty() => continue,
                None => sent.push((described.id.clone(), digest)),
            }
            out.retain(|(id, ..)| *id != described.id);
            out.push((described.id.clone(), notes, truncated));
        }
    }

    /// How far the analysis of this track's sources has come (the least done of them), when it
    /// moved by a percent, started or ended since the last call: `Some(None)` when nothing of
    /// this track is being analysed any more.
    pub(crate) fn take_analysis(&self) -> Option<Option<f32>> {
        let shared = self.shared.borrow();
        let mine: Vec<usize> = shared
            .source_ids
            .iter()
            .zip(&shared.host.sources)
            .filter(|(id, _)| self.owned_sources.contains(id))
            .map(|(_, source)| std::ptr::from_ref::<Source>(source) as usize)
            .collect();
        let least = {
            let analysis = shared.host.analysis.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            analysis.iter().filter(|(s, _)| mine.contains(s)).map(|(_, p)| *p).reduce(f32::min)
        };
        let percent = least.map(|p| (p * 100.0) as u32);
        if percent == self.analysis_sent.get() {
            return None;
        }
        self.analysis_sent.set(percent);
        Some(least)
    }

    /// The regions' notes read since the last call: region id, its notes as they are now
    /// (empty when it has none any more), and whether there were more than the protocol carries.
    pub(crate) fn take_notes(&self) -> Vec<(String, Vec<AraNote>, bool)> {
        std::mem::take(&mut *self.notes_out.borrow_mut())
    }

    /// This track's edits as an archive: the plug-in's document archive id and the bytes, the
    /// sources and modifications its clips play (and the document's own data). `None` when the
    /// plug-in cannot store them.
    pub(crate) fn store(&self) -> Option<(String, Vec<u8>)> {
        let shared = self.shared.borrow();
        // What the plug-in has queued is counted before the count is read.
        shared.notify_model_updates();
        let changes = shared.host.archive_changes.load(Ordering::Acquire);
        if let Some((at, archive)) = self.stored.borrow().as_ref()
            && *at == changes
        {
            return Some(archive.clone());
        }
        let started = Instant::now();
        let archive = shared.store(&self.owned_sources, &self.owned_modifications)?;
        suhina_plugins::trace!(
            "ara store {}: {} bytes in {:.1} ms",
            self.sequence_described.0,
            archive.1.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        *self.stored.borrow_mut() = Some((changes, archive.clone()));
        Some(archive)
    }

    /// The edits of one modification (`content-<id>`) and its source (the pool key), stored on
    /// their own (a partial archive, ADR 0028 step 6): to give another track's plug-in when a
    /// clip goes there with its edits kept. The plug-in's document archive id and the bytes.
    pub(crate) fn export(&self, modification: &str, source: &str) -> Result<(String, Vec<u8>), String> {
        let shared = self.shared.borrow();
        if !shared.modification_ids.iter().any(|m| m == modification) {
            return Err(format!("the document has no {modification}"));
        }
        if !shared.source_ids.iter().any(|s| s == source) {
            return Err("the document does not have the clip's audio".into());
        }
        shared
            .store(&[source.to_string()], &[modification.to_string()])
            .ok_or_else(|| "the plug-in could not store the clip's edits".into())
    }

    /// Gives the plug-in the edits [`Document::export`] stored in another track's document,
    /// for its modification `to` (it was `from` there) of the same `source`: a clip moved or
    /// copied to this track with its edits kept. Refused while the document has no `to` yet
    /// (the clip has not reached it), or for an archive of a kind the plug-in does not read.
    pub(crate) fn import(&self, id: &str, bytes: &[u8], from: &str, to: &str, source: &str) -> Result<(), String> {
        let mut shared = self.shared.borrow_mut();
        if !shared.modification_ids.iter().any(|m| m == to) {
            return Err(format!("the document has no {to} yet"));
        }
        if !shared.source_ids.iter().any(|s| s == source) {
            return Err("the document does not have the clip's audio yet".into());
        }
        let texts = [source, from, to].map(CString::new);
        let [Ok(source), Ok(from), Ok(to)] = texts else { return Err("a bad persistent id".into()) };
        let source_id = source.as_ptr();
        let (from_id, to_id) = (from.as_ptr(), to.as_ptr());
        let filter = ARARestoreObjectsFilter {
            structSize: size_of::<ARARestoreObjectsFilter>(),
            documentData: 0,
            audioSourceIDsCount: 1,
            audioSourceArchiveIDs: &source_id,
            audioSourceCurrentIDs: &source_id,
            audioModificationIDsCount: 1,
            audioModificationArchiveIDs: &from_id,
            audioModificationCurrentIDs: &to_id,
        };
        if shared.restore_with(id, bytes, &filter)? {
            Ok(())
        } else {
            Err("the plug-in could not read the clip's edits".into())
        }
    }

    /// Gives the plug-in back the edits [`Document::store`] took, for the objects with the same
    /// persistent ids (the same clips). Refused when the archive is of a kind the plug-in does
    /// not read (another plug-in's, or a newer version's).
    pub(crate) fn restore(&self, id: &str, bytes: &[u8]) -> Result<(), String> {
        // No filter: the archive holds this track's objects (a document of its own before the
        // tracks shared one), and every object in it whose id matches takes its edits.
        let restored = self.shared.borrow_mut().restore_with(id, bytes, std::ptr::null())?;
        if restored { Ok(()) } else { Err("the plug-in could not read its saved edits".into()) }
    }

    /// Brings the track's part of the document in line with `document`, the track's clips as
    /// they are now, in one edit: sources and modifications it lacks are made (none goes: their
    /// edits stay in the archive), regions that moved or changed length are updated, new ones
    /// made, the sequence renamed or moved. Regions that are gone, or play another modification
    /// now, are left in the document for [`Document::renderers`] and [`Document::destroy`]; the
    /// result names them and the ones made.
    pub(crate) fn edit(&mut self, document: &AraDocument) -> Result<Edited, String> {
        let shared = Rc::clone(&self.shared);
        let new_sources = shared.borrow().open_new_sources(document)?;
        let dc = shared.borrow().controller().ok_or("the plug-in has no document controller")?;
        let edited = shared.borrow_mut().edit_with(&dc, |s| {
            s.take_context_and_sources(&dc, document, new_sources)?;
            self.apply(s, &dc, document)
        })?;
        let shared = shared.borrow();
        // The plug-in redraws the change once it may send what the edit set off.
        shared.notify_model_updates();
        // Moved, trimmed or new regions: the plug-in need not say so, so their notes are read.
        shared.host.notes_may_have_changed();
        shared.host.archive_may_have_changed();
        edited
    }

    /// [`Document::edit`]'s changes to the track's sequence and regions, inside the edit it
    /// opened.
    fn apply(&mut self, s: &mut Shared, dc: &Controller, document: &AraDocument) -> Result<Edited, String> {
        let wanted = (document.sequence.clone(), document.order);
        if wanted != self.sequence_described
            && let Some(update) = dc.interface.updateRegionSequenceProperties
        {
            self.sequence_described = wanted;
            let properties = ARARegionSequenceProperties {
                structSize: size_of::<ARARegionSequenceProperties>(),
                name: s.text(&self.sequence_described.0),
                orderIndex: i32::try_from(self.sequence_described.1).unwrap_or(i32::MAX),
                musicalContextRef: s.context,
                color: std::ptr::null(),
            };
            // SAFETY: a live controller inside an edit, for this track's sequence.
            unsafe { update(dc.reference, self.sequence, &properties) };
        }
        let regions = self.regions_in(s, document)?;
        let mut edited = Edited::default();
        // Gone, or playing another modification (a region's modification cannot change).
        for (index, old) in self.region_described.iter().enumerate() {
            let keeps = regions.iter().any(|r| r.id == old.id && r.modification == old.modification);
            if !keeps && let Some(&region) = self.regions.get(index) {
                edited.removed.push(region);
            }
        }
        for region in &regions {
            let found = self
                .region_described
                .iter()
                .position(|old| old.id == region.id && old.modification == region.modification)
                .filter(|&index| self.regions.get(index).is_some_and(|r| !edited.removed.contains(r)));
            match found {
                Some(index)
                    if self.region_described.get(index) == Some(&AraRegion { clone_of: None, ..region.clone() }) => {}
                Some(index) => {
                    let update = dc
                        .interface
                        .updatePlaybackRegionProperties
                        .ok_or("the document controller cannot update a playback region")?;
                    let made = *self.regions.get(index).ok_or("a region is missing")?;
                    let properties = self.region_properties(s, region);
                    // SAFETY: a live controller inside an edit, for a region of this document.
                    unsafe { update(dc.reference, made, &properties) };
                    if let Some(old) = self.region_described.get_mut(index) {
                        *old = AraRegion { clone_of: None, ..region.clone() };
                    }
                }
                None => edited.added.push(self.add_region(s, dc, region)?),
            }
        }
        Ok(edited)
    }

    /// Takes the removed regions out of the plug-in's renderers and puts the added ones in.
    /// ARA allows this only while the plug-in does not render, so the caller stops its
    /// processing around it.
    pub(crate) fn renderers(&self, edited: &Edited) {
        // SAFETY: the plug-in's extension instance, valid while it is bound, or null.
        let Some(extension) = (unsafe { self.extension.as_ref() }) else { return };
        // SAFETY: the renderers' tables, valid with the extension instance, or null.
        let playback = unsafe { extension.playbackRendererInterface.as_ref() };
        // SAFETY: as above.
        let editor = unsafe { extension.editorRendererInterface.as_ref() };
        for &region in &edited.removed {
            if let Some(remove) = playback.and_then(|p| p.removePlaybackRegion) {
                // SAFETY: a live renderer and a region of its document it holds, on the model
                // thread.
                unsafe { remove(extension.playbackRendererRef, region) };
            }
            if let Some(remove) = editor.and_then(|e| e.removePlaybackRegion) {
                // SAFETY: as above, for the editor renderer.
                unsafe { remove(extension.editorRendererRef, region) };
            }
        }
        for &region in &edited.added {
            if let Some(add) = playback.and_then(|p| p.addPlaybackRegion) {
                // SAFETY: a live renderer and a region of its document, on the model thread.
                unsafe { add(extension.playbackRendererRef, region) };
            }
            if let Some(add) = editor.and_then(|e| e.addPlaybackRegion) {
                // SAFETY: as above, for the editor renderer.
                unsafe { add(extension.editorRendererRef, region) };
            }
        }
    }

    /// Destroys the regions [`Document::edit`] found gone, in one edit, once the renderers no
    /// longer hold them.
    pub(crate) fn destroy(&mut self, edited: &Edited) {
        if edited.removed.is_empty() {
            return;
        }
        let shared = self.shared.borrow();
        let Some(dc) = shared.controller() else { return };
        let (Some(begin), Some(end), Some(destroy)) =
            (dc.interface.beginEditing, dc.interface.endEditing, dc.interface.destroyPlaybackRegion)
        else {
            return;
        };
        // SAFETY: a live document controller, on its model thread.
        unsafe { begin(dc.reference) };
        for &region in &edited.removed {
            // SAFETY: a region of this document that no renderer holds, destroyed once.
            unsafe { destroy(dc.reference, region) };
        }
        // SAFETY: closing the edit opened above.
        unsafe { end(dc.reference) };
        drop(shared);
        let mut index = 0;
        while index < self.regions.len() {
            if self.regions.get(index).is_some_and(|r| edited.removed.contains(r)) {
                self.regions.remove(index);
                if index < self.region_described.len() {
                    self.region_described.remove(index);
                }
            } else {
                index += 1;
            }
        }
    }
}

/// What [`Document::edit`] changed in the regions the plug-in's renderers hold.
#[derive(Debug, Default)]
pub(crate) struct Edited {
    added: Vec<ARAPlaybackRegionRef>,
    removed: Vec<ARAPlaybackRegionRef>,
}

impl Edited {
    /// Whether the renderers must change, which needs the plug-in's processing stopped.
    pub(crate) fn changes_renderers(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty()
    }
}

impl Drop for Document {
    /// Takes the track's regions out of the instance's renderers, then destroys them and the
    /// track's sequence in one edit. The document itself goes with the last track
    /// ([`Shared`]'s drop). Called with the instance's processing stopped.
    fn drop(&mut self) {
        let all = Edited { added: Vec::new(), removed: self.regions.clone() };
        self.renderers(&all);
        let shared = self.shared.borrow();
        let Some(dc) = shared.controller() else { return };
        let i = &dc.interface;
        if let (Some(begin), Some(end)) = (i.beginEditing, i.endEditing) {
            // SAFETY: a live document controller, on its model thread.
            unsafe { begin(dc.reference) };
            for &region in &self.regions {
                if let Some(destroy) = i.destroyPlaybackRegion {
                    // SAFETY: a region made in this document that no renderer holds, destroyed
                    // once.
                    unsafe { destroy(dc.reference, region) };
                }
            }
            if !self.sequence.is_null()
                && let Some(destroy) = i.destroyRegionSequence
            {
                // SAFETY: as above, for the track's sequence, empty now.
                unsafe { destroy(dc.reference, self.sequence) };
            }
            // SAFETY: closing the edit opened above.
            unsafe { end(dc.reference) };
        }
        // The other tracks' plug-ins hear and show the change.
        shared.notify_model_updates();
        let mine: Vec<(usize, ARAAudioSourceRef)> = shared
            .source_ids
            .iter()
            .zip(shared.source_keys())
            .filter(|(id, _)| self.owned_sources.contains(id))
            .map(|(_, keys)| keys)
            .collect();
        forget_analysis(&shared.host.analysis, &shared.analysis_asked, &mine);
    }
}

/// Traces a call into the plug-in that held this thread (its model thread, where its editor
/// also runs) 50 ms or more: a stalled editor shows which call it was in.
fn slow_call(what: &str, started: Instant) {
    let took = started.elapsed();
    if took >= Duration::from_millis(50) {
        suhina_plugins::trace!("ara slow call: {what} took {:.1} ms", took.as_secs_f64() * 1000.0);
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
    isMusicalContextContentAvailable: Some(context_content),
    getMusicalContextContentGrade: Some(context_grade),
    createMusicalContextContentReader: Some(context_reader),
    isAudioSourceContentAvailable: Some(no_source_content),
    getAudioSourceContentGrade: Some(source_grade),
    createAudioSourceContentReader: Some(no_source_reader),
    getContentReaderEventCount: Some(events),
    getContentReaderDataForEvent: Some(event),
    destroyContentReader: Some(no_reader_to_destroy),
};

// The musical context has the song's tempo map and time signatures (so the plug-in counts bars
// as the song does); Suhina describes no other content (chords, notes) yet: the plug-in analyses
// its own. A reader is the content type it reads, as a pointer-sized number.
const TEMPO_READER: usize = 1;
const BARS_READER: usize = 2;

fn context_kind(kind: ARAContentType) -> Option<usize> {
    if kind == kARAContentTypeTempoEntries as ARAContentType {
        Some(TEMPO_READER)
    } else if kind == kARAContentTypeBarSignatures as ARAContentType {
        Some(BARS_READER)
    } else {
        None
    }
}

unsafe extern "C" fn context_content(
    _host: ARAContentAccessControllerHostRef,
    _context: ARAMusicalContextHostRef,
    kind: ARAContentType,
) -> ARABool {
    ARABool::from(context_kind(kind).is_some())
}

unsafe extern "C" fn context_grade(
    _host: ARAContentAccessControllerHostRef,
    _context: ARAMusicalContextHostRef,
    kind: ARAContentType,
) -> ARAContentGrade {
    // The song's own tempo and meter, as the user set them.
    if context_kind(kind).is_some() {
        kARAContentGradeAdjusted as ARAContentGrade
    } else {
        kARAContentGradeInitial as ARAContentGrade
    }
}

unsafe extern "C" fn context_reader(
    _host: ARAContentAccessControllerHostRef,
    _context: ARAMusicalContextHostRef,
    kind: ARAContentType,
    _range: *const ARAContentTimeRange,
) -> ARAContentReaderHostRef {
    // Every entry, whatever the range: those around it are among them, as ARA asks.
    context_kind(kind).map_or(std::ptr::null_mut(), std::ptr::without_provenance_mut)
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

unsafe extern "C" fn events(host: ARAContentAccessControllerHostRef, reader: ARAContentReaderHostRef) -> ARAInt32 {
    // SAFETY: the host ref is the document's `Host`, which outlives its controller.
    let Some(host) = (unsafe { host.cast::<Host>().as_ref() }) else { return 0 };
    let count = match reader.addr() {
        TEMPO_READER => host.tempo.lock().map_or(0, |t| t.len()),
        BARS_READER => host.bars.lock().map_or(0, |b| b.len()),
        _ => 0,
    };
    ARAInt32::try_from(count).unwrap_or(0)
}

unsafe extern "C" fn event(
    host: ARAContentAccessControllerHostRef,
    reader: ARAContentReaderHostRef,
    index: ARAInt32,
) -> *const c_void {
    // SAFETY: as in `events`.
    let Some(host) = (unsafe { host.cast::<Host>().as_ref() }) else { return std::ptr::null() };
    let Ok(index) = usize::try_from(index) else { return std::ptr::null() };
    // The entries change only inside an edit, so the pointer stays good while the plug-in reads.
    match reader.addr() {
        TEMPO_READER => host.tempo.lock().ok().and_then(|t| t.get(index).map(|e| std::ptr::from_ref(e).cast())),
        BARS_READER => host.bars.lock().ok().and_then(|b| b.get(index).map(|e| std::ptr::from_ref(e).cast())),
        _ => None,
    }
    .unwrap_or(std::ptr::null())
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
    host: ARAModelUpdateControllerHostRef,
    source: ARAAudioSourceHostRef,
    state: ARAAnalysisProgressState,
    value: f32,
) {
    suhina_plugins::trace!("ara analysis progress state {state} value {value:.2}");
    // How far the source's analysis has come, for its clips to show.
    // SAFETY: as in `modification_changed`.
    if let Some(host) = unsafe { host.cast::<Host>().as_ref() } {
        let key = source as usize;
        let mut analysis = host.analysis.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        analysis.retain(|(s, _)| *s != key);
        if state != kARAAnalysisProgressCompleted as ARAAnalysisProgressState {
            analysis.push((key, if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 }));
        }
    }
    // An analysis done: its notes are there to read, whether or not the plug-in also says the
    // content changed.
    // SAFETY: as in `modification_changed`.
    if let Some(host) = unsafe { host.cast::<Host>().as_ref() } {
        host.archive_may_have_changed();
    }
    if state == kARAAnalysisProgressCompleted as ARAAnalysisProgressState {
        // SAFETY: as in `modification_changed`.
        notes_may_have_changed(unsafe { host.cast::<Host>().as_ref() }, 0);
    }
}

unsafe extern "C" fn source_changed(
    host: ARAModelUpdateControllerHostRef,
    _source: ARAAudioSourceHostRef,
    _range: *const ARAContentTimeRange,
    flags: ARAContentUpdateFlags,
) {
    // SAFETY: the host ref is the document's boxed `Host`, live while the plug-in calls back.
    if let Some(host) = unsafe { host.cast::<Host>().as_ref() } {
        host.archive_may_have_changed();
    }
    // An analysis done, say: the regions of the source have notes to read.
    // SAFETY: as in `modification_changed`.
    notes_may_have_changed(unsafe { host.cast::<Host>().as_ref() }, flags);
}

/// Takes out of `analysis` (source key, progress) each source of `sources` (key, plug-in ref)
/// that `incomplete` says is analysed; true when one was taken out. A key not in `sources`
/// stays.
fn settle_analysis(
    analysis: &Mutex<Vec<(usize, f32)>>,
    sources: &[(usize, ARAAudioSourceRef)],
    incomplete: impl Fn(ARAAudioSourceRef) -> bool,
) -> bool {
    let listed: Vec<(usize, ARAAudioSourceRef)> = {
        let analysis = analysis.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        sources.iter().copied().filter(|(key, _)| analysis.iter().any(|(s, _)| s == key)).collect()
    };
    // Asked outside the lock: the plug-in may report progress from another thread meanwhile.
    let done: Vec<usize> = listed.into_iter().filter(|(_, source)| !incomplete(*source)).map(|(key, _)| key).collect();
    if done.is_empty() {
        return false;
    }
    let mut analysis = analysis.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    analysis.retain(|(s, _)| !done.contains(s));
    true
}

/// Forgets what was asked and reported of the analysis of `sources` (key, plug-in ref), the
/// sources of a track that leaves the document (#816): the next track that plays one asks the
/// plug-in again, which sends progress again if it still analyses it. An analysis another
/// track still waits on shows again with the plug-in's next report.
fn forget_analysis(
    analysis: &Mutex<Vec<(usize, f32)>>,
    asked: &RefCell<Vec<ARAAudioSourceRef>>,
    sources: &[(usize, ARAAudioSourceRef)],
) {
    asked.borrow_mut().retain(|source| !sources.iter().any(|(_, s)| s == source));
    let mut analysis = analysis.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    analysis.retain(|(key, _)| !sources.iter().any(|(k, _)| k == key));
}

/// Marks the notes to be read again, unless the plug-in says they stayed as they were.
fn notes_may_have_changed(host: Option<&Host>, flags: ARAContentUpdateFlags) {
    suhina_plugins::trace!("ara content changed flags {flags:#x}");
    if let Some(host) = host
        && flags & kARAContentUpdateNoteScopeRemainsUnchanged as ARAContentUpdateFlags == 0
    {
        host.notes_may_have_changed();
    }
}

/// The notes the plug-in has for `region`, which starts `start` seconds into the song: times
/// from the region's start, read with a playback-region content reader as the API asks of a host
/// that shows them. `Some` with none when it has no notes for it, `None` when they cannot be read.
fn region_notes(dc: &Controller, region: ARAPlaybackRegionRef, start: f64) -> Option<(Vec<AraNote>, bool)> {
    let i = &dc.interface;
    let (Some(available), Some(create), Some(count), Some(data), Some(destroy)) = (
        i.isPlaybackRegionContentAvailable,
        i.createPlaybackRegionContentReader,
        i.getContentReaderEventCount,
        i.getContentReaderDataForEvent,
        i.destroyContentReader,
    ) else {
        return None;
    };
    let notes_type = kARAContentTypeNotes as ARAContentType;
    // SAFETY: a live controller, not being edited, for a region of this document.
    if unsafe { available(dc.reference, region, notes_type) } == 0 {
        return Some((Vec::new(), false));
    }
    // SAFETY: as above; a null range reads the whole region.
    let reader = unsafe { create(dc.reference, region, notes_type, std::ptr::null()) };
    if reader.is_null() {
        return None;
    }
    // SAFETY: the reader just made, destroyed below.
    let total = usize::try_from(unsafe { count(dc.reference, reader) }).unwrap_or(0);
    let mut notes = Vec::with_capacity(total.min(MAX_ARA_NOTES));
    for index in 0..total.min(MAX_ARA_NOTES) {
        let Ok(index) = i32::try_from(index) else { break };
        // SAFETY: an index below the count, for the live reader.
        let event = unsafe { data(dc.reference, reader, index) }.cast::<ARAContentNote>();
        if event.is_null() {
            continue;
        }
        // SAFETY: the plug-in's note, valid until the next call; packed, so read unaligned.
        let note = unsafe { event.read_unaligned() };
        let Ok(pitch) = u8::try_from(note.pitchNumber) else { continue };
        let (at, length, volume) = (note.startPosition - start, note.noteDuration, note.volume);
        if pitch > 127 || !(at.is_finite() && length.is_finite() && length >= 0.0 && volume.is_finite()) {
            continue;
        }
        notes.push(AraNote { start: at, length, pitch, volume: volume.clamp(0.0, 1.0) });
    }
    // SAFETY: the reader made above, not used again.
    unsafe { destroy(dc.reference, reader) };
    Some((notes, total > MAX_ARA_NOTES))
}

/// A digest of a region's notes, to send them only when they change.
fn notes_digest(notes: &[AraNote], truncated: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    truncated.hash(&mut hasher);
    for note in notes {
        (note.start.to_bits(), note.length.to_bits(), note.pitch, note.volume.to_bits()).hash(&mut hasher);
    }
    hasher.finish()
}

unsafe extern "C" fn modification_changed(
    host: ARAModelUpdateControllerHostRef,
    _modification: ARAAudioModificationHostRef,
    _range: *const ARAContentTimeRange,
    flags: ARAContentUpdateFlags,
) {
    // SAFETY: the host ref is the document's boxed `Host`, live while the plug-in calls back.
    if let Some(host) = unsafe { host.cast::<Host>().as_ref() } {
        host.archive_may_have_changed();
    }
    // An edit in the plug-in: the project has unsaved changes.
    // SAFETY: the host ref is the document's boxed `Host`, live while the plug-in calls back.
    let host = unsafe { host.cast::<Host>().as_ref() };
    if let Some(host) = host {
        host.changed.store(true, Ordering::Release);
    }
    notes_may_have_changed(host, flags);
}

unsafe extern "C" fn region_changed(
    host: ARAModelUpdateControllerHostRef,
    _region: ARAPlaybackRegionHostRef,
    _range: *const ARAContentTimeRange,
    flags: ARAContentUpdateFlags,
) {
    // SAFETY: as in `modification_changed`.
    notes_may_have_changed(unsafe { host.cast::<Host>().as_ref() }, flags);
}

unsafe extern "C" fn document_changed(host: ARAModelUpdateControllerHostRef) {
    // SAFETY: the host ref is the document's boxed `Host`, live while the plug-in calls back.
    if let Some(host) = unsafe { host.cast::<Host>().as_ref() } {
        host.archive_may_have_changed();
    }
    // SAFETY: as in `modification_changed`.
    if let Some(host) = unsafe { host.cast::<Host>().as_ref() } {
        host.changed.store(true, Ordering::Release);
    }
}

static PLAYBACK: ARAPlaybackControllerInterface = ARAPlaybackControllerInterface {
    structSize: size_of::<ARAPlaybackControllerInterface>(),
    requestStartPlayback: Some(start_request),
    requestStopPlayback: Some(stop_request),
    requestSetPlaybackPosition: Some(position_request),
    requestSetCycleRange: Some(no_cycle_request),
    requestEnableCycle: Some(no_cycle_switch),
};

/// What the plug-ins asked of the transport, waiting for the main loop to send it on (#717). A
/// plug-in may ask from any thread, so it is behind a lock; a newer position replaces one not
/// sent yet, and a burst is cut at [`MAX_PLAYBACK_REQUESTS`].
static PLAYBACK_REQUESTS: Mutex<Vec<AraPlayback>> = Mutex::new(Vec::new());
const MAX_PLAYBACK_REQUESTS: usize = 16;

fn ask(request: AraPlayback) {
    suhina_plugins::trace!("ara plug-in asks transport {request:?}");
    // Sent at once: the main loop would send it only after the plug-in's click is handled.
    if crate::send_now(&suhina_plugins::protocol::Message::AraPlayback(request)) {
        return;
    }
    let mut requests = PLAYBACK_REQUESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let AraPlayback::Position(_) = request {
        requests.retain(|r| !matches!(r, AraPlayback::Position(_)));
    }
    if requests.len() < MAX_PLAYBACK_REQUESTS {
        requests.push(request);
    }
}

/// The transport requests the plug-ins made since the last call, oldest first.
pub(crate) fn take_playback_requests() -> Vec<AraPlayback> {
    std::mem::take(&mut *PLAYBACK_REQUESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

// The plug-in's own transport buttons and a click in its timeline: the engine's transport plays,
// stops or moves, as Suhina's own buttons and ruler do.
unsafe extern "C" fn start_request(_host: ARAPlaybackControllerHostRef) {
    ask(AraPlayback::Start);
}

unsafe extern "C" fn stop_request(_host: ARAPlaybackControllerHostRef) {
    ask(AraPlayback::Stop);
}

unsafe extern "C" fn position_request(_host: ARAPlaybackControllerHostRef, at: ARATimePosition) {
    if at.is_finite() && at >= 0.0 {
        ask(AraPlayback::Position(at));
        hold_position(at);
    }
}

/// The position an ARA plug-in last asked for, in seconds (as `f64` bits; NaN for none), and
/// until when it is shown, in milliseconds since [`epoch`].
static HELD_POSITION: AtomicU64 = AtomicU64::new(f64::NAN.to_bits());
static HELD_UNTIL: AtomicU64 = AtomicU64::new(0);
/// How long a position asked for is shown at most, should the engine never get there.
const HOLD_FOR: Duration = Duration::from_secs(1);

fn epoch() -> Instant {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn hold_position(seconds: f64) {
    let until = epoch().elapsed().saturating_add(HOLD_FOR).as_millis();
    HELD_UNTIL.store(u64::try_from(until).unwrap_or(u64::MAX), Ordering::Release);
    HELD_POSITION.store(seconds.to_bits(), Ordering::Release);
}

/// The plug-in's place while the transport is stopped: the position it asked for until the
/// engine reports it (or a second has passed), rather than the engine's last one. The request
/// goes to the engine and back, which takes a block or more; the plug-in drew the position it
/// was clicked at, then the old one from the next block, then the new one: its playhead
/// flickered back. `engine` and the result are in samples. Real-time safe.
pub(crate) fn shown_position(engine: i64, sample_rate: f64, playing: bool) -> i64 {
    let held = f64::from_bits(HELD_POSITION.load(Ordering::Acquire));
    if held.is_nan() {
        return engine;
    }
    let now = u64::try_from(epoch().elapsed().as_millis()).unwrap_or(u64::MAX);
    let wanted = (held * sample_rate).round() as i64;
    // The engine is there (within a millisecond), playing, or it took too long: its own.
    if playing || now > HELD_UNTIL.load(Ordering::Acquire) || (engine - wanted).abs() as f64 <= sample_rate / 1000.0 {
        HELD_POSITION.store(f64::NAN.to_bits(), Ordering::Release);
        return engine;
    }
    wanted
}

// The loop (cycle) requests wait for a later step: Suhina's loop region is set in the ruler.

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

    /// A plug-in's source ref for the tests, never read.
    fn source_ref(n: usize) -> ARAAudioSourceRef {
        std::ptr::without_provenance_mut(n)
    }

    /// A plug-in that said an analysis *started* and then found it done (restored from an
    /// archive) never says *completed*: asking it takes the source out, while one it still
    /// analyses stays (#816).
    #[test]
    fn an_analysis_the_plug_in_says_is_complete_stops_showing() {
        let analysis = Mutex::new(vec![(1, 0.0), (2, 0.4), (9, 0.5)]);
        let sources = [(1, source_ref(11)), (2, source_ref(12)), (3, source_ref(13))];
        // Source 12 is still being analysed; 11 is done; 9 is not this document's.
        assert!(settle_analysis(&analysis, &sources, |source| source == source_ref(12)));
        assert_eq!(*analysis.lock().unwrap(), vec![(2, 0.4), (9, 0.5)]);
        // Nothing more to take out: false, and the list stays.
        assert!(!settle_analysis(&analysis, &sources, |source| source == source_ref(12)));
        assert_eq!(*analysis.lock().unwrap(), vec![(2, 0.4), (9, 0.5)]);
        // Source 3 is done but was never listed: nothing changes.
        assert!(!settle_analysis(&Mutex::new(Vec::new()), &sources, |_| false));
    }

    /// A track removed while its source is analysed leaves nothing behind: a plug-in added again
    /// for that source is asked for its analysis again and starts with no stale progress (#816).
    #[test]
    fn a_track_that_leaves_forgets_its_sources_analysis() {
        let analysis = Mutex::new(vec![(1, 0.0), (2, 0.3)]);
        let asked = RefCell::new(vec![source_ref(11), source_ref(12)]);
        forget_analysis(&analysis, &asked, &[(1, source_ref(11))]);
        assert_eq!(*analysis.lock().unwrap(), vec![(2, 0.3)]);
        assert_eq!(*asked.borrow(), vec![source_ref(12)]);
    }

    #[test]
    fn a_position_asked_for_is_shown_until_the_engine_is_there() {
        let rate = 48_000.0;
        // Nothing asked for: the engine's position.
        assert_eq!(shown_position(1000, rate, false), 1000);
        // Asked for 2 s while the engine is still at its old place: 2 s is shown.
        hold_position(2.0);
        assert_eq!(shown_position(1000, rate, false), 96_000);
        assert_eq!(shown_position(1512, rate, false), 96_000);
        // The engine gets there: its own from then on, the hold let go of.
        assert_eq!(shown_position(96_010, rate, false), 96_010);
        assert_eq!(shown_position(1000, rate, false), 1000);
        // Playing, the engine's position always.
        hold_position(3.0);
        assert_eq!(shown_position(1000, rate, true), 1000);
        assert_eq!(shown_position(1000, rate, false), 1000);
    }

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
