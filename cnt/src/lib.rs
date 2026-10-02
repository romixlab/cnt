#![no_std]

pub use cnt_macro::{Count, bkp_cnt, bkp_cnt_if, bkp_counters, cnt, cnt_if, counters};
use core::marker::PhantomData;

#[cfg(not(feature = "disabled"))]
mod buffers;
#[cfg(not(feature = "disabled"))]
mod consts;
#[cfg(not(feature = "disabled"))]
pub use buffers::*;

/// With the `disabled` feature all counters are compiled out: the macros expand to nothing (their arguments are still
/// type-checked, but not evaluated), [`Counters`] is a zero-sized no-op and there are no buffers. Useful for library
/// crates that want counters to be optional for their users, or to measure the overhead of counting.
#[cfg(feature = "disabled")]
pub const DISABLED: bool = true;
/// See the `disabled` feature.
#[cfg(not(feature = "disabled"))]
pub const DISABLED: bool = false;

/// Identifies the buffer layout to host tools: magic followed by the format version (little-endian u16). Bump the
/// version when the counters encoding changes in an incompatible way.
#[cfg(not(feature = "disabled"))]
#[used]
#[unsafe(no_mangle)]
#[cfg_attr(target_os = "macos", unsafe(link_section = ".rodata,cnt_signature"))]
#[cfg_attr(
    not(target_os = "macos"),
    unsafe(link_section = ".rodata.cnt_signature")
)]
static _CNT_SIGNATURE: [u8; 8] = *b"CNTRS\0\x02\0";

/// Where a counter lives.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Storage {
    /// `_CNT_RAM_BUFFER`, zeroed on reset by the startup code.
    Ram,
    /// `_CNT_BKP_BUFFER`, in memory that survives a reset.
    Bkp,
}

/// Numeric type of a counter.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ty {
    U32,
    U64,
}

impl Ty {
    /// Number of 32-bit words a counter of this type occupies.
    pub const fn words(self) -> usize {
        match self {
            Ty::U32 => 1,
            Ty::U64 => 2,
        }
    }
}

/// Position of one counter within a [`Count`] type, in 32-bit words.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Slot {
    pub word: usize,
    pub ty: Ty,
}

/// A set of named counters, implemented by `#[derive(Count)]` on an enum with unit variants.
///
/// ```
/// #[derive(cnt::Count)]
/// enum FramerEvent {
///     CrcError,                                   // u32, info
///     #[count(warn)]
///     Overrun,
///     #[count(u64, unit = "B", debug)]
///     RxBytes,
/// }
/// ```
///
/// Variants may be annotated with `#[count(...)]` taking, in any order: `u32` or `u64` (default `u32`), a severity
/// (`error`, `warn`, `info`, `debug`, `trace`, default `info`) and `unit = "..."`.
pub trait Count {
    /// Number of 32-bit words all counters of this type occupy.
    const WORDS: usize;
    /// Marker describing the counters to host tools, its address in the `.cnt_layout` section identifies the layout.
    #[doc(hidden)]
    const LAYOUT: &'static u8;
    /// Word offset and type of a counter.
    fn slot(&self) -> Slot;
}

/// A block of counters of type `E` in the RAM or BKP buffer, created with [`counters!`] or [`bkp_counters!`] in a
/// `static`. Library code takes a `&'static Counters<E>`, so the firmware decides how many instances there are and
/// where they live:
///
/// ```
/// # #[derive(cnt::Count)] enum FramerEvent { CrcError }
/// # struct Framer { cnt: &'static cnt::Counters<FramerEvent> }
/// # impl Framer { fn feed(&self) { self.cnt.count(FramerEvent::CrcError); } }
/// static UART1_FRAMER_CNT: cnt::Counters<FramerEvent> = cnt::counters!(FramerEvent, uart1);
/// static RADIO_FRAMER_CNT: cnt::Counters<FramerEvent> = cnt::bkp_counters!(FramerEvent, radio);
///
/// let uart1 = Framer { cnt: &UART1_FRAMER_CNT };
/// let radio = Framer { cnt: &RADIO_FRAMER_CNT };
/// ```
pub struct Counters<E: Count> {
    /// First byte of the marker in the `.counters_ram`/`.counters_bkp` section, its address is the index of the
    /// first word.
    #[cfg(not(feature = "disabled"))]
    slots: &'static u8,
    #[cfg(not(feature = "disabled"))]
    index: Index,
    storage: Storage,
    _event: PhantomData<fn(E)>,
}

impl<E: Count> Counters<E> {
    #[cfg(not(feature = "disabled"))]
    #[doc(hidden)]
    pub const fn new(slots: &'static u8, storage: Storage) -> Self {
        Self {
            slots,
            index: Index::new(),
            storage,
            _event: PhantomData,
        }
    }

    /// Counters that count nothing, what [`counters!`] expands to with the `disabled` feature.
    #[cfg(feature = "disabled")]
    #[doc(hidden)]
    pub const fn disabled(storage: Storage) -> Self {
        Self {
            storage,
            _event: PhantomData,
        }
    }

    /// Where the counters are stored.
    pub const fn storage(&self) -> Storage {
        self.storage
    }

    /// Add 1 to a counter.
    #[inline(always)]
    pub fn count(&self, event: E) {
        self.add(event, 1);
    }

    /// Add 1 to a counter if `condition` is true.
    #[inline(always)]
    pub fn count_if(&self, condition: bool, event: E) {
        if condition {
            self.add(event, 1);
        }
    }

    /// Add `rhs` to a counter, saturating at the maximum value of the counter's type.
    #[cfg(feature = "disabled")]
    #[inline(always)]
    pub fn add(&self, event: E, rhs: u64) {
        let _ = (event, rhs);
    }

    /// Add `rhs` to a counter, saturating at the maximum value of the counter's type.
    #[cfg(not(feature = "disabled"))]
    #[inline(always)]
    pub fn add(&self, event: E, rhs: u64) {
        // The static holding `self` is immutable, so `storage` and `slot` fold to constants after inlining
        let Slot { word, ty } = event.slot();
        let idx = self.first_word() + word;
        match (self.storage, ty) {
            (Storage::Ram, Ty::U32) => saturating_add_u32_ram(idx, saturate_u32(rhs)),
            (Storage::Bkp, Ty::U32) => saturating_add_u32_bkp(idx, saturate_u32(rhs)),
            (Storage::Ram, Ty::U64) => saturating_add_u64_ram(idx, rhs),
            (Storage::Bkp, Ty::U64) => saturating_add_u64_bkp(idx, rhs),
        }
    }

    /// Current value of a counter, e.g. to assert on it in a test running on the target, or to report it from the
    /// firmware.
    ///
    /// ```
    /// # #[derive(cnt::Count)] enum FramerEvent { CrcError }
    /// static CNT: cnt::Counters<FramerEvent> = cnt::counters!(FramerEvent, test);
    ///
    /// CNT.count(FramerEvent::CrcError);
    /// assert_eq!(CNT.get(FramerEvent::CrcError), 1);
    /// ```
    ///
    /// A u64 counter read from an interrupt that preempted an increment of the same counter may be torn.
    #[cfg(not(feature = "disabled"))]
    #[inline(always)]
    pub fn get(&self, event: E) -> u64 {
        let Slot { word, ty } = event.slot();
        let idx = self.first_word() + word;
        match ty {
            Ty::U32 => load_u32(self.storage, idx).into(),
            Ty::U64 => load_u64(self.storage, idx),
        }
    }

    /// Current value of a counter, always 0 with the `disabled` feature.
    #[cfg(feature = "disabled")]
    #[inline(always)]
    pub fn get(&self, event: E) -> u64 {
        let _ = event;
        0
    }

    /// Set all counters of this instance to 0. An increment running concurrently, e.g. in an interrupt, may be lost
    /// or survive the clear.
    #[cfg(not(feature = "disabled"))]
    #[inline(always)]
    pub fn clear(&self) {
        buffers::clear(self.storage, self.first_word(), E::WORDS);
    }

    /// Set all counters of this instance to 0, a no-op with the `disabled` feature.
    #[cfg(feature = "disabled")]
    #[inline(always)]
    pub fn clear(&self) {}

    /// Index of the first word of this instance in its buffer.
    #[cfg(not(feature = "disabled"))]
    #[inline(always)]
    fn first_word(&self) -> usize {
        self.index.get(self.slots, E::WORDS, self.storage)
    }
}

#[cfg(not(feature = "disabled"))]
#[inline(always)]
const fn saturate_u32(v: u64) -> u32 {
    if v > u32::MAX as u64 {
        u32::MAX
    } else {
        v as u32
    }
}

/// Emitted by [`counters!`] into the `.cnt_instance` section, ties an instance to the layout of its counters. Both
/// fields are resolved by the linker to addresses in INFO sections placed at 0, i.e. the counter index and layout id.
#[doc(hidden)]
#[repr(C)]
pub struct InstanceInfo {
    pub slots: *const u8,
    pub layout: *const u8,
}

// The pointers are only ever read by host tools from the ELF
unsafe impl Sync for InstanceInfo {}

#[cfg(all(test, not(feature = "disabled")))]
mod tests {
    use super::*;

    #[test]
    fn saturate_to_u32() {
        assert_eq!(saturate_u32(5), 5);
        assert_eq!(saturate_u32(1 << 40), u32::MAX);
    }

    #[test]
    fn signature() {
        assert_eq!(&_CNT_SIGNATURE[..6], b"CNTRS\0");
        assert_eq!(
            u16::from_le_bytes([_CNT_SIGNATURE[6], _CNT_SIGNATURE[7]]),
            2
        );
    }
}
