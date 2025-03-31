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
use num_enum::{IntoPrimitive, TryFromPrimitive};
use opus_embedded_sys::*;

/// # Safety
///
/// The implementation of numeric must return a valid error code defined by libopus.
unsafe trait RawOpusError {
    /**
     * Returns valid numeric error code defined by libopus
     */
    fn numeric(&self) -> c_int;
}

pub trait OpusError {
    fn message(&self) -> &'static str;
}

impl<E: RawOpusError> OpusError for E {
    fn message(&self) -> &'static str {
        // SAFETY: OpusError::numeric() returns valid error code and null value is handled
        let error = unsafe {
            let error = opus_strerror(self.numeric());
            if error.is_null() {
                return "Unknown error";
            }
            CStr::from_ptr(error)
        };
        error.to_str().unwrap()
    }
}

#[derive(Debug)]
pub struct DecoderError {
    error_code: c_int,
}

unsafe impl RawOpusError for DecoderError {
    fn numeric(&self) -> c_int {
        // SAFETY: This error code was given by libopus and we trust that it is correct
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

#[derive(Debug)]
pub struct InvalidPacket {}

unsafe impl RawOpusError for InvalidPacket {
    fn numeric(&self) -> c_int {
        OPUS_INVALID_PACKET
    }
}

impl core::fmt::Display for InvalidPacket {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.message())
    }
}

impl core::error::Error for InvalidPacket {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

#[derive(Debug)]
pub struct InvalidChannels {
    tried: i32,
}

impl core::fmt::Display for InvalidChannels {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("Invalid number of channels: {}", self.tried))
    }
}

impl core::error::Error for InvalidChannels {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, IntoPrimitive, TryFromPrimitive)]
#[repr(u8)]
pub enum Channels {
    Mono = 1,
    Stereo = 2,
}

impl Channels {
    pub fn channels(&self) -> u8 {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
        }
    }
}

pub struct Decoder {
    decoder: OpusDecoder,
}

impl Decoder {
    pub fn new(freq: i32, channels: Channels) -> Result<Self, DecoderError> {
        let channels = channels.channels().into();
        let mut decoder = Decoder {
            decoder: OpusDecoder::default(),
        };
        // SAFETY: The number of channels can be only one or two as required
        let size = unsafe { opus_decoder_get_size(channels) };
        assert!(
            core::mem::size_of::<OpusDecoder>() >= size.try_into().unwrap(),
            "OpusDecoder struct is too small!"
        );
        // SAFETY: decoder.decoder points to a correct sized chunk of memory
        let error_code = unsafe { opus_decoder_init(&mut decoder.decoder, freq, channels) };
        // PANIC: All error codes are small integers
        if error_code != OPUS_OK.try_into().unwrap() {
            Err(DecoderError { error_code })
        } else {
            Ok(decoder)
        }
    }

    pub fn get_nb_samples(&self, data: &[u8]) -> Result<usize, InvalidPacket> {
        // SAFETY: Length is derived from input arrays
        let samples = unsafe {
            let len = data.len().try_into().unwrap();
            let data = data.as_ptr();
            opus_decoder_get_nb_samples(&self.decoder, data, len)
        };
        if samples < 0 {
            Err(InvalidPacket {})
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

#[derive(Copy, Clone, Debug)]
pub enum Bandwidth {
    Narrowband,
    Mediumband,
    Wideband,
    Superwideband,
    Fullband,
}

pub struct OpusPacket<'data> {
    data: &'data [u8],
}

impl<'data> OpusPacket<'data> {
    pub fn new(data: &'data [u8]) -> Self {
        Self { data }
    }

    pub fn get_nb_channels(&self) -> Result<u8, InvalidPacket> {
        // SAFETY: Raw data, libopus can deal with it
        let channels = unsafe {
            let data = self.data.as_ptr();
            opus_packet_get_nb_channels(data)
        };
        if channels < 0 {
            debug_assert!(channels == OPUS_INVALID_PACKET);
            Err(InvalidPacket {})
        } else {
            Ok(channels.saturating_as())
        }
    }

    pub fn get_nb_frames(&self) -> Result<u32, InvalidPacket> {
        // SAFETY: Length is derived from input array
        let frames = unsafe {
            let len = self.data.len().try_into().unwrap();
            let data = self.data.as_ptr();
            opus_packet_get_nb_frames(data, len)
        };
        if frames < 0 {
            debug_assert!(frames == OPUS_INVALID_PACKET);
            Err(InvalidPacket {})
        } else {
            Ok(frames.saturating_as())
        }
    }

    pub fn get_bandwidth(&self) -> Result<Bandwidth, InvalidPacket> {
        // SAFETY: Raw data, libopus can deal with it
        let bandwidth = unsafe {
            let data = self.data.as_ptr();
            opus_packet_get_bandwidth(data)
        };
        if bandwidth < 0 {
            debug_assert!(bandwidth == OPUS_INVALID_PACKET);
            Err(InvalidPacket {})
        } else {
            use Bandwidth::*;
            // PANIC: All bandwidth values are small positive integers
            #[allow(non_snake_case)]
            Ok(match bandwidth.try_into().unwrap() {
                OPUS_BANDWIDTH_NARROWBAND => Narrowband,
                OPUS_BANDWIDTH_MEDIUMBAND => Mediumband,
                OPUS_BANDWIDTH_WIDEBAND => Wideband,
                OPUS_BANDWIDTH_SUPERWIDEBAND => Superwideband,
                OPUS_BANDWIDTH_FULLBAND => Fullband,
                _ => panic!("Invalid bandwidth value returned by libopus"),
            })
        }
    }

    pub fn get_samples_per_frame(&self) -> Result<u32, InvalidPacket> {
        // SAFETY: Length is derived from input array
        let samples = unsafe {
            let len = self.data.len().try_into().unwrap();
            let data = self.data.as_ptr();
            opus_packet_get_samples_per_frame(data, len)
        };
        if samples < 0 {
            debug_assert!(samples == OPUS_INVALID_PACKET);
            Err(InvalidPacket {})
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
        let decoder = Decoder::new(8_000, Channels::Stereo);
        assert!(decoder.is_ok());
    }
}
