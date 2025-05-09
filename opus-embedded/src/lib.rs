/*
 * Copyright (c) 2025 Tomi Leppänen
 * SPDX-License-Identifier: BSD-3-Clause
 */
/*!
 * Small no_std and no_alloc opus decoder for opus audio.
 *
 * Uses libopus.
 */

#![no_std]
#![deny(missing_docs)]

use az::SaturatingAs;
use core::ffi::{c_int, CStr};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use opus_embedded_sys::*;

pub mod prelude {
    /*!
     * opus_embedded prelude.
     *
     * Includes the most commonly needed types.
     *
     * ```
     * # #![allow(unused_imports)]
     * use opus_embedded::prelude::*;
     * ```
     */

    pub use super::{Channels, Decoder, SamplingRate};
}

/**
 * # Safety
 *
 * The implementation of numeric must return a valid error code defined by libopus.
 */
unsafe trait RawOpusError {
    /**
     * Returns valid numeric error code defined by libopus.
     */
    fn numeric(&self) -> c_int;
}

/// Error from parsing opus data.
pub trait OpusError {
    /// Returns the error message as it is defined by libopus.
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

/// Error from decoding opus data.
#[derive(Debug, PartialEq)]
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

/// Invalid opus data packet encountered.
#[derive(Debug, PartialEq)]
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

/**
 * Number of channels for opus decoder.
 *
 * Note that stereo decoders cannot be created if stereo feature has not been enabled.
 */
#[derive(Copy, Clone, Debug, Eq, PartialEq, IntoPrimitive, TryFromPrimitive)]
#[repr(u8)]
pub enum Channels {
    /// Select mono audio.
    Mono = 1,
    /// Select stereo audio. Samples are interleaved.
    Stereo = 2,
}

impl Channels {
    /// Return the number of channels.
    pub fn channels(&self) -> u8 {
        (*self).into()
    }
}

/// Opus decoder.
#[derive(Debug)]
pub struct Decoder {
    decoder: OpusDecoder,
    channels: Channels,
}

/**
 * Sampling rate.
 *
 * Only valid sampling rates can be presented.
 */
#[derive(Copy, Clone, Debug, Eq, PartialEq, IntoPrimitive, TryFromPrimitive)]
#[repr(i32)]
pub enum SamplingRate {
    /// 8 kHz sampling rate.
    F8k = 8000,
    /// 12 kHz sampling rate.
    F12k = 12000,
    /// 16 kHz sampling rate.
    F16k = 16000,
    /// 24 kHz sampling rate.
    F24k = 24000,
    /// 48 kHz sampling rate.
    F48k = 48000,
}

impl SamplingRate {
    /// Creates sampling rate that is the same or higher than the requested value up to 48 kHz.
    pub fn closest(value: i32) -> Self {
        use SamplingRate::*;
        if value <= F8k.into() {
            F8k
        } else if value <= F12k.into() {
            F12k
        } else if value <= F16k.into() {
            F16k
        } else if value <= F24k.into() {
            F24k
        } else {
            F48k
        }
    }
}

impl Decoder {
    /**
     * Construct decoder from requested sampling rate and number of channels.
     *
     * See also [`opus_decoder_get_size`] and [`opus_decoder_init`].
     */
    pub fn new(freq: SamplingRate, channels: Channels) -> Result<Self, DecoderError> {
        if !cfg!(feature = "stereo") && channels == Channels::Stereo {
            let error_code = OPUS_ALLOC_FAIL;
            return Err(DecoderError { error_code });
        }
        let mut decoder = Decoder {
            decoder: OpusDecoder::default(),
            channels,
        };
        let channels = channels.channels().into();
        // SAFETY: The number of channels can be only one or two as required
        let size = unsafe { opus_decoder_get_size(channels) };
        assert!(
            core::mem::size_of::<OpusDecoder>() >= size.try_into().unwrap(),
            "OpusDecoder struct is too small!"
        );
        // SAFETY: decoder.decoder points to a correct sized chunk of memory
        let error_code = unsafe { opus_decoder_init(&mut decoder.decoder, freq.into(), channels) };
        // PANIC: All error codes are small integers
        if error_code != OPUS_OK.try_into().unwrap() {
            Err(DecoderError { error_code })
        } else {
            Ok(decoder)
        }
    }

    /**
     * Return the number of samples in the opus data multiplied by the number of channels.
     *
     * This value can be used for output buffer size for decoding when total number of samples in
     * frame is expected.
     *
     * See also [`Decoder::get_nb_samples`].
     */
    pub fn get_nb_samples_total(&self, data: &[u8]) -> Result<usize, DecoderError> {
        match self.channels {
            Channels::Mono => self.get_nb_samples(data),
            Channels::Stereo => Ok(self.get_nb_samples(data)? * 2),
        }
    }

    /**
     * Return the number of samples in the opus data.
     *
     * This value can be used for audio output when frame size is expected, i.e. the number of
     * samples per channel.
     *
     * See also [`opus_decoder_get_nb_samples`].
     */
    pub fn get_nb_samples(&self, data: &[u8]) -> Result<usize, DecoderError> {
        // SAFETY: The pointer points to a valid slice of data or null if the slice was empty.
        // Length is derived from the input slice
        let samples = unsafe {
            let len = data.len().saturating_as();
            let data = if !data.is_empty() {
                data.as_ptr()
            } else {
                core::ptr::null()
            };
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

    /**
     * Decode opus packet from data into output buffer.
     *
     * Returns decoded frame stored on output buffer. Its length is total number of samples in a
     * frame.
     *
     * ```
     * # use opus_embedded::{Decoder, SamplingRate, Channels};
     * # let data = [0, 0, 0, 0, 0, 0, 0];
     * # let data = data.as_slice();
     * let mut decoder = Decoder::new(SamplingRate::F24k, Channels::Mono).unwrap();
     * let mut output = Vec::new();
     * output.resize(decoder.get_nb_samples_total(data).unwrap(), 0);
     * let output = decoder.decode(data, &mut output).unwrap();
     * println!("Got {} samples of data in output", output.len());
     * ```
     *
     * See also [`opus_decode`].
     */
    pub fn decode<'output>(
        &mut self,
        data: &[u8],
        output: &'output mut [i16],
    ) -> Result<&'output [i16], DecoderError> {
        // SAFETY: The pointers point to valid slices of data or null if their respective slice was
        // empty. Lengths are derived from the respective slices
        let samples = unsafe {
            let len: i32 = data.len().saturating_as();
            let data = if !data.is_empty() {
                data.as_ptr()
            } else {
                core::ptr::null()
            };
            // Let's calculate frame_size that will fit in the output buffer
            let frame_size: i32 = match self.channels {
                Channels::Mono => output.len(),
                Channels::Stereo => output.len() / 2,
            }
            .saturating_as();
            let output = if !output.is_empty() {
                output.as_mut_ptr()
            } else {
                core::ptr::null_mut()
            };
            opus_decode(&mut self.decoder, data, len, output, frame_size, 0)
        };
        if samples < 0 {
            Err(DecoderError {
                error_code: samples,
            })
        } else {
            let frame_size = match self.channels {
                Channels::Mono => samples as usize,
                Channels::Stereo => samples as usize * 2,
            };
            Ok(&output[..frame_size])
        }
    }
}

/// Bandwidth in the opus data.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Bandwidth {
    /// Narrowband data (4 kHz bandpass).
    Narrowband,
    /// Mediumband data (6 kHz bandpass).
    Mediumband,
    /// Wideband data (8 kHz bandpass).
    Wideband,
    /// Superwideband data (12 kHz bandpass).
    Superwideband,
    /// Fullband data (20 kHz bandpass).
    Fullband,
}

/// Wraps opus data into a packet type.
#[derive(Debug)]
pub struct OpusPacket<'data> {
    data: &'data [u8],
}

impl<'data> OpusPacket<'data> {
    /**
     * Construct packet from data.
     *
     * Does not check for validity.
     *
     * See also [`opus_packet_get_nb_channels`].
     *
     * # Panics
     * Panics if data is an empty slice.
     */
    pub fn new(data: &'data [u8]) -> Self {
        assert!(!data.is_empty());
        Self { data }
    }

    /// Return the number of channels for the packet.
    pub fn get_nb_channels(&self) -> Result<u8, InvalidPacket> {
        // SAFETY: The pointer points to a valid slice of data, and the size is not zero
        let channels = unsafe {
            let data = self.data.as_ptr();
            opus_packet_get_nb_channels(data)
        };
        if channels < 0 {
            debug_assert_eq!(channels, OPUS_INVALID_PACKET);
            Err(InvalidPacket {})
        } else {
            Ok(channels.saturating_as())
        }
    }

    /**
     * Return the number of frames for the packet.
     *
     * See also [`opus_packet_get_nb_frames`].
     */
    pub fn get_nb_frames(&self) -> Result<u32, InvalidPacket> {
        // SAFETY: The pointer points to a valid slice of data, the length is derived from the
        // slice and the slice is not empty
        let frames = unsafe {
            let len = self.data.len().saturating_as();
            let data = self.data.as_ptr();
            opus_packet_get_nb_frames(data, len)
        };
        if frames < 0 {
            debug_assert_eq!(frames, OPUS_INVALID_PACKET);
            Err(InvalidPacket {})
        } else {
            Ok(frames.saturating_as())
        }
    }

    /**
     * Return the bandwidth of the packet.
     *
     * See also [`opus_packet_get_bandwidth`].
     */
    pub fn get_bandwidth(&self) -> Result<Bandwidth, InvalidPacket> {
        // SAFETY: The pointer points to a valid slice of data, and the size is not zero
        let bandwidth = unsafe {
            let data = self.data.as_ptr();
            opus_packet_get_bandwidth(data)
        };
        if bandwidth < 0 {
            debug_assert_eq!(bandwidth, OPUS_INVALID_PACKET);
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

    /**
     * Return the number of sampels per frame in the packet.
     *
     * See also [`opus_packet_get_samples_per_frame`].
     */
    pub fn get_samples_per_frame(&self) -> Result<u32, InvalidPacket> {
        // SAFETY: The pointer points to a valid slice of data, the length is derived from the
        // slice and the slice is not empty
        let samples = unsafe {
            let len = self.data.len().saturating_as();
            let data = self.data.as_ptr();
            opus_packet_get_samples_per_frame(data, len)
        };
        if samples < 0 {
            debug_assert_eq!(samples, OPUS_INVALID_PACKET);
            Err(InvalidPacket {})
        } else {
            Ok(samples.saturating_as())
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;
    use alloc::string::ToString;
    use core::error::Error;

    #[test]
    fn create_decoder() {
        let decoder = Decoder::new(SamplingRate::F8k, Channels::Mono);
        assert!(decoder.is_ok());
    }

    #[test]
    fn create_decoder_stereo() {
        let decoder = Decoder::new(SamplingRate::F16k, Channels::Stereo);
        if cfg!(feature = "stereo") {
            assert!(decoder.is_ok());
        } else {
            assert!(decoder.is_err());
            assert_eq!(decoder.unwrap_err().numeric(), OPUS_ALLOC_FAIL);
        }
    }

    #[test]
    fn sampling_rate() {
        assert_eq!(SamplingRate::closest(8_000), SamplingRate::F8k);
        assert_eq!(SamplingRate::closest(12_000), SamplingRate::F12k);
        assert_eq!(SamplingRate::closest(16_000), SamplingRate::F16k);
        assert_eq!(SamplingRate::closest(24_000), SamplingRate::F24k);
        assert_eq!(SamplingRate::closest(48_000), SamplingRate::F48k);
    }

    #[test]
    fn sampling_rate_from_primitive() {
        assert!(SamplingRate::try_from(1_000).is_err());
        assert!(SamplingRate::try_from(8_000).is_ok());
        assert!(SamplingRate::try_from(12_000).is_ok());
        assert!(SamplingRate::try_from(16_000).is_ok());
        assert!(SamplingRate::try_from(24_000).is_ok());
        assert!(SamplingRate::try_from(48_000).is_ok());
        assert!(SamplingRate::try_from(64_000).is_err());
    }

    #[test]
    fn channels_from_primitive() {
        assert!(Channels::try_from(0).is_err());
        assert!(Channels::try_from(1).is_ok());
        assert!(Channels::try_from(2).is_ok());
        for channels in 3..=255 {
            assert!(Channels::try_from(channels).is_err());
        }
    }

    #[test]
    fn test_decoder_with_zero_length_packet() {
        // NB: Error strings depend on libopus internal error messages.
        const DATA: [u8; 0] = [0u8; 0];
        let mut decoder = Decoder::new(SamplingRate::F8k, Channels::Mono).unwrap();
        let result = decoder.get_nb_samples(&DATA);
        assert_eq!(
            result,
            Err(DecoderError {
                error_code: OPUS_BAD_ARG
            })
        );
        let error = result.unwrap_err();
        assert_eq!(error.numeric(), OPUS_BAD_ARG);
        assert!(error.source().is_none());
        assert_eq!(error.to_string(), "invalid argument");
        // Passing empty slice (-> null) is a valid input for decoding
        let mut output = [0i16; 100];
        let result = decoder.decode(&DATA, &mut output);
        assert_eq!(result.unwrap().len(), output.len());
        for v in output {
            assert_eq!(v, 0);
        }
        // However empty slice for output is not
        let mut output = [0i16; 0];
        let result = decoder.decode(&[0, 0, 0, 0, 0], &mut output);
        assert_eq!(
            result,
            Err(DecoderError {
                error_code: OPUS_BAD_ARG
            })
        );
        let error = result.unwrap_err();
        assert_eq!(error.numeric(), OPUS_BAD_ARG);
        assert!(error.source().is_none());
        assert_eq!(error.to_string(), "invalid argument");
    }

    #[test]
    fn test_decoder_with_zero_packet() {
        const DATA: [u8; 8] = [0x00u8; 8];
        let mut decoder = Decoder::new(SamplingRate::F8k, Channels::Mono).unwrap();
        assert_eq!(decoder.get_nb_samples(&DATA), Ok(80));
        let mut output = [0i16; 80];
        assert_eq!(decoder.decode(&DATA, &mut output).unwrap().len(), 80);
    }

    #[test]
    fn test_decoder_with_0xff_packet() {
        const DATA: [u8; 8] = [0xffu8; 8];
        let mut decoder = Decoder::new(SamplingRate::F8k, Channels::Mono).unwrap();
        let result = decoder.get_nb_samples(&DATA);
        assert_eq!(
            result,
            Err(DecoderError {
                error_code: OPUS_INVALID_PACKET
            })
        );
        let error = result.unwrap_err();
        assert_eq!(error.numeric(), OPUS_INVALID_PACKET);
        assert!(error.source().is_none());
        assert_eq!(error.to_string(), "corrupted stream");
        let mut output = [0i16; 80];
        let result = decoder.decode(&DATA, &mut output);
        assert_eq!(
            result,
            Err(DecoderError {
                error_code: OPUS_INVALID_PACKET
            })
        );
        let error = result.unwrap_err();
        assert_eq!(error.numeric(), OPUS_INVALID_PACKET);
        assert!(error.source().is_none());
        assert_eq!(error.to_string(), "corrupted stream");
    }

    #[test]
    #[should_panic]
    fn test_zero_length_packet() {
        const DATA: [u8; 0] = [0u8; 0];
        let _packet = OpusPacket::new(&DATA);
    }

    #[test]
    fn test_zero_packet() {
        let data = [0x00u8; 8];
        let packet = OpusPacket::new(&data);
        assert_eq!(packet.get_nb_channels(), Ok(1));
        assert_eq!(packet.get_nb_frames(), Ok(1));
        assert_eq!(packet.get_bandwidth(), Ok(Bandwidth::Narrowband));
        assert_eq!(packet.get_samples_per_frame(), Ok(0));
    }

    #[test]
    fn test_0xff_packet() {
        let data = [0xFFu8; 8];
        let packet = OpusPacket::new(&data);
        assert_eq!(packet.get_nb_channels(), Ok(2));
        assert_eq!(packet.get_nb_frames(), Ok(63));
        assert_eq!(packet.get_bandwidth(), Ok(Bandwidth::Fullband));
        assert_eq!(packet.get_samples_per_frame(), Ok(0));
    }

    #[test]
    fn test_one_length_packet() {
        // Something that returns OPUS_INVALID_PACKET
        let packet = OpusPacket::new(&[0xff]);
        assert_eq!(packet.get_nb_channels(), Ok(2));
        let result = packet.get_nb_frames();
        assert_eq!(result, Err(InvalidPacket {}));
        let error = result.unwrap_err();
        assert_eq!(error.numeric(), OPUS_INVALID_PACKET);
        assert!(error.source().is_none());
        assert_eq!(error.to_string(), "corrupted stream");
        assert_eq!(packet.get_bandwidth(), Ok(Bandwidth::Fullband));
        assert_eq!(packet.get_samples_per_frame(), Ok(0));
    }

    #[test]
    fn test_packet_bandwidths() {
        // Just tests that all values can appear
        let packet = OpusPacket::new(&[0x00]);
        assert_eq!(packet.get_bandwidth(), Ok(Bandwidth::Narrowband));
        let packet = OpusPacket::new(&[0x20]);
        assert_eq!(packet.get_bandwidth(), Ok(Bandwidth::Mediumband));
        let packet = OpusPacket::new(&[0xB0]);
        assert_eq!(packet.get_bandwidth(), Ok(Bandwidth::Wideband));
        let packet = OpusPacket::new(&[0xC0]);
        assert_eq!(packet.get_bandwidth(), Ok(Bandwidth::Superwideband));
        let packet = OpusPacket::new(&[0xF0]);
        assert_eq!(packet.get_bandwidth(), Ok(Bandwidth::Fullband));
    }
}
