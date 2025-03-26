/*!
 * Small no_std and no_alloc opus decoder for mono opus audio
 *
 * Copyright (c) 2025 Tomi Leppänen
 *
 * Uses libopus.
 */

#![no_std]

use az::SaturatingAs;
use core::ffi::{c_int, CStr};
use opus_embedded_sys::*;

#[derive(Debug)]
pub struct DecoderError {
    error_code: c_int,
}

impl DecoderError {
    pub fn message(&self) -> &'static str {
        // SAFETY: DecoderError has been created by us, so it has a valid error_code and null value
        // is handled
        let error = unsafe {
            let error = opus_strerror(self.error_code);
            if error.is_null() {
                return "Unknown error";
            }
            CStr::from_ptr(error)
        };
        error.to_str().unwrap()
    }

    pub fn numeric(&self) -> i32 {
        self.error_code
    }
}

impl core::fmt::Display for DecoderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.message())
    }
}

impl core::error::Error for DecoderError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

pub struct Decoder {
    decoder: OpusDecoder,
}

impl Decoder {
    pub fn new(freq: i32, channels: u8) -> Result<Self, DecoderError> {
        assert!(
            channels != 1 || channels != 2,
            "The number of channels must be 1 or 2"
        );
        // SAFETY: Number of channels was checked to be one or two
        let mut decoder = Decoder {
            decoder: OpusDecoder::default(),
        };
        let size = unsafe { opus_decoder_get_size(channels.into()) };
        assert!(
            core::mem::size_of::<OpusDecoder>() >= size.try_into().unwrap(),
            "OpusDecoder struct is too small!"
        );
        // SAFETY: decoder.decoder points to a correct sized chunk of memory
        let error_code = unsafe { opus_decoder_init(&mut decoder.decoder, freq, channels.into()) };
        // PANIC: All error codes are small positive integers
        if error_code != OPUS_OK.try_into().unwrap() {
            Err(DecoderError { error_code })
        } else {
            Ok(decoder)
        }
    }

    pub fn get_nb_samples(&self, data: &[u8]) -> Result<usize, DecoderError> {
        // SAFETY: Lengths is derived from input arrays
        let samples = unsafe {
            let len = data.len().try_into().unwrap();
            let data = data.as_ptr();
            opus_decoder_get_nb_samples(&self.decoder, data, len)
        };
        if samples < 0 {
            Err(DecoderError {
                error_code: samples,
            })
        } else {
            Ok(samples.saturating_as())
        }
    }

    pub fn decode(&mut self, data: &[u8], output: &mut [i16]) -> Result<usize, DecoderError> {
        // SAFETY: All lengths are derived from input arrays
        let samples = unsafe {
            let len: i32 = data.len().try_into().unwrap();
            let data = data.as_ptr();
            let output_len = output.len();
            let output = output.as_mut_ptr();
            opus_decode(
                &mut self.decoder,
                data,
                len,
                output,
                output_len.saturating_as(),
                0,
            )
        };
        if samples < 0 {
            Err(DecoderError {
                error_code: samples,
            })
        } else {
            Ok(samples.saturating_as())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_decoder() {
        let decoder = Decoder::new(8_000, 1);
        assert!(decoder.is_ok());
    }
}
