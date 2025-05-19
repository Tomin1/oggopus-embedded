/*
 * Copyright (c) 2025 Tomi Leppänen
 * SPDX-License-Identifier: BSD-3-Clause
 */
//! Opus parsing code.

use core::num::NonZeroUsize;
use nom::{bytes::complete::tag, error::ErrorKind, number, Parser};

/// Error values for formatting.
#[derive(Debug, PartialEq)]
#[doc(hidden)]
#[non_exhaustive]
pub enum ErrorValues {
    ZeroStreamCount,
    BadNumberOfChannels(u8, u8),
    InvalidChannelIndex(u8),
    TotalStreamCountExceeds(u16),
    StreamCountsMismatch(u8, u8),
    BadTableLength(usize, u8),
    TableTooBig(usize, u8),
}

/// Errors from parsing opus data.
#[derive(Debug, PartialEq)]
pub enum OpusError {
    /// Parsing error from nom library.
    ParsingError(ErrorKind),
    /// Stream ended abruptly.
    EndOfStreamError(Option<NonZeroUsize>),
    /// Stream is not a valid opus stream, e.g. it has a value outside specifications.
    InvalidStream(ErrorValues),
    /// Stream is not supported. Enabled features may affect this.
    UnsupportedStream(&'static str),
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
            InvalidStream(issue) => match issue {
                ErrorValues::ZeroStreamCount => {
                    f.write_str("stream count cannot be 0")?
                }
                ErrorValues::BadNumberOfChannels(family, channels) => f.write_fmt(format_args!(
                    "bad number of channels for family {}: {}",
                    family, channels
                ))?,
                ErrorValues::InvalidChannelIndex(index) => f.write_fmt(format_args!(
                    "invalid channel index in channel mapping table: {}",
                    index
                ))?,
                ErrorValues::TotalStreamCountExceeds(total) => f.write_fmt(format_args!(
                    "total stream count cannot be more than 255: {}",
                    total
                ))?,
                ErrorValues::StreamCountsMismatch(coupled_count, stream_count) => f.write_fmt(format_args!(
                    "coupled stream count ({}) cannot be larger than stream count ({})",
                    coupled_count, stream_count
                ))?,
                ErrorValues::BadTableLength(length, channels) => f.write_fmt(format_args!(
                    "channel mapping table length does not match the number of channels ({}): {}",
                    channels, length
                ))?,
                ErrorValues::TableTooBig(length, max_size) => f.write_fmt(format_args!(
                    "channel mapping table does not fit to reserved space ({}), it is {} bytes long",
                    max_size, length,
                ))?,
            },
            UnsupportedStream(issue) => {
                f.write_fmt(format_args!("unsupported stream: {}", issue))?
            }
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
#[non_exhaustive]
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
     * Vorbis channel order.
     */
    Family1 {
        /// The number of channels, which can be between 1 and 8.
        channels: u8,
        /// Channel mapping table.
        table: ChannelMappingTable<8>,
    },
    #[cfg_attr(docsrs, doc(cfg(feature = "family255")))]
    #[cfg(feature = "family255")]
    /**
     * Family 255 channel mapping.
     *
     * Channels are unidentified.
     */
    Family255 {
        /// The number of channels.
        channels: u8,
        /// Channel mapping table.
        table: ChannelMappingTable<255>,
    },
    #[cfg_attr(docsrs, doc(cfg(feature = "family255")))]
    #[cfg(feature = "family255")]
    /**
     * Reserved channel mapping value was used in the stream.
     *
     * Such mapping may be a future extension to the container format.
     */
    Reserved {
        /// The number of channels.
        channels: u8,
        /// Channel mapping table.
        table: ChannelMappingTable<255>,
    },
}

impl ChannelMapping {
    /// Get channel count.
    pub fn get_channel_count(&self) -> u8 {
        use ChannelMapping::*;
        match self {
            Family0 { channels } => *channels,
            Family1 { channels, .. } => *channels,
            #[cfg(feature = "family255")]
            Family255 { channels, .. } => *channels,
            #[cfg(feature = "family255")]
            Reserved { channels, .. } => *channels,
        }
    }

    /// Get stream count.
    pub fn get_stream_count(&self) -> u8 {
        use ChannelMapping::*;
        match self {
            Family0 { .. } => 1,
            Family1 { table, .. } => table.stream_count,
            #[cfg(feature = "family255")]
            Family255 { table, .. } => table.stream_count,
            #[cfg(feature = "family255")]
            Reserved { table, .. } => table.stream_count,
        }
    }

    /// Get coupled stream count.
    pub fn get_coupled_stream_count(&self) -> u8 {
        use ChannelMapping::*;
        match self {
            Family0 { channels } => *channels - 1,
            Family1 { table, .. } => table.coupled_count,
            #[cfg(feature = "family255")]
            Family255 { table, .. } => table.coupled_count,
            #[cfg(feature = "family255")]
            Reserved { table, .. } => table.coupled_count,
        }
    }

    /**
     * Get channel mapping for given channel index.
     *
     * Returns [`None`] for invalid channel indexes.
     */
    pub fn get_mapping(&self, channel: u8) -> Option<Mapping> {
        use ChannelMapping::*;
        let (speaker_location, index, coupled_count) = match self {
            Family0 { channels } => {
                let speaker_location = match (*channels, channel) {
                    (1, 0) => SpeakerLocation::Mono,
                    (2, 0) => SpeakerLocation::Left,
                    (2, 1) => SpeakerLocation::Right,
                    _ => return None,
                };
                Some((Some(speaker_location), channel, *channels - 1))
            }
            Family1 { channels, table } => {
                let speaker_location = match (*channels, channel) {
                    (1, 0) => SpeakerLocation::Mono,
                    (2..=8, 0) => SpeakerLocation::Left,
                    (2, 1) | (3, 2) | (4, 1) | (5..=8, 2) => SpeakerLocation::Right,
                    (3, 1) | (5..=8, 1) => SpeakerLocation::Center,
                    (4, 2) | (5..=6, 3) | (8, 5) => SpeakerLocation::RearLeft,
                    (4, 3) | (5..=6, 4) | (8, 6) => SpeakerLocation::RearRight,
                    (6, 5) | (7, 6) | (8, 7) => SpeakerLocation::LFE,
                    (7..=8, 3) => SpeakerLocation::SideLeft,
                    (7..=8, 4) => SpeakerLocation::SideRight,
                    (7, 5) => SpeakerLocation::RearCenter,
                    _ => return None,
                };
                let ChannelMappingTable {
                    coupled_count,
                    mapping,
                    ..
                } = &table;
                let index = mapping[usize::from(channel)];
                Some((Some(speaker_location), index, *coupled_count))
            }
            #[cfg(feature = "family255")]
            Family255 { channels, table } | Reserved { channels, table } => {
                if channel >= *channels {
                    None
                } else {
                    let ChannelMappingTable {
                        coupled_count,
                        mapping,
                        ..
                    } = &table;
                    let index = mapping[usize::from(channel)];
                    Some((None, index, *coupled_count))
                }
            }
        }?;
        if index == 255 {
            Some(Mapping {
                stream: None,
                speaker_location,
            })
        } else {
            let stream = if index < 2 * coupled_count {
                if index % 2 == 0 {
                    (index / 2, DecodedChannel::Left)
                } else {
                    (index / 2, DecodedChannel::Right)
                }
            } else {
                (index - coupled_count, DecodedChannel::Mono)
            };
            let stream = Some(stream);
            Some(Mapping {
                stream,
                speaker_location,
            })
        }
    }
}

/// Speaker location in audio setup.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum SpeakerLocation {
    /// Mono speaker.
    Mono,
    /// Left channel of a stereo stream. Front left in surround setup.
    Left,
    /// Right channel of a stereo stream. Front right in surround setup.
    Right,
    /// Center channel in linear surround setup. Front center in surround setup.
    Center,
    /// Rear left channel of a surround setup.
    RearLeft,
    /// Rear right channel of a surround setup.
    RearRight,
    /// Rear center channel of a surround setup.
    RearCenter,
    /// Side left channel of a surround setup.
    SideLeft,
    /// Side right channel of a surround setup.
    SideRight,
    /// Low-frequency effects channel of a surround setup.
    LFE,
}

/// Decoded opus channel to use for this audio channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DecodedChannel {
    /// The opus stream will be mono audio.
    Mono,
    /// Use the first decoded channel of the stereo stream.
    Left,
    /// Use the second decoded channel of the stereo stream.
    Right,
}

/// Channel mapping for an audio channel.
#[derive(Debug, PartialEq)]
pub struct Mapping {
    /// Stream index and decoded opus stream channel to use. Silent channel if [`None`].
    pub stream: Option<(u8, DecodedChannel)>,
    /// Speaker location for this index if available.
    pub speaker_location: Option<SpeakerLocation>,
}

/// Channel mapping table.
#[derive(Debug, PartialEq)]
pub struct ChannelMappingTable<const MAX_CHANNELS: usize> {
    /// The number of total streams encoded in each Ogg packet.
    stream_count: u8,
    /// The number of stereo decoders needed.
    coupled_count: u8,
    /// Channel mapping data.
    mapping: [u8; MAX_CHANNELS],
}

impl<const MAX_CHANNELS: usize> ChannelMappingTable<MAX_CHANNELS> {
    fn parse(input: &[u8], channels: u8) -> Result<ChannelMappingTable<MAX_CHANNELS>> {
        use OpusError::*;
        let (input, stream_count) = number::u8().parse(input)?;
        let (input, coupled_count) = number::u8().parse(input)?;
        let total_stream_count = stream_count.checked_add(coupled_count).ok_or_else(|| {
            let total = stream_count as u16 + coupled_count as u16;
            InvalidStream(ErrorValues::TotalStreamCountExceeds(total))
        })?;
        if stream_count == 0 {
            Err(InvalidStream(ErrorValues::ZeroStreamCount))
        } else if coupled_count > stream_count {
            Err(InvalidStream(ErrorValues::StreamCountsMismatch(
                coupled_count,
                stream_count,
            )))
        } else if input.len() != channels.into() {
            Err(InvalidStream(ErrorValues::BadTableLength(
                input.len(),
                channels,
            )))
        } else if input.len() > MAX_CHANNELS {
            Err(InvalidStream(ErrorValues::TableTooBig(
                input.len(),
                MAX_CHANNELS as u8, // At most 255
            )))
        } else {
            let mut mapping = [0; MAX_CHANNELS];
            for (i, &v) in input.iter().enumerate() {
                if v >= total_stream_count && v != 255 {
                    return Err(InvalidStream(ErrorValues::InvalidChannelIndex(v)));
                }
                mapping[i] = v;
            }
            Ok(ChannelMappingTable {
                stream_count,
                coupled_count,
                mapping,
            })
        }
    }
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
     * May return [`UnsupportedStream`][`OpusError::UnsupportedStream`] if family255 feature has
     * not been enabled and such stream is encountered.
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
        let (channel_mapping_table, channel_mapping_family) = number::u8().parse(input)?;
        let channels = match channel_mapping_family {
            0 => match channels {
                1..=2 => ChannelMapping::Family0 { channels },
                _ => return Err(InvalidStream(ErrorValues::BadNumberOfChannels(0, channels))),
            },
            1 => match channels {
                1..=8 => ChannelMapping::Family1 {
                    channels,
                    table: ChannelMappingTable::parse(channel_mapping_table, channels)?,
                },
                _ => return Err(InvalidStream(ErrorValues::BadNumberOfChannels(1, channels))),
            },
            #[cfg(feature = "family255")]
            255 => ChannelMapping::Family255 {
                channels,
                table: ChannelMappingTable::parse(channel_mapping_table, channels)?,
            },
            #[cfg(feature = "family255")]
            _ => ChannelMapping::Reserved {
                channels,
                table: ChannelMappingTable::parse(channel_mapping_table, channels)?,
            },
            #[cfg(not(feature = "family255"))]
            _ => {
                return Err(UnsupportedStream(
                    "family 255 channel mapping is not supported",
                ))
            }
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
        let header = OpusHeader::parse(data).unwrap();
        assert_eq!(header.version, 1);
        if let ChannelMapping::Family0 { channels } = header.channels {
            assert_eq!(channels, 1);
        } else {
            panic!("Channel mapping family must be 0");
        }
        assert_eq!(header.pre_skip, 312);
        assert_eq!(header.sample_rate, 8_000);
        assert_eq!(header.output_gain, 0);
    }

    #[test]
    fn family_0_mono() {
        let channels = ChannelMapping::Family0 { channels: 1 };
        assert_eq!(channels.get_channel_count(), 1);
        assert_eq!(channels.get_stream_count(), 1);
        assert_eq!(channels.get_coupled_stream_count(), 0);
        let mapping = channels.get_mapping(0).unwrap();
        assert_eq!(mapping.stream, Some((0, DecodedChannel::Mono)));
        assert_eq!(mapping.speaker_location, Some(SpeakerLocation::Mono));
        for index in 1..=255 {
            assert_eq!(channels.get_mapping(index), None);
        }
    }

    #[test]
    fn family_0_stereo() {
        let channels = ChannelMapping::Family0 { channels: 2 };
        assert_eq!(channels.get_channel_count(), 2);
        assert_eq!(channels.get_stream_count(), 1);
        assert_eq!(channels.get_coupled_stream_count(), 1);
        let mapping = channels.get_mapping(0).unwrap();
        assert_eq!(mapping.stream, Some((0, DecodedChannel::Left)));
        assert_eq!(mapping.speaker_location, Some(SpeakerLocation::Left));
        let mapping = channels.get_mapping(1).unwrap();
        assert_eq!(mapping.stream, Some((0, DecodedChannel::Right)));
        assert_eq!(mapping.speaker_location, Some(SpeakerLocation::Right));
        for index in 2..=255 {
            assert_eq!(channels.get_mapping(index), None);
        }
    }

    #[test]
    fn family_1_stereo() {
        let channels = ChannelMapping::Family1 {
            channels: 2,
            table: ChannelMappingTable::parse(&[1, 1, 0, 1], 2).unwrap(),
        };
        assert_eq!(channels.get_channel_count(), 2);
        assert_eq!(channels.get_stream_count(), 1);
        assert_eq!(channels.get_coupled_stream_count(), 1);
        let mapping = channels.get_mapping(0).unwrap();
        assert_eq!(mapping.stream, Some((0, DecodedChannel::Left)));
        assert_eq!(mapping.speaker_location, Some(SpeakerLocation::Left));
        let mapping = channels.get_mapping(1).unwrap();
        assert_eq!(mapping.stream, Some((0, DecodedChannel::Right)));
        assert_eq!(mapping.speaker_location, Some(SpeakerLocation::Right));
        for index in 2..=255 {
            assert_eq!(channels.get_mapping(index), None);
        }
    }

    #[test]
    fn family_1_surround_5_1() {
        let data: [u8; 0x08] = [0x04, 0x02, 0x00, 0x04, 0x01, 0x02, 0x03, 0x05];
        let channels = ChannelMapping::Family1 {
            channels: 6,
            table: ChannelMappingTable::parse(&data, 6).unwrap(),
        };
        assert_eq!(channels.get_channel_count(), 6);
        assert_eq!(channels.get_stream_count(), 4);
        assert_eq!(channels.get_coupled_stream_count(), 2);
        let mappings = [
            (0, DecodedChannel::Left, SpeakerLocation::Left),
            (2, DecodedChannel::Mono, SpeakerLocation::Center),
            (0, DecodedChannel::Right, SpeakerLocation::Right),
            (1, DecodedChannel::Left, SpeakerLocation::RearLeft),
            (1, DecodedChannel::Right, SpeakerLocation::RearRight),
            (3, DecodedChannel::Mono, SpeakerLocation::LFE),
        ];
        for (index, (stream, channel, location)) in mappings.iter().enumerate() {
            let mapping = channels.get_mapping(index as u8).unwrap();
            assert_eq!(mapping.stream, Some((*stream, *channel)));
            assert_eq!(mapping.speaker_location, Some(*location));
        }
        for index in 6..=255 {
            assert_eq!(channels.get_mapping(index), None);
        }
    }

    #[test]
    #[cfg(feature = "family255")]
    fn family_255() {
        let data: [u8; 0x08] = [0x04, 0x02, 0x00, 0x04, 0x01, 0x02, 0x03, 0x05];
        let channels = ChannelMapping::Family255 {
            channels: 6,
            table: ChannelMappingTable::parse(&data, 6).unwrap(),
        };
        assert_eq!(channels.get_channel_count(), 6);
        assert_eq!(channels.get_stream_count(), 4);
        assert_eq!(channels.get_coupled_stream_count(), 2);
        let mappings = [
            (0, DecodedChannel::Left),
            (2, DecodedChannel::Mono),
            (0, DecodedChannel::Right),
            (1, DecodedChannel::Left),
            (1, DecodedChannel::Right),
            (3, DecodedChannel::Mono),
        ];
        for (index, (stream, channel)) in mappings.iter().enumerate() {
            let mapping = channels.get_mapping(index as u8).unwrap();
            assert_eq!(mapping.stream, Some((*stream, *channel)));
            assert_eq!(mapping.speaker_location, None);
        }
        for index in 6..=255 {
            assert_eq!(channels.get_mapping(index), None);
        }
    }

    #[test]
    #[cfg(feature = "family255")]
    fn family_reserved() {
        let data: [u8; 0x06] = [0x02, 0x01, 0x01, 0x00, 0x02, 0xFF];
        let channels = ChannelMapping::Reserved {
            channels: 4,
            table: ChannelMappingTable::parse(&data, 4).unwrap(),
        };
        assert_eq!(channels.get_channel_count(), 4);
        assert_eq!(channels.get_stream_count(), 2);
        assert_eq!(channels.get_coupled_stream_count(), 1);
        let mappings = [
            Some((0, DecodedChannel::Right)),
            Some((0, DecodedChannel::Left)),
            Some((1, DecodedChannel::Mono)),
            None,
        ];
        for (index, stream) in mappings.iter().enumerate() {
            let mapping = channels.get_mapping(index as u8).unwrap();
            assert_eq!(mapping.stream, *stream);
            assert_eq!(mapping.speaker_location, None);
        }
        for index in 4..=255 {
            assert_eq!(channels.get_mapping(index), None);
        }
    }

    #[test]
    fn invalid_stream_count() {
        let data: [u8; 0x08] = [0x04, 0x05, 0x00, 0x04, 0x01, 0x02, 0x03, 0x05];
        let result = ChannelMappingTable::<255>::parse(&data, 6);
        assert_eq!(
            result,
            Err(OpusError::InvalidStream(ErrorValues::StreamCountsMismatch(
                5, 4
            )))
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "coupled stream count (5) cannot be larger than stream count (4)"
        );
    }

    #[test]
    fn zero_stream_count() {
        let data: [u8; 0x08] = [0x00, 0x01, 0x00, 0x04, 0x01, 0x02, 0x03, 0x05];
        let result = ChannelMappingTable::<255>::parse(&data, 6);
        assert_eq!(
            result,
            Err(OpusError::InvalidStream(ErrorValues::ZeroStreamCount))
        );
        assert_eq!(result.unwrap_err().to_string(), "stream count cannot be 0");
    }

    #[test]
    fn invalid_table_length() {
        let data: [u8; 0x08] = [0x04, 0x02, 0x00, 0x04, 0x01, 0x02, 0x03, 0x05];
        let result = ChannelMappingTable::<255>::parse(&data, 2);
        assert_eq!(
            result,
            Err(OpusError::InvalidStream(ErrorValues::BadTableLength(6, 2)))
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "channel mapping table length does not match the number of channels (2): 6"
        );
    }

    #[test]
    fn too_long_table() {
        let data: [u8; 0x08] = [0x04, 0x02, 0x00, 0x04, 0x01, 0x02, 0x03, 0x05];
        let result = ChannelMappingTable::<5>::parse(&data, 6);
        assert_eq!(
            result,
            Err(OpusError::InvalidStream(ErrorValues::TableTooBig(6, 5)))
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "channel mapping table does not fit to reserved space (5), it is 6 bytes long"
        );
    }

    #[test]
    fn invalid_channel_index() {
        let data: [u8; 0x08] = [0x04, 0x02, 0x00, 0xFF, 0x0A, 0x02, 0x03, 0x05];
        let result = ChannelMappingTable::<255>::parse(&data, 6);
        assert_eq!(
            result,
            Err(OpusError::InvalidStream(ErrorValues::InvalidChannelIndex(
                0x0A
            )))
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "invalid channel index in channel mapping table: 10"
        );
    }

    #[test]
    fn too_many_streams() {
        let data: [u8; 0x02] = [0x90, 0x90];
        let result = ChannelMappingTable::<5>::parse(&data, 6);
        assert_eq!(
            result,
            Err(OpusError::InvalidStream(
                ErrorValues::TotalStreamCountExceeds(0x90 * 2)
            ))
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "total stream count cannot be more than 255: 288"
        );
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
            Err(OpusError::InvalidStream(ErrorValues::BadNumberOfChannels(
                0, 0
            )))
        );
        assert!(result.unwrap_err().source().is_none());
        data[0x12] = 1;
        let result = OpusHeader::parse(&data);
        assert_eq!(
            result,
            Err(OpusError::InvalidStream(ErrorValues::BadNumberOfChannels(
                1, 0
            )))
        );
        assert!(result.unwrap_err().source().is_none());
    }
}
