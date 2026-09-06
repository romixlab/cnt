#![no_std]

use crate::consts::{BKP_BUF_SIZE, RAM_BUF_SIZE};
pub use cnt_macro::{bkp_cnt_if, cnt_if};

mod consts;

#[used]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".rodata.cnt_signature")]
static mut _CNT_SIGNATURE: [u8; 8] = [0; 8];

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
// #[cfg_attr(target_os = "macos", unsafe(link_section = ".cnt_bkp_buffer,cnt.BUFFER"))]
// #[cfg_attr(not(target_os = "macos"), link_section = ".cnt_bkp_buffer.cnt.BUFFER")]
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

pub unsafe fn saturating_add_u64_ram(counter_idx_lo: usize, rhs: u64) {
    unsafe {
        let value = counters_ram_buffer_mut()
            .as_mut_ptr()
            .offset(counter_idx_lo as isize) as *mut u64;
        *value = (*value).saturating_add(rhs);
    }
}

pub unsafe fn saturating_add_u64_bkp(counter_idx_lo: usize, rhs: u64) {
    unsafe {
        let value = counters_bkp_buffer_mut()
            .as_mut_ptr()
            .offset(counter_idx_lo as isize) as *mut u64;
        *value = (*value).saturating_add(rhs);
    }
}
