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

pub const OPUS_DECODER_SIZE_CH1: usize = 17860;
pub const OPUS_DECODER_SIZE_CH2: usize = 26580;

include!(concat!(env!("OUT_DIR"), "/opus_decoder_gen.rs"));

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
        assert_eq!(core::mem::size_of::<OpusDecoder>(), OPUS_DECODER_SIZE_CH2);
    }
}
