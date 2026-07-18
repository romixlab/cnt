#![no_std]

use crate::consts::{BKP_BUF_SIZE, RAM_BUF_SIZE};
pub use cnt_macro::{cnt_if, bkp_cnt_if};

mod consts;

#[unsafe(no_mangle)]
static mut _CNT_RAM_BUFFER: [u32; RAM_BUF_SIZE] = [0; RAM_BUF_SIZE];

#[inline(always)]
pub fn counters_ram_buffer() -> &'static [u32] {
    unsafe {
        core::slice::from_raw_parts(
            &raw const _CNT_RAM_BUFFER as *const _ as *const u32,
            RAM_BUF_SIZE,
        )
    }
}

#[inline(always)]
fn counters_ram_buffer_mut() -> &'static mut [u32] {
    unsafe {
        core::slice::from_raw_parts_mut(
            &raw mut _CNT_RAM_BUFFER as *mut _ as *mut u32,
            RAM_BUF_SIZE,
        )
    }
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".cnt_bkp_buffer")]
static mut _CNT_BKP_BUFFER: [u32; BKP_BUF_SIZE] = [0; BKP_BUF_SIZE];

#[inline(always)]
pub fn counters_bkp_buffer() -> &'static [u32] {
    unsafe {
        core::slice::from_raw_parts(
            &raw const _CNT_BKP_BUFFER as *const _ as *const u32,
            BKP_BUF_SIZE,
        )
    }
}

#[inline(always)]
fn counters_bkp_buffer_mut() -> &'static mut [u32] {
    unsafe {
        core::slice::from_raw_parts_mut(
            &raw mut _CNT_BKP_BUFFER as *mut _ as *mut u32,
            BKP_BUF_SIZE,
        )
    }
}

// TODO: Use atomics? and if not available - critical section
#[inline(always)]
pub unsafe fn increment_u32_ram(counter_idx: usize) {
    let buffer = counters_ram_buffer_mut();
    buffer[counter_idx] = buffer[counter_idx].saturating_add(1);
}

#[inline(always)]
pub unsafe fn increment_u32_bkp(counter_idx: usize) {
    let buffer = counters_bkp_buffer_mut();
    buffer[counter_idx] = buffer[counter_idx].saturating_add(1);
}

#[inline(always)]
pub unsafe fn increment_u64_ram(counter_idx_lo: usize, counter_idx_hi: usize) {
    let buffer = counters_ram_buffer_mut();
    increment_u64_inner(buffer, counter_idx_lo, counter_idx_hi);
}

#[inline(always)]
pub unsafe fn increment_u64_bkp(counter_idx_lo: usize, counter_idx_hi: usize) {
    let buffer = counters_bkp_buffer_mut();
    increment_u64_inner(buffer, counter_idx_lo, counter_idx_hi);
}

#[inline(always)]
fn increment_u64_inner(buffer: &mut [u32], counter_idx_lo: usize, counter_idx_hi: usize) {
    let lo = buffer[counter_idx_lo];
    let hi = buffer[counter_idx_hi];
    if hi == u32::MAX {
        buffer[counter_idx_lo] = lo.saturating_add(1);
    } else {
        let (lo, overflowed) = lo.overflowing_add(1);
        buffer[counter_idx_lo] = lo;
        if overflowed {
            buffer[counter_idx_hi] = hi.saturating_add(1);
        }
    }
}
