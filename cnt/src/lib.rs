#![no_std]

use crate::consts::{BKP_BUF_SIZE, RAM_BUF_SIZE};
pub use cnt_macro::{bkp_cnt_if, cnt_if};

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

#[inline(always)]
pub unsafe fn saturating_add_u32_ram(counter_idx: usize, rhs: u32) {
    let buffer = counters_ram_buffer_mut();
    buffer[counter_idx] = buffer[counter_idx].saturating_add(rhs);
}

#[inline(always)]
pub unsafe fn saturating_add_u32_bkp(counter_idx: usize, rhs: u32) {
    let buffer = counters_bkp_buffer_mut();
    buffer[counter_idx] = buffer[counter_idx].saturating_add(rhs);
}

#[inline(always)]
pub unsafe fn saturating_add_u64_ram(counter_idx_lo: usize, counter_idx_hi: usize, rhs: u64) {
    let buffer = counters_ram_buffer_mut();
    saturating_add_u64_inner(buffer, counter_idx_lo, counter_idx_hi, rhs);
}

#[inline(always)]
pub unsafe fn saturating_add_u64_bkp(counter_idx_lo: usize, counter_idx_hi: usize, rhs: u64) {
    let buffer = counters_bkp_buffer_mut();
    saturating_add_u64_inner(buffer, counter_idx_lo, counter_idx_hi, rhs);
}

#[inline(always)]
fn saturating_add_u64_inner(
    buffer: &mut [u32],
    counter_idx_lo: usize,
    counter_idx_hi: usize,
    rhs: u64,
) {
    let lo = buffer[counter_idx_lo];
    let hi = buffer[counter_idx_hi];
    let value = (hi as u64) << 32 | (lo as u64);
    let value = value.saturating_add(rhs);
    let lo = value as u32 & u32::MAX;
    let hi = (value >> 32) as u32;
    buffer[counter_idx_lo] = lo;
    buffer[counter_idx_hi] = hi;
}
