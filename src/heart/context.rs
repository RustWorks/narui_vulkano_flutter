use crate::{EvaluatedFragment, Fragment, Key, KeyMap, Layouter, UnevaluatedFragment};
use dashmap::DashMap;
use derivative::Derivative;
use hashbrown::{HashMap, HashSet};

use crate::MaybeEvaluatedFragment::{Evaluated, Unevaluated};
use freelist::FreeList;
use parking_lot::{MappedRwLockReadGuard, RwLock, RwLockReadGuard};
use rutter_layout::Idx;
use smallvec::SmallVec;
use std::{any::Any, fmt::Debug, num::NonZeroUsize, ops::Deref, sync::Arc};
use tinyset::Set64;

#[derive(Debug)]
pub enum MaybeEvaluatedFragment {
    Unevaluated(UnevaluatedFragment),
    Evaluated(EvaluatedFragment),
}

impl MaybeEvaluatedFragment {
    pub(crate) fn key(&self) -> Key {
        match self {
            Unevaluated(frag) => frag.key,
            Evaluated(frag) => frag.key,
        }
    }

    pub(crate) fn assert_evaluated(&self) -> &EvaluatedFragment {
        match self {
            MaybeEvaluatedFragment::Evaluated(frag) => frag,
            _ => panic!("tried to get a evaluated fragment from {:?}, but was unevaluated", self),
        }
    }

    pub(crate) fn assert_unevaluated(&self) -> &UnevaluatedFragment {
        match self {
            MaybeEvaluatedFragment::Unevaluated(frag) => frag,
            _ => panic!("tried to get a unevaluated fragment from {:?}, but was evaluated", self),
        }
    }

    pub(crate) fn into_evaluated(self) -> EvaluatedFragment {
        match self {
            MaybeEvaluatedFragment::Evaluated(frag) => frag,
            _ => panic!("tried to get a evaluated fragment from {:?}, but was unevaluated", self),
        }
    }

    pub(crate) fn into_unevaluated(self) -> UnevaluatedFragment {
        match self {
            MaybeEvaluatedFragment::Unevaluated(frag) => frag,
            _ => panic!("tried to get a unevaluated fragment from {:?}, but was evaluated", self),
        }
    }

    pub(crate) fn assert_evaluated_mut(&mut self) -> &mut EvaluatedFragment {
        match self {
            MaybeEvaluatedFragment::Evaluated(frag) => frag,
            _ => panic!("tried to get a evaluated fragment from {:?}, but was unevaluated", self),
        }
    }

    pub(crate) fn assert_unevaluated_mut(&mut self) -> &mut UnevaluatedFragment {
        match self {
            MaybeEvaluatedFragment::Unevaluated(frag) => frag,
            _ => panic!("tried to get a unevaluated fragment from {:?}, but was evaluated", self),
        }
    }
}

#[derive(Debug)]
pub struct FragmentInfo {
    pub fragment: Option<MaybeEvaluatedFragment>,
    pub args: Option<SmallVec<[Box<dyn Any>; 8]>>,
}

#[derive(Debug, Default)]
pub struct FragmentStore {
    pub(crate) data: FreeList<FragmentInfo>,
    dirty_args: Vec<Fragment>,
}

impl FragmentStore {
    pub fn add_empty_fragment(&mut self) -> Fragment {
        let idx = Fragment(self.data.add(FragmentInfo { fragment: None, args: None }));
        log::trace!("initialized a new fragment with idx {:?}", idx);
        idx
    }

    pub unsafe fn removed(&mut self, idx: Fragment) -> bool {
        self.data.removed(idx.0) || self.data[idx.0].fragment.is_none()
    }

    pub fn add_fragment(
        &mut self,
        idx: Fragment,
        init: impl FnOnce() -> UnevaluatedFragment,
    ) -> Fragment {
        if self.data[idx.0].fragment.is_none() {
            self.data[idx.0].fragment = Some(MaybeEvaluatedFragment::Unevaluated(init()));
        }
        idx
    }

    pub(crate) fn get(&self, idx: Fragment) -> &MaybeEvaluatedFragment {
        self.data[idx.0].fragment.as_ref().unwrap()
    }

    pub(crate) fn get_mut(&mut self, idx: Fragment) -> &mut MaybeEvaluatedFragment {
        self.data[idx.0].fragment.as_mut().unwrap()
    }

    pub fn remove(&mut self, idx: Fragment) {
        self.data[idx.0].fragment = None;
        self.data.remove(idx.0);
    }

    pub fn get_args(&self, idx: Fragment) -> &Option<SmallVec<[Box<dyn Any>; 8]>> {
        &self.data[idx.0].args
    }

    pub fn set_args(&mut self, idx: Fragment, args: SmallVec<[Box<dyn Any>; 8]>) {
        self.dirty_args.push(idx);
        self.data[idx.0].args = Some(args);
    }

    pub fn dirty_args<'a>(&'a mut self) -> impl Iterator<Item = Fragment> + 'a {
        self.dirty_args.drain(..).rev()
    }
}

#[derive(Debug, Default)]
pub struct ExternalHookCount {
    counts: HashMap<Key, u16>,
}

impl ExternalHookCount {
    fn next(&mut self, key: Key) -> u16 {
        let count = self.counts.entry(key).or_insert(0);
        let idx = *count;
        *count += 1;
        idx
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct WidgetContext<'a> {
    pub widget_local: WidgetLocalContext,
    #[derivative(Debug = "ignore")]
    pub tree: Arc<PatchedTree>,
    pub local_hook: bool,
    pub external_hook_count: &'a mut ExternalHookCount,
    pub fragment_store: &'a mut FragmentStore,
    pub widget_loc: (usize, usize),
    #[derivative(Debug(format_with = "crate::util::format_helpers::print_vec_len"))]
    pub(crate) after_frame_callbacks: &'a mut Vec<AfterFrameCallback>,
    pub key_map: &'a mut KeyMap,
}

impl<'a> WidgetContext<'a> {
    pub fn key_for_hook(&mut self) -> HookKey {
        if self.local_hook {
            let counter = self.widget_local.hook_counter;
            self.widget_local.hook_counter += 1;
            log::trace!(
                "creating local hook: {:?}:{}",
                self.key_map.key_debug(self.widget_local.key),
                counter
            );
            (self.widget_local.key, counter)
        } else {
            let key = self.external_hook_count.next(self.widget_local.key) | 0b1000_0000_0000_0000;
            log::trace!(
                "creating external hook: {:?}:{}",
                self.key_map.key_debug(self.widget_local.key),
                key
            );
            (self.widget_local.key, key)
        }
    }

    pub fn thread_context(&self) -> ThreadContext { ThreadContext { tree: self.tree.clone() } }

    pub fn root(
        top: Fragment,
        tree: Arc<PatchedTree>,
        external_hook_count: &'a mut ExternalHookCount,
        fragment_store: &'a mut FragmentStore,
        after_frame_callbacks: &'a mut Vec<AfterFrameCallback>,
        key_map: &'a mut KeyMap,
    ) -> Self {
        Self {
            tree,
            after_frame_callbacks,
            fragment_store,
            widget_local: WidgetLocalContext::for_key(Default::default(), top),
            widget_loc: (0, 0),
            key_map,
            external_hook_count,
            local_hook: true,
        }
    }

    pub fn for_fragment(
        tree: Arc<PatchedTree>,
        external_hook_count: &'a mut ExternalHookCount,
        fragment_store: &'a mut FragmentStore,
        key: Key,
        idx: Fragment,
        after_frame_callbacks: &'a mut Vec<AfterFrameCallback>,
        key_map: &'a mut KeyMap,
    ) -> Self {
        WidgetContext {
            tree,
            after_frame_callbacks,
            fragment_store,
            widget_local: WidgetLocalContext::for_key(key, idx),
            widget_loc: (0, 0),
            key_map,
            external_hook_count,
            local_hook: true,
        }
    }

    pub fn with_key_widget(&mut self, key: Key, idx: Fragment) -> WidgetContext {
        WidgetContext {
            tree: self.tree.clone(),
            local_hook: true,
            external_hook_count: &mut self.external_hook_count,
            fragment_store: self.fragment_store,
            widget_loc: (0, 0),
            after_frame_callbacks: self.after_frame_callbacks,
            widget_local: WidgetLocalContext::for_key(key, idx),
            key_map: &mut self.key_map,
        }
    }
}

#[derive(Clone)]
pub struct ThreadContext {
    pub(crate) tree: Arc<PatchedTree>,
}

pub struct CallbackContext<'a> {
    pub(crate) tree: Arc<PatchedTree>,
    pub key_map: &'a KeyMap,
    pub(crate) layout: &'a Layouter,
    pub(crate) fragment_store: &'a FragmentStore,
}

// thread access
//   - get value (not listen because we don't have the rebuild if changed thing)
//   - shout
// widget access
//   - create listenable
//   - listen
//   - create after-frame-callback
// callback access
//   - shout
//   - get value
//   - measure

type Dependents = tinyset::Set64<usize>;
pub type TreeItem = Box<dyn Any + Send + Sync>;

#[derive(Debug)]
struct Patch<T> {
    key: HookKey,
    value: T,
}

// TODO(robin): investigate evmap instead
type FxDashMap<K, V> = DashMap<K, V, ahash::RandomState>;

// 15 bits of idx + top bit set if external
pub type HookKey = (Key, u16);
pub type HookRef = (HookKey, Idx);

#[derive(Debug, Default)]
pub struct PatchedTree {
    data: RwLock<FreeList<(Set64<usize>, TreeItem)>>,
    key_to_idx: RwLock<HashMap<Key, HashMap<u16, Idx>>>,
    patch: FxDashMap<Idx, Patch<TreeItem>>,
}

type DataRef<'a> = MappedRwLockReadGuard<'a, (Set64<usize>, TreeItem)>;
type HashPatchRef<'a> = dashmap::mapref::one::Ref<'a, Idx, Patch<TreeItem>, ahash::RandomState>;

pub struct PatchTreeEntry<'a> {
    patched_entry: Option<HashPatchRef<'a>>,
    unpatched_entry: Option<DataRef<'a>>,
}

impl<'a> PatchTreeEntry<'a> {
    fn new(patched_entry: Option<HashPatchRef<'a>>, unpatched_entry: Option<DataRef<'a>>) -> Self {
        Self { patched_entry, unpatched_entry }
    }
}

impl<'a> Deref for PatchTreeEntry<'a> {
    type Target = TreeItem;

    fn deref(&self) -> &Self::Target {
        match &self.patched_entry {
            Some(p) => &p.value().value,
            None => match &self.unpatched_entry {
                Some(v) => &(*v).1,
                None => unreachable!(),
            },
        }
    }
}

impl PatchedTree {
    pub fn get_patched(&self, idx: HookRef) -> PatchTreeEntry {
        match self.patch.get(&idx.1) {
            None => self.get_unpatched(idx),
            Some(patch) => PatchTreeEntry::new(Some(patch), None),
        }
    }

    pub fn get_unpatched(&self, idx: HookRef) -> PatchTreeEntry {
        PatchTreeEntry::new(None, Some(RwLockReadGuard::map(self.data.read(), |v| &v[idx.1])))
    }

    pub fn remove_patch(&self, idx: HookRef) { self.patch.remove(&idx.1); }

    pub fn initialize(&self, key: HookKey, value: TreeItem) -> HookRef {
        (
            key,
            *self
                .key_to_idx
                .write()
                .entry(key.0)
                .or_default()
                .entry(key.1)
                .or_insert_with(|| self.data.write().add((Default::default(), value))),
        )
    }

    pub fn initialize_with(&self, key: HookKey, gen: impl FnOnce() -> TreeItem) -> HookRef {
        (
            key,
            *self
                .key_to_idx
                .write()
                .entry(key.0)
                .or_default()
                .entry(key.1)
                .or_insert_with(|| self.data.write().add((Default::default(), gen()))),
        )
    }

    pub fn set(&self, idx: HookRef, value: TreeItem) {
        self.patch.insert(idx.1, Patch { value, key: idx.0 });
    }

    pub fn set_unconditional(&self, idx: Idx, value: TreeItem) { self.data.write()[idx].1 = value; }

    pub fn remove_widget(&self, key: &Key) {
        if let Some(indices) = self.key_to_idx.write().remove(key) {
            for idx in indices.values() {
                self.data.write().remove(*idx);
            }
        }
    }

    // apply the patch to the tree starting a new frame
    pub fn update_tree<'a>(&'a self, _key_map: &mut KeyMap) -> impl Iterator<Item = HookRef> + 'a {
        let mut keys = vec![];
        for kv in self.patch.iter() {
            keys.push(*kv.key());
        }

        keys.into_iter().map(move |idx| {
            let (idx, Patch { value, key }) = self.patch.remove(&idx).unwrap();
            self.set_unconditional(idx, value);

            (key, idx)
        })
    }

    pub fn set_dependent(&self, key: HookRef, frag: Fragment) {
        self.data.write()[key.1].0.insert(frag.0.get());
    }

    pub fn dependents<'a>(&'a self, key: HookRef) -> impl Iterator<Item = Fragment> + 'a {
        std::mem::take(&mut self.data.write()[key.1].0)
            .into_iter()
            .map(|v| Fragment(unsafe { NonZeroUsize::new_unchecked(v) }))
    }
}

pub type AfterFrameCallback = Box<dyn for<'a> Fn(&'a CallbackContext<'a>)>;

#[derive(Clone, Debug)]
pub struct WidgetLocalContext {
    pub key: Key,
    pub idx: Fragment,
    pub hook_counter: u16,
}

impl WidgetLocalContext {
    pub fn for_key(key: Key, idx: Fragment) -> Self { Self { idx, key, hook_counter: 0 } }
}
