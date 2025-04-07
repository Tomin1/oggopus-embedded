/*
 * Copyright (c) 2025 Tomi Leppänen
 * SPDX-License-Identifier: BSD-3-Clause
 */
/*!
 * Opus parsing code.
 */

use core::num::NonZeroUsize;
use nom::{bytes::complete::tag, error::ErrorKind, number, Parser};

/// Errors from parsing opus data.
#[derive(Debug, PartialEq)]
pub enum OpusError {
    /// Parsing error from nom library.
    ParsingError(ErrorKind),
    /// Stream ended abruptly.
    EndOfStreamError(Option<NonZeroUsize>),
    /// Stream was not a valid opus stream.
    InvalidStream(&'static str),
    /// Stream is not an opus stream but something else.
    NotOpusStream,
}

impl core::fmt::Display for OpusError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use OpusError::*;
        match self {
            ParsingError(kind) => f.write_fmt(format_args!(
                "parsing error with Opus: {}",
                kind.description()
            ))?,
            EndOfStreamError(Some(size)) => f.write_fmt(format_args!(
                "Opus stream ended abruptly with {} more bytes needed",
                size
            ))?,
            EndOfStreamError(None) => f.write_fmt(format_args!("Opus stream ended abruptly"))?,
            InvalidStream(issue) => f.write_fmt(format_args!("invalid stream: {}", issue))?,
            NotOpusStream => f.write_str("this is not an Opus stream")?,
        };
        Ok(())
    }
}

impl core::error::Error for OpusError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

impl<'data> From<nom::Err<(&'data [u8], ErrorKind)>> for OpusError {
    fn from(error: nom::Err<(&'data [u8], ErrorKind)>) -> OpusError {
        use OpusError::*;
        match error {
            nom::Err::Failure((_, kind)) => ParsingError(kind),
            nom::Err::Error((_, kind)) => ParsingError(kind),
            nom::Err::Incomplete(nom::Needed::Size(size)) => EndOfStreamError(Some(size)),
            nom::Err::Incomplete(nom::Needed::Unknown) => EndOfStreamError(None),
        }
    }
}

/// Result for opus parsing errors.
pub type Result<'data, O> = core::result::Result<O, OpusError>;

/// Channel mapping of opus stream.
#[derive(Debug, PartialEq)]
pub enum ChannelMapping {
    /**
     * Family 0 channel mapping.
     *
     * Supports mono and stereo audio.
     */
    Family0 {
        /// The number of channels, which can be 1 (mono) or 2 (stereo).
        channels: u8,
    },
    /**
     * Family 1 channel mapping.
     *
     * This is missing channel mapping table member.
     *
     * Currently not supported.
     */
    Family1 {
        // TODO: Add the table
        /// The number of channels, which can be between 1 and 8.
        channels: u8,
    },
    /**
     * Family 255 channel mapping.
     *
     * Currently not supported.
     */
    Family255,
    /**
     * Reserved channel mapping value was used in the stream.
     *
     * Currently not supported.
     */
    Reserved,
}

/// Opus header data.
#[derive(Debug, PartialEq)]
pub struct OpusHeader {
    /// Opus version.
    pub version: u8,
    /// Channel mapping.
    pub channels: ChannelMapping,
    /// The number of samples to skip in the beginning of the stream.
    pub pre_skip: u16,
    /// Sample rate used for the original audio.
    pub sample_rate: u32,
    /// Output gain.
    pub output_gain: u16,
}

impl OpusHeader {
    /**
     * Parse opus header from input data.
     *
     * # Panics
     * Will panic when unsupported channel mapping is encountered.
     */
    pub fn parse(input: &[u8]) -> Result<Self> {
        use OpusError::*;
        let (input, _) = tag(b"OpusHead".as_slice())(input)
            .map_err(|_: nom::Err<(&[u8], ErrorKind)>| NotOpusStream)?;
        let (input, version) = number::u8().parse(input)?;
        let (input, channels) = number::u8().parse(input)?;
        let (input, pre_skip) = number::le_u16().parse(input)?;
        let (input, sample_rate) = number::le_u32().parse(input)?;
        let (input, output_gain) = number::le_u16().parse(input)?;
        let (_channel_mapping_table, channel_mapping_family) = number::u8().parse(input)?;
        let channels = match channel_mapping_family {
            0 => match channels {
                1..=2 => ChannelMapping::Family0 { channels },
                _ => return Err(InvalidStream("bad number of channels for family 0")),
            },
            1 => match channels {
                1..=8 => todo!(),
                _ => return Err(InvalidStream("bad number of channels for family 1")),
            },
            255 => todo!(),
            _ => todo!(),
        };
        Ok(Self {
            version,
            channels,
            pre_skip,
            sample_rate,
            output_gain,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use core::error::Error;

    #[test]
    fn parse_header() {
        let data = include_bytes!("test/opus.data");
        let page = OpusHeader::parse(data).unwrap();
        assert_eq!(page.version, 1);
        if let ChannelMapping::Family0 { channels } = page.channels {
            assert_eq!(channels, 1);
        } else {
            panic!("Channel mapping family must be 0");
        }
        assert_eq!(page.pre_skip, 312);
        assert_eq!(page.sample_rate, 8_000);
        assert_eq!(page.output_gain, 0);
    }

    #[test]
    fn corrupted_stream() {
        let mut data = Vec::from(include_bytes!("test/opus.data"));
        data[2] = 10;
        let result = OpusHeader::parse(&data);
        assert_eq!(result, Err(OpusError::NotOpusStream));
        assert!(result.unwrap_err().source().is_none());
    }

    #[test]
    fn incomplete_header() {
        let data = include_bytes!("test/opus.data");
        let result = OpusHeader::parse(&data[..10]);
        assert_eq!(
            result,
            Err(OpusError::EndOfStreamError(Some(2.try_into().unwrap())))
        );
        assert!(result.unwrap_err().source().is_none());
    }

    #[test]
    fn invalid_channels() {
        let mut data = Vec::from(include_bytes!("test/opus.data"));
        data[9] = 0;
        let result = OpusHeader::parse(&data);
        assert_eq!(
            result,
            Err(OpusError::InvalidStream(
                "bad number of channels for family 0"
            ))
        );
        assert!(result.unwrap_err().source().is_none());
        data[0x12] = 1;
        let result = OpusHeader::parse(&data);
        assert_eq!(
            result,
            Err(OpusError::InvalidStream(
                "bad number of channels for family 1"
            ))
        );
        assert!(result.unwrap_err().source().is_none());
    }
}
