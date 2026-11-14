//! The virtual-register layer: locals cached in callee-saved registers.
//!
//! The frame is always the authority on where a local lives — the cache is
//! an overlay. A load consults it first; a store updates it. A local that
//! is never promoted simply keeps using its slot, so a missed promotion
//! costs speed, never correctness. What the overlay buys: a cached local's
//! value sits in a register (`d8`–`d13`, `x19`–`x24`) that expression
//! evaluation never touches, so it can be used directly as an operand —
//! and it survives a loop's back edge without a store/reload round trip.
//!
//! The registers are callee-saved, so they also survive calls: the runtime
//! preserves them by the C ABI, and the generated code saves whichever of
//! them a function uses in its prologue and restores them in its epilogue.
//!
//! Ownership: an entry is owned by the innermost open *region* — a `while`
//! loop (the function itself is region 0). A loop promotes the locals its
//! body assigns, so they stay in registers across the back edge; when the
//! loop exits, its entries are flushed and dropped. Entries from enclosing
//! scopes are current on every path through the loop and are kept.

/// A local whose value lives in a callee-saved register.
#[derive(Clone, Copy)]
pub(super) struct Entry {
    /// The local's frame offset, its identity (slots are never reused).
    pub slot: i32,
    /// The register number: `d8+n` for a number, `x19+n` otherwise.
    pub reg: u8,
    /// True when the register holds a value the slot has not seen.
    pub dirty: bool,
    /// The region that promoted the local.
    pub owner: u32,
}

/// How many registers of each class the layer may use.
const NUM_FP: usize = 6;
const NUM_GP: usize = 6;

/// The first floating-point register of the pool (`d8`).
pub(super) const FP_BASE: u8 = 8;
/// The first general-purpose register of the pool (`x19`).
pub(super) const GP_BASE: u8 = 19;

/// The cache of one function.
pub(super) struct Cache {
    entries: Vec<Entry>,
    fp_used: [bool; NUM_FP],
    gp_used: [bool; NUM_GP],
    region: u32,
}

impl Cache {
    pub fn new() -> Cache {
        Cache {
            entries: Vec::new(),
            fp_used: [false; NUM_FP],
            gp_used: [false; NUM_GP],
            region: 0,
        }
    }

    /// Open a region; the id identifies its entries at close time.
    pub fn open_region(&mut self) -> u32 {
        self.region += 1;
        self.region
    }

    /// Close a region: flush its dirty entries to their slots and drop
    /// them. Entries of enclosing regions are current on every path and
    /// are left alone. (An inner region's entries cannot outlive it: a
    /// region is closed in emission order, innermost first.)
    pub fn close_region(&mut self, region: u32) -> Vec<Entry> {
        let (mine, kept): (Vec<Entry>, Vec<Entry>) = self
            .entries
            .iter()
            .copied()
            .partition(|e| e.owner == region);
        self.entries = kept;
        for e in &mine {
            self.free(e);
        }
        mine
    }

    fn free(&mut self, e: &Entry) {
        let (n, used) = match e.reg.checked_sub(FP_BASE) {
            Some(n) if (n as usize) < NUM_FP => (n as usize, &mut self.fp_used),
            _ => (e.reg.checked_sub(GP_BASE).unwrap() as usize, &mut self.gp_used),
        };
        used[n] = false;
    }

    /// Drop the entries of locals a scope is closing over. Their values
    /// are dead — the slots can no longer be named — so they are not
    /// flushed.
    pub fn drop_slots(&mut self, slots: &[i32]) {
        let dropped: Vec<Entry> = self
            .entries
            .iter()
            .copied()
            .filter(|e| slots.contains(&e.slot))
            .collect();
        for e in &dropped {
            self.free(e);
        }
        self.entries.retain(|e| !slots.contains(&e.slot));
    }

    /// The register a local is cached in, if any.
    pub fn get(&self, slot: i32) -> Option<Entry> {
        self.entries.iter().copied().find(|e| e.slot == slot)
    }

    /// Mark the cached value as newer than the slot.
    pub fn mark_dirty(&mut self, slot: i32) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.slot == slot) {
            e.dirty = true;
        }
    }

    /// Promote a local into a free register, if one is left. Returns the
    /// register number: `d8+n` for a number, `x19+n` for an `int` or
    /// `boolean`.
    pub fn promote(&mut self, slot: i32, fp: bool) -> Option<u8> {
        if self.get(slot).is_some() {
            return None;
        }
        let (used, base) = if fp {
            (&mut self.fp_used, FP_BASE)
        } else {
            (&mut self.gp_used, GP_BASE)
        };
        let i = used.iter().position(|u| !*u)?;
        used[i] = true;
        let reg = base + i as u8;
        self.entries.push(Entry {
            slot,
            reg,
            dirty: false,
            owner: self.region,
        });
        Some(reg)
    }

    /// The registers the function must save: `(fp numbers, gp numbers)`,
    /// in ascending order.
    pub fn used(&self) -> (Vec<u8>, Vec<u8>) {
        let fp = (0..NUM_FP)
            .filter(|i| self.fp_used[*i])
            .map(|i| FP_BASE + i as u8)
            .collect();
        let gp = (0..NUM_GP)
            .filter(|i| self.gp_used[*i])
            .map(|i| GP_BASE + i as u8)
            .collect();
        (fp, gp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promotion_takes_distinct_registers_and_frees_on_close() {
        let mut c = Cache::new();
        // A function-level entry, outside any region.
        assert!(c.promote(-32, true).is_some());
        let r = c.open_region();
        let a = c.promote(-8, true).unwrap();
        let b = c.promote(-16, true).unwrap();
        assert_ne!(a, b);
        // A local already cached is not promoted twice.
        assert_eq!(c.promote(-8, true), None);
        // Closing the region frees its two entries and no one else's.
        assert_eq!(c.close_region(r).len(), 2);
        assert!(c.get(-8).is_none());
        assert!(c.get(-32).is_some());
        assert!(c.promote(-8, true).is_some());
    }

    #[test]
    fn a_local_promoted_inside_a_region_is_dropped_with_it() {
        let mut c = Cache::new();
        let outer = c.open_region();
        assert!(c.promote(-8, true).is_some());
        let inner = c.open_region();
        assert!(c.promote(-16, true).is_some());
        // Closing the inner region drops only its own entry.
        assert_eq!(c.close_region(inner).len(), 1);
        assert!(c.get(-8).is_some());
        assert!(c.get(-16).is_none());
        assert_eq!(c.close_region(outer).len(), 1);
    }

    #[test]
    fn an_outer_regions_entry_survives_an_inner_loop_only_until_function_end() {
        let mut c = Cache::new();
        let _function = 0;
        assert!(c.promote(-8, true).is_some());
        let loop_region = c.open_region();
        // The loop promotes a local of its own, then closes.
        assert!(c.promote(-16, true).is_some());
        c.close_region(loop_region);
        // The function-level entry is still cached.
        assert!(c.get(-8).is_some());
    }

    #[test]
    fn gp_and_fp_pools_are_independent() {
        let mut c = Cache::new();
        let d = c.promote(-8, true).unwrap();
        let x = c.promote(-16, false).unwrap();
        assert!((FP_BASE..FP_BASE + NUM_FP as u8).contains(&d));
        assert!((GP_BASE..GP_BASE + NUM_GP as u8).contains(&x));
        assert_eq!(c.used().0.len(), 1);
        assert_eq!(c.used().1.len(), 1);
    }

    #[test]
    fn dropping_a_scope_slot_frees_its_register() {
        let mut c = Cache::new();
        assert!(c.promote(-8, true).is_some());
        c.drop_slots(&[-8]);
        assert!(c.get(-8).is_none());
        assert!(c.promote(-24, true).is_some());
    }
}
