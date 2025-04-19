/*
 * Copyright (c) 2025 Tomi Leppänen
 * SPDX-License-Identifier: BSD-3-Clause
 */
/*!
 * Small no_std and no_alloc ogg parser for mono and stereo opus audio.
 *
 * While this tries to follow the RFCs to the maximum extent reasonable, this is not suitable as
 * general purpose ogg opus parser and you should never use this for untrusted inputs. This was
 * built for parsing opus data from internal flash as part of an embedded system. You will want to
 * use something else for anything more powerful than that.
 *
 * See also [RFC3533](https://datatracker.ietf.org/doc/html/rfc3533)
 * and [RFC7845](https://datatracker.ietf.org/doc/html/rfc7845).
 *
 * # Limitations
 * - Supports only one logical stream at a time. Grouping is not supported.
 * - Mixing (interleaving or otherwise) other types of streams than opus is not supported.
 * - This parses ID header and ignores comment header.
 * - This does not validate CRC or handle missing packets.
 * - Seeking is not supported.
 * - Parsing of [RFC8486](https://datatracker.ietf.org/doc/html/rfc8486) family channel mappings is not supported.
 */

#![cfg_attr(not(test), no_std)]
#![deny(missing_docs)]

mod container;
pub mod opus;

/// Error from parsing bitstream.
#[derive(Debug, PartialEq)]
pub enum BitstreamError {
    /// Error from parsing ogg container.
    OggError(container::OggError),
    /// Error from parsing opus data within ogg container.
    OpusError(opus::OpusError),
    /// Invalid ogg stream encountered.
    InvalidOggStream(&'static str),
    /// Invalid opus stream encountered.
    InvalidOpusStream(&'static str),
    /// Unsupported opus version encountered. Indicates requested version.
    UnsupportedOpusVersion(u8),
    /// Unsupported ogg opus stream encountered. Enabled features may affect this.
    UnsupportedStream(&'static str),
    /// Stream is not an opus stream but something else.
    NotOpusStream,
}

impl core::fmt::Display for BitstreamError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use BitstreamError::*;
        match self {
            OggError(error) => error.fmt(f),
            OpusError(error) => error.fmt(f),
            InvalidOggStream(error) => f.write_str(error),
            InvalidOpusStream(error) => f.write_str(error),
            UnsupportedOpusVersion(version) => {
                f.write_fmt(format_args!("unsupported Opus version: {}", version))
            }
            UnsupportedStream(error) => f.write_str(error),
            NotOpusStream => f.write_str("this is not an Opus stream"),
        }
    }
}

impl core::error::Error for BitstreamError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        use BitstreamError::*;
        match self {
            OggError(error) => Some(error),
            OpusError(error) => Some(error),
            _ => None,
        }
    }
}

impl From<container::OggError> for BitstreamError {
    fn from(error: container::OggError) -> BitstreamError {
        if let container::OggError::UnsupportedStream(error) = error {
            Self::UnsupportedStream(error)
        } else {
            Self::OggError(error)
        }
    }
}

impl From<opus::OpusError> for BitstreamError {
    fn from(error: opus::OpusError) -> BitstreamError {
        match error {
            opus::OpusError::UnsupportedStream(error) => Self::UnsupportedStream(error),
            opus::OpusError::NotOpusStream => Self::NotOpusStream,
            _ => Self::OpusError(error),
        }
    }
}

/// Result of parsing bitstream.
pub type Result<'data, T> = core::result::Result<T, BitstreamError>;

/// Ogg opus bitstream.
#[derive(Debug)]
pub struct Bitstream<'data> {
    data: &'data [u8],
}

impl<'data> Bitstream<'data> {
    /**
     * Construct new [`Bitstream`] for constant data.
     */
    pub const fn new(data: &'data [u8]) -> Self {
        Self { data }
    }

    /**
     * Create [`BitstreamReader`] to parse [`Bitstream`].
     *
     * Returns [`BitstreamReader`] that is positioned at the beginning of a stream.
     */
    pub fn reader<'bs>(&'bs self) -> BitstreamReader<'bs, 'data, states::Beginning> {
        BitstreamReader::<'bs, 'data, states::Beginning>::new(self)
    }
}

pub mod states {
    //! [`BitstreamReader`][`super::BitstreamReader`] states.

    mod sealed {
        pub trait Sealed {}

        impl Sealed for super::Beginning {}
        impl Sealed for super::InStream {}
        impl Sealed for super::EndOfStream {}
    }

    /// [`BitstreamReader`][`super::BitstreamReader`] is at the beginning of parsing bitstream.
    #[derive(Debug, PartialEq)]
    pub struct Beginning;
    /// [`BitstreamReader`][`super::BitstreamReader`] has parsed headers and is ready to return opus data.
    #[derive(Debug, PartialEq)]
    pub struct InStream {
        // TODO: Allow selecting bitstream
        // TODO: Allow having multiple bitstreams in the same file but reading only one
        /// Serial number of the bitstream.
        pub bitstream_serial_number: u32,
        /// Page sequence number of the last read page.
        pub page_sequence_number: u32,
    }
    /// [`BitstreamReader`][`super::BitstreamReader`] has completed stream parsing.
    #[derive(Debug, PartialEq)]
    pub struct EndOfStream;

    /// State trait for [`BitstreamReader`][`super::BitstreamReader`]. Sealed.
    pub trait ReaderState: sealed::Sealed {}

    impl ReaderState for Beginning {}
    impl ReaderState for InStream {}
    impl ReaderState for EndOfStream {}

    /// Either state may be returned.
    #[derive(Debug, PartialEq)]
    pub enum Either<A, B> {
        /// Parsing can continue.
        Continued(A),
        /// Parsing has reached the end of the stream.
        Ended(B),
    }
}

use states::{Beginning, Either, EndOfStream, InStream, ReaderState};

/// Header with reader for the stream or stream ended.
pub type EitherHeaderOrEnded<'bs, 'data> = (
    Either<BitstreamReader<'bs, 'data, InStream>, BitstreamReader<'bs, 'data, EndOfStream>>,
    opus::OpusHeader,
);

/// Packets with reader for the stream or stream ended.
pub type EitherPacketsOrEnded<'bs, 'data, const BUFFER_SIZE: usize> = (
    Either<BitstreamReader<'bs, 'data, InStream>, BitstreamReader<'bs, 'data, EndOfStream>>,
    container::Packets<'data, BUFFER_SIZE>,
);

/// Reader for [`Bitstream`].
#[derive(Debug, PartialEq)]
pub struct BitstreamReader<'bs, 'data: 'bs, S: ReaderState> {
    bitstream: core::marker::PhantomData<&'bs Bitstream<'data>>,
    remaining: &'data [u8],
    marker: S,
}

impl<S: ReaderState> BitstreamReader<'_, '_, S> {
    /**
     * Construct [`BitstreamReader`] for [`Bitstream`].
     *
     * ```ignore
     * use oggopus_embedded::Bitstream;
     * let stream = Bitstream::new(include_bytes!("audio.opus"));
     * ```
     */
    pub fn new<'bs, 'data>(
        bitstream: &'bs Bitstream<'data>,
    ) -> BitstreamReader<'bs, 'data, Beginning> {
        BitstreamReader {
            bitstream: core::marker::PhantomData::<_>,
            remaining: bitstream.data,
            marker: Beginning,
        }
    }
}

impl<'bs, 'data> BitstreamReader<'bs, 'data, Beginning> {
    /**
     * Read a header packet from [`Bitstream`].
     *
     * Also skips the comments packet and returs [`BitstreamReader`] that can read the following opus packets.
     *
     * ```rust
     * # use oggopus_embedded::{Bitstream, opus::ChannelMapping};
     * # let data = include_bytes!("test/mono.opus");
     * # let stream = Bitstream::new(data);
     * let reader = stream.reader();
     * let (reader, header) = reader.read_header().unwrap();
     * if let ChannelMapping::Family0 { channels } = header.channels {
     *     println!(
     *         "{} channels at {} Hz with pre skip of {}",
     *         channels, header.sample_rate, header.pre_skip
     *     );
     * }
     * ```
     */
    pub fn read_header(self) -> Result<'data, EitherHeaderOrEnded<'bs, 'data>> {
        use BitstreamError::*;
        let BitstreamReader {
            bitstream,
            remaining,
            ..
        } = self;
        let (remaining, mut packets) = container::Packets::<30>::parse(remaining)?;
        let bitstream_serial_number = packets.bitstream_serial_number();
        let page_sequence_number = packets.current_page_sequence_number();
        if page_sequence_number != 0 {
            return Err(InvalidOggStream(
                "unexpected page sequence number in header",
            ));
        }
        if let Some(packet) = packets.next() {
            let header = opus::OpusHeader::parse(packet.data)?;
            if header.version > 15 {
                return Err(UnsupportedOpusVersion(header.version));
            }
            if packets.next().is_some() {
                return Err(InvalidOpusStream("unexpected segment after header"));
            }
            let (remaining, last_page) = container::Page::skip(remaining)?;
            if last_page.bitstream_serial_number() != bitstream_serial_number {
                return Err(UnsupportedStream(
                    "bitstream serial number changed unexpectedly",
                ));
            }
            Ok((
                Either::Continued(BitstreamReader {
                    bitstream,
                    remaining,
                    marker: InStream {
                        bitstream_serial_number,
                        page_sequence_number: last_page.page_sequence_number(),
                    },
                }),
                header,
            ))
        } else {
            Err(InvalidOpusStream("missing header"))
        }
    }
}

impl<'bs, 'data> BitstreamReader<'bs, 'data, InStream> {
    /**
     * Read next packets from Bitstream.
     *
     * Returns also the next [`BitstreamReader`] to read further content.
     *
     * ```rust
     * # use oggopus_embedded::{Bitstream, EitherHeaderOrEnded, EitherPacketsOrEnded, opus::ChannelMapping, states::Either};
     * # let data = include_bytes!("test/mono.opus");
     * # let stream = Bitstream::new(data);
     * # let reader = stream.reader();
     * # let (reader, _header) = reader.read_header().unwrap();
     * # let channels = 1;
     * # let sample_rate = 16_000;
     * let mut reader = match reader {
     *     Either::Continued(reader) => reader,
     *     _ => panic!("No more data"),
     * };
     * loop {
     *     let (new_reader, mut packets) = reader.next_packets::<1_024>().unwrap();
     *     while let Some(packet) = packets.next() {
     *         // Decode or whatever you need to do here
     *         println!("Got {} bytes of opus data", packet.data.len());
     *     }
     *     match new_reader {
     *         Either::Continued(new_reader) => {
     *             // Prepare for the next loop
     *             reader = new_reader;
     *         }
     *         Either::Ended(_reader) => {
     *             break; // You can also expect the next stream to start here
     *         }
     *     }
     * }
     * ```
     */
    pub fn next_packets<const BUFFER_SIZE: usize>(
        &self,
    ) -> Result<'data, EitherPacketsOrEnded<'bs, 'data, BUFFER_SIZE>> {
        use BitstreamError::*;
        let (remaining, packets) = container::Packets::parse(self.remaining)?;
        if self.marker.bitstream_serial_number != packets.bitstream_serial_number() {
            return Err(UnsupportedStream(
                "bitstream serial number changed unexpectedly",
            ));
        }
        if packets.current_page_sequence_number() != self.marker.page_sequence_number + 1 {
            return Err(InvalidOggStream(
                "page sequence numbers are not sequential for data",
            ));
        }
        if !packets.end_of_stream() {
            Ok((
                Either::Continued(BitstreamReader {
                    bitstream: self.bitstream,
                    remaining,
                    marker: InStream {
                        bitstream_serial_number: self.marker.bitstream_serial_number,
                        page_sequence_number: packets.last_page_sequence_number(),
                    },
                }),
                packets,
            ))
        } else {
            Ok((
                Either::Ended(BitstreamReader {
                    bitstream: self.bitstream,
                    remaining,
                    marker: EndOfStream,
                }),
                packets,
            ))
        }
    }
}

impl<'bs, 'data> BitstreamReader<'bs, 'data, EndOfStream> {
    /**
     * Return whether there is more data to read.
     */
    pub fn has_more(&self) -> bool {
        !self.remaining.is_empty()
    }

    /**
     * Get next reader for more data if there is any.
     *
     * ```rust
     * # use oggopus_embedded::{Bitstream, EitherHeaderOrEnded, EitherPacketsOrEnded, opus::ChannelMapping, states::Either};
     * # let data = include_bytes!("test/mono.opus");
     * # let stream = Bitstream::new(data);
     * # let (reader, _header) = stream.reader().read_header().unwrap();
     * # let Either::Continued(mut reader) = reader
     * # else { panic!("Data endded abruptly"); };
     * # let channels = 1;
     * # let sample_rate = 16_000;
     * loop {
     *     let (new_reader, _packets) = reader.next_packets::<1_024>().unwrap();
     *     // ...
     *     match new_reader {
     *         Either::Continued(new_reader) => {
     *             reader = new_reader;
     *         }
     *         Either::Ended(old_reader) => {
     *             if let Some(new_reader) = old_reader.next_reader() {
     *                 // Reinitialize decoding and continue looping
     *                 let (new_reader, header) = new_reader.read_header().unwrap();
     *                 if let Either::Continued(reader) = new_reader {
     *                 }
     *             } else {
     *                 break;
     *             }
     *         }
     *     }
     * }
     */
    pub fn next_reader(self) -> Option<BitstreamReader<'bs, 'data, Beginning>> {
        if self.has_more() {
            Some(BitstreamReader {
                bitstream: core::marker::PhantomData::<_>,
                remaining: self.remaining,
                marker: Beginning,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_mono() {
        const DATA: &[u8] = include_bytes!("test/mono.opus");
        let bitstream = Bitstream::new(DATA);
        let reader = bitstream.reader();
        let (_either, header) = reader.read_header().unwrap();
        assert_eq!(
            header.channels,
            opus::ChannelMapping::Family0 { channels: 1 }
        );
    }

    #[test]
    fn parse_stereo() {
        const DATA: &[u8] = include_bytes!("test/stereo.opus");
        let bitstream = Bitstream::new(DATA);
        let reader = bitstream.reader();
        let (_either, header) = reader.read_header().unwrap();
        assert_eq!(
            header.channels,
            opus::ChannelMapping::Family0 { channels: 2 }
        );
    }

    #[test]
    fn parse_vorbis() {
        const DATA: &[u8] = include_bytes!("test/vorbis.ogg");
        let bitstream = Bitstream::new(DATA);
        let reader = bitstream.reader();
        assert_eq!(
            reader.read_header(),
            Err(BitstreamError::NotOpusStream)
        );
    }
}
