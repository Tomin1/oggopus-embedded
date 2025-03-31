/*
 * Opus parsing code.
 *
 * Copyright (c) 2025 Tomi Leppänen
 * SPDX-License-Identifier: BSD-3-Clause
 */

use core::num::NonZeroUsize;
use nom::{bytes::complete::tag, error::ErrorKind, number, Parser};

#[derive(Debug)]
pub enum OpusError {
    ParsingError(ErrorKind),
    EndOfStreamError(Option<NonZeroUsize>),
    InvalidStream(&'static str),
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
            EndOfStreamError(None) => {
                f.write_fmt(format_args!("Opus stream ended abruptly"))?
            }
            InvalidStream(issue) => f.write_fmt(format_args!("invalid stream: {}", issue))?,
            NotOpusStream => f.write_str("this is not and Opus stream")?,
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

pub type Result<'data, O> = core::result::Result<O, OpusError>;

pub enum ChannelMapping {
    Family0 { channels: u8 },
    Family1 { channels: u8 }, // TODO: Add the table
    Family255,
    Reserved,
}

pub struct OpusHeader {
    pub version: u8,
    pub channels: ChannelMapping,
    pub pre_skip: u16,
    pub sample_rate: u32,
    pub output_gain: u16,
}

impl OpusHeader {
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

    #[test]
    fn parse_header() {
        let data = [
            0x4F, 0x70, 0x75, 0x73, 0x48, 0x65, 0x61, 0x64, // "OpusHead"
            0x01, // version, always 1
            0x01, // number of channels
            0x38, 0x01, // pre skip
            0x80, 0x3E, 0x00, 0x00, // input sample rate
            0x00, 0x00, // output gain
            0x00, // channel mapping family, optionally followed by channel mapping table
        ];
        let result = OpusHeader::parse(&data);
        assert!(result.is_ok());
        let page = result.unwrap();
        assert_eq!(page.version, 1);
        if let ChannelMapping::Family0 { channels } = page.channels {
            assert_eq!(channels, 1);
        } else {
            panic!("Channel mapping family must be 0");
        }
        assert_eq!(page.pre_skip, 312);
        assert_eq!(page.sample_rate, 16_000);
        assert_eq!(page.output_gain, 0);
    }
}
