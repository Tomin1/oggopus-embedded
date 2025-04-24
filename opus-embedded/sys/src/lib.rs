/*
 * Copyright (c) 2025 Tomi Leppänen
 * SPDX-License-Identifier: BSD-3-Clause
 */
/*!
 * Minimal bindings for opus decoder
 *
 */

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![no_std]

#[cfg(target_os = "none")]
use core::ffi::{c_char, c_int, CStr};

pub const OPUS_DECODER_SIZE_CH1: usize = 17860;
pub const OPUS_DECODER_SIZE_CH2: usize = 26580;

include!(concat!(env!("OUT_DIR"), "/opus_decoder_gen.rs"));

#[cfg(target_os = "none")]
#[no_mangle]
pub unsafe extern "C" fn celt_fatal(str_: *const c_char, file: *const c_char, line: c_int) {
    /*!
     * Celt fatal implementation that doesn't need C stdlib.
     *
     * # Panics
     * Always.
     *
     * # Safety
     * Caller should ensure that these are valid C strings. Additionally this checks for null
     * pointers.
     */
    unsafe {
        if str_.is_null() {
            panic!("celt_fatal: str_ is null");
        }
        if file.is_null() {
            panic!("celt_fatal: file is null");
        }
        let str_ = CStr::from_ptr(str_);
        let file = CStr::from_ptr(file);
        panic!(
            "{}: {}: {}",
            str_.to_str().unwrap(),
            file.to_str().unwrap(),
            line
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_reported_size() {
        let size = unsafe { opus_decoder_get_size(1) };
        assert_eq!(size, OPUS_DECODER_SIZE_CH1.try_into().unwrap());
        let size = unsafe { opus_decoder_get_size(2) };
        assert_eq!(size, OPUS_DECODER_SIZE_CH2.try_into().unwrap());
    }

    #[test]
    fn check_struct_size() {
        assert_eq!(
            core::mem::size_of::<OpusDecoder>(),
            if cfg!(feature = "stereo") {
                OPUS_DECODER_SIZE_CH2
            } else {
                OPUS_DECODER_SIZE_CH1
            }
        );
    }
}
