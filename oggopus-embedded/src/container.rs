/*
 * Copyright (c) 2025 Tomi Leppänen
 * SPDX-License-Identifier: BSD-3-Clause
 */
//! Ogg parsing code.

use super::ErrorValues;
use bitflags::bitflags;
use core::num::NonZeroUsize;
use nom::{
    bytes::complete::{tag, take},
    error::ErrorKind,
    number, Parser,
};

bitflags! {
    #[derive(Debug, PartialEq)]
    struct HeaderFlags: u8 {
        const Continuation = 0b001;
        const BeginOfStream = 0b010;
        const EndOfStream = 0b100;
    }
}

/// Error from parsing ogg container.
#[derive(Debug, PartialEq)]
pub enum OggError {
    /// Unsupported ogg version.
    UnsupportedVersion(u8),
    /// Parsing error from nom library.
    ParsingError(ErrorKind),
    /// Stream ended abruptly.
    EndOfStreamError(Option<NonZeroUsize>),
    /// Stream did not validate as ogg stream.
    InvalidStream(ErrorValues),
    /// Stream is not supported, e.g. it contains a grouped stream.
    UnsupportedStream(&'static str),
    /// Stream is not ogg stream.
    NotOggStream,
    /**
     * Buffer was too small to contain packet.
     *
     * Contains size of the buffer and how many bytes would have been actually needed.
     */
    BufferTooSmallError(usize, usize),
}

impl core::fmt::Display for OggError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use OggError::*;
        match self {
            UnsupportedVersion(version) => {
                f.write_fmt(format_args!("unsupported Ogg version: {}", version))?
            }
            ParsingError(kind) => f.write_fmt(format_args!(
                "parsing error with Ogg: {}",
                kind.description()
            ))?,
            EndOfStreamError(Some(size)) => f.write_fmt(format_args!(
                "Ogg stream ended abruptly with {} more bytes needed",
                size
            ))?,
            EndOfStreamError(None) => f.write_fmt(format_args!("Ogg stream ended abruptly"))?,
            InvalidStream(error) => {
                f.write_str("invalid stream: ")?;
                error.fmt(f)?;
            }
            UnsupportedStream(error) => {
                f.write_fmt(format_args!("unsupported stream: {}", error))?
            }
            NotOggStream => f.write_str("this is not an Ogg stream")?,
            BufferTooSmallError(got, needed) => f.write_fmt(format_args!(
                "buffer is too small: got {} but needed {}",
                got, needed
            ))?,
        };
        Ok(())
    }
}

impl core::error::Error for OggError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

impl<'data> From<nom::Err<(&'data [u8], ErrorKind)>> for OggError {
    fn from(error: nom::Err<(&'data [u8], ErrorKind)>) -> OggError {
        use OggError::*;
        fn convert(kind: ErrorKind) -> OggError {
            if kind == ErrorKind::Eof {
                EndOfStreamError(None)
            } else {
                ParsingError(kind)
            }
        }
        match error {
            nom::Err::Failure((_, kind)) => convert(kind),
            nom::Err::Error((_, kind)) => convert(kind),
            nom::Err::Incomplete(nom::Needed::Size(size)) => EndOfStreamError(Some(size)),
            nom::Err::Incomplete(nom::Needed::Unknown) => EndOfStreamError(None),
        }
    }
}

pub(crate) type Result<'data, O> = core::result::Result<(&'data [u8], O), OggError>;

#[derive(Debug, PartialEq)]
struct Segment {
    before: usize,
    size: usize,
    complete: bool,
}

#[derive(Debug, PartialEq)]
struct SegmentTableIterator<'data> {
    table: &'data [u8],
    cumulated: usize,
}

impl Iterator for SegmentTableIterator<'_> {
    type Item = Segment;

    fn next(&mut self) -> Option<Self::Item> {
        if self.table.is_empty() {
            None
        } else {
            let mut index = 0;
            let mut size = 0;
            while index < self.table.len() && self.table[index] == 255 {
                size += usize::from(self.table[index]);
                index += 1;
            }
            let complete;
            if index < self.table.len() {
                assert!(self.table[index] != 255);
                size += usize::from(self.table[index]);
                self.table = &self.table[index + 1..];
                complete = true;
            } else {
                self.table = &self.table[0..0];
                complete = false;
            }
            let before = self.cumulated;
            self.cumulated += size;
            Some(Segment {
                before,
                size,
                complete,
            })
        }
    }
}

#[derive(Debug, PartialEq)]
struct PageHeader<'data> {
    version: u8,
    header_type: HeaderFlags,
    _granule_position: u64,
    bitstream_serial_number: u32,
    page_sequence_number: u32,
    segment_table: &'data [u8],
}

impl PageHeader<'_> {
    fn parse(input: &[u8]) -> Result<PageHeader<'_>> {
        use OggError::*;
        let (input, _) = tag(b"OggS".as_slice())(input)
            .map_err(|_: nom::Err<(&[u8], ErrorKind)>| NotOggStream)?;
        let (input, version) = number::u8().parse(input)?;
        let (input, header_type) = number::u8()
            .parse(input)
            .map(|(input, flags)| (input, HeaderFlags::from_bits_retain(flags)))?;
        let (input, granule_position) = number::le_u64().parse(input)?;
        let (input, bitstream_serial_number) = number::le_u32().parse(input)?;
        let (input, page_sequence_number) = number::le_u32().parse(input)?;
        let (input, _crc_checksum) = number::le_u32().parse(input)?;
        let (input, count) = number::u8().parse(input)?;
        let (input, segment_table) = take(count)(input)?;
        Ok((
            input,
            PageHeader {
                version,
                header_type,
                _granule_position: granule_position,
                bitstream_serial_number,
                page_sequence_number,
                segment_table,
            },
        ))
    }

    fn iter_segment_table(data: &[u8]) -> SegmentTableIterator<'_> {
        let (_, header) = PageHeader::parse(data).unwrap();
        SegmentTableIterator {
            table: header.segment_table,
            cumulated: 0,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct Page<'data> {
    header: PageHeader<'data>,
    pub data: &'data [u8],
}

impl Page<'_> {
    fn parse(input: &[u8]) -> Result<'_, Page<'_>> {
        use OggError::*;
        let (data, header) = PageHeader::parse(input)?;
        if header.version != 0 {
            return Err(UnsupportedVersion(header.version));
        }
        let size: usize = header.segment_table.iter().map(|x| usize::from(*x)).sum();
        let (remaining, data) = take(size)(data)?;
        Ok((remaining, Page { header, data }))
    }

    fn last_packet_continues(&self) -> bool {
        *self.header.segment_table.last().unwrap() == 255
    }

    fn max_segment_size(&self, old_max: usize, accumulated: usize) -> (usize, usize) {
        let (max, last_max) = self.header.segment_table.iter().fold(
            (old_max, accumulated),
            |(all_max, mut current_max), current| {
                current_max += usize::from(*current);
                if *current < 255 {
                    (all_max.max(current_max), 0)
                } else {
                    (all_max, current_max)
                }
            },
        );
        if self.last_packet_continues() {
            (max.max(last_max), last_max)
        } else {
            (max.max(last_max), 0)
        }
    }

    /// Bitstream serial number for the page.
    pub fn bitstream_serial_number(&self) -> u32 {
        self.header.bitstream_serial_number
    }

    /// Page sequence number for the page.
    pub fn page_sequence_number(&self) -> u32 {
        self.header.page_sequence_number
    }

    /**
     * Parse pages from data until end of page at packet boundary.
     *
     * Useful for skipping comment headers. Returns the last page which is useful for validating
     * the stream.
     */
    pub(crate) fn skip(data: &[u8]) -> Result<'_, Page> {
        use OggError::*;
        let (mut remaining, mut page) = Self::parse(data)?;
        let mut page_sequence_number = page.page_sequence_number();
        let bitstream_serial_number = page.bitstream_serial_number();
        while page.last_packet_continues() {
            (remaining, page) = Self::parse(remaining)?;
            if page.page_sequence_number() != page_sequence_number + 1 {
                return Err(InvalidStream(ErrorValues::SequenceNumberMismatch(
                    page_sequence_number,
                    page.page_sequence_number(),
                )));
            }
            page_sequence_number = page.page_sequence_number();
            if page.bitstream_serial_number() != bitstream_serial_number {
                return Err(UnsupportedStream(
                    "bitstream serial number changed unexpectedly",
                ));
            }
        }
        Ok((remaining, page))
    }
}

/**
 * Iterator for ogg packets.
 *
 * Note that this does not implement [`Iterator`] trait because it is not possible to borrow from
 * iterator in [`Item`][`Iterator::Item`].
 */
#[derive(Debug, PartialEq)]
pub struct Packets<'data, const BUFFER_SIZE: usize> {
    data: &'data [u8],
    page: Page<'data>,
    segments: SegmentTableIterator<'data>,
    buffer: [u8; BUFFER_SIZE],
}

/// Ogg packet.
pub struct Packet<'buffer> {
    /// Data in ogg packet.
    pub data: &'buffer [u8],
}

impl<const BUFFER_SIZE: usize> Packets<'_, BUFFER_SIZE> {
    /// Parses input data for pages until a page that ends at packet boundary.
    pub(crate) fn parse(data: &[u8]) -> Result<'_, Packets<'_, BUFFER_SIZE>> {
        use OggError::*;
        let (mut remaining, mut page) = Page::parse(data)?;
        let (mut max_segment, mut acc) = page.max_segment_size(0, 0);
        let mut page_sequence_number = page.page_sequence_number();
        let bitstream_serial_number = page.bitstream_serial_number();
        while page.last_packet_continues() {
            (remaining, page) = Page::parse(remaining)?;
            (max_segment, acc) = page.max_segment_size(max_segment, acc);
            if page.page_sequence_number() != page_sequence_number + 1 {
                return Err(InvalidStream(ErrorValues::SequenceNumberMismatch(
                    page_sequence_number,
                    page.page_sequence_number(),
                )));
            }
            page_sequence_number = page.page_sequence_number();
            if page.bitstream_serial_number() != bitstream_serial_number {
                return Err(UnsupportedStream(
                    "bitstream serial number changed unexpectedly",
                ));
            }
        }
        if max_segment > BUFFER_SIZE {
            return Err(BufferTooSmallError(BUFFER_SIZE, max_segment));
        }
        let (next_data, page) = Page::parse(data)?;
        let (remaining, next_data) = take(next_data.len() - remaining.len())(next_data)?;
        Ok((
            remaining,
            Packets {
                data: next_data,
                page,
                segments: PageHeader::iter_segment_table(data),
                buffer: [0; BUFFER_SIZE],
            },
        ))
    }

    /// Returns page sequence number for the page being read.
    pub fn current_page_sequence_number(&self) -> u32 {
        self.page.page_sequence_number()
    }

    /// Returns page sequence number of the last page.
    pub fn last_page_sequence_number(&self) -> u32 {
        if self.data.is_empty() {
            self.current_page_sequence_number()
        } else {
            // These have been parsed already, we can expect them to succeed
            let (mut remaining, mut page) = Page::parse(self.data).unwrap();
            while page.last_packet_continues() {
                (remaining, page) = Page::parse(remaining).unwrap();
            }
            page.page_sequence_number()
        }
    }

    /// Returns bitstream serial number for the page being read.
    pub fn bitstream_serial_number(&self) -> u32 {
        self.page.bitstream_serial_number()
    }

    /// Returns whether the current page is the end of the stream.
    pub fn end_of_stream(&self) -> bool {
        self.page
            .header
            .header_type
            .contains(HeaderFlags::EndOfStream)
    }

    /// Iterates to the next packet and returns it, or [`None`] if the last packet has been read.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Packet<'_>> {
        let mut buf = 0;
        loop {
            if let Some(Segment {
                before,
                size,
                complete,
            }) = self.segments.next()
            {
                self.buffer[buf..buf + size]
                    .copy_from_slice(&self.page.data[before..before + size]);
                buf += size;
                if complete {
                    return Some(Packet {
                        data: &self.buffer[0..buf],
                    });
                }
            } else if self.page.last_packet_continues() {
                assert!(!self.data.is_empty());
                // These have been parsed already, we can expect them to succeed
                self.segments = PageHeader::iter_segment_table(self.data);
                (self.data, self.page) = Page::parse(self.data).unwrap();
                assert!(
                    (self.page.last_packet_continues() && !self.data.is_empty())
                        || (!self.page.last_packet_continues() && self.data.is_empty())
                );
            } else {
                assert!(self.data.is_empty());
                return None;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use core::error::Error;

    #[test]
    fn parse_empty_page() {
        let data = include_bytes!("test/empty.ogg");
        let (remaining, page) = Page::parse(data).unwrap();
        assert_eq!(remaining.len(), 0);
        assert_eq!(page.data.len(), 0);
        assert_eq!(page.header.version, 0);
        assert_eq!(page.header.header_type, HeaderFlags::BeginOfStream);
        assert_eq!(page.header._granule_position, 0);
        assert_eq!(page.header.bitstream_serial_number, 2132339074);
        assert_eq!(page.header.page_sequence_number, 0);
        assert_eq!(page.header.segment_table, &[0]);
    }

    #[test]
    fn parse_single_segment() {
        let data = include_bytes!("test/single.ogg");
        let (remaining, page) = Page::parse(data).unwrap();
        assert_eq!(remaining.len(), 0);
        assert_eq!(page.data.len(), 0x13);
        assert_eq!(page.header.version, 0);
        assert_eq!(page.header.header_type, HeaderFlags::BeginOfStream);
        assert_eq!(page.header._granule_position, 0);
        assert_eq!(page.header.bitstream_serial_number, 2132339074);
        assert_eq!(page.header.page_sequence_number, 0);
        assert_eq!(page.header.segment_table, &[0x13]);
        for (a, b) in (1u8..=0x19).zip(page.data) {
            assert_eq!(a, *b);
        }
    }

    #[test]
    fn parse_packet() -> core::result::Result<(), String> {
        let data = include_bytes!("test/split.ogg");
        let (remaining, mut packets) = Packets::<512>::parse(data).unwrap();
        assert_eq!(remaining.len(), 0);
        let packet = packets.next().unwrap();
        assert_eq!(packet.data.len(), 300);
        for (i, (a, b)) in (0u8..=99)
            .chain(0u8..=99)
            .chain(0u8..=99)
            .zip(packet.data)
            .enumerate()
        {
            if a != *b {
                return Err(format!("{a} != {b} at {i}"));
            }
        }
        for (i, (a, b)) in (0u8..=99)
            .chain(0u8..=99)
            .chain(0u8..=99)
            .chain(core::iter::repeat(0))
            .zip(packet.data.iter())
            .enumerate()
        {
            if a != *b {
                return Err(format!("{a} != {b} at {i}"));
            }
        }
        Ok(())
    }

    #[test]
    fn incomplete_page() {
        let data = include_bytes!("test/single.ogg");
        let result = Page::parse(&data[..40]);
        assert_eq!(result, Err(OggError::EndOfStreamError(None)));
    }

    #[test]
    fn incomplete_packet() {
        let data = include_bytes!("test/split.ogg");
        let result = Packets::<512>::parse(&data[..350]);
        assert_eq!(result, Err(OggError::EndOfStreamError(None)));
        assert!(result.unwrap_err().source().is_none());
        let result = Packets::<512>::parse(&data[..300]);
        assert_eq!(
            result,
            Err(OggError::EndOfStreamError(Some(1.try_into().unwrap())))
        );
        assert!(result.unwrap_err().source().is_none());
    }

    #[test]
    fn invalid_version() {
        let mut data = Vec::from(include_bytes!("test/empty.ogg"));
        data[4] = 1;
        let result = Page::parse(&data);
        assert_eq!(result, Err(OggError::UnsupportedVersion(1)));
        assert!(result.unwrap_err().source().is_none());
    }

    #[test]
    fn test_skip() {
        let data = include_bytes!("test/split.ogg");
        let (remaining, page) = Page::skip(data).unwrap();
        assert_eq!(remaining.len(), 0);
        assert_eq!(page.data.len(), 45);
        assert_eq!(page.header.version, 0);
        assert_eq!(page.header.header_type, HeaderFlags::Continuation);
        assert_eq!(page.header._granule_position, 0);
        assert_eq!(page.header.bitstream_serial_number, 2132339074);
        assert_eq!(page.header.page_sequence_number, 17);
        assert_eq!(page.header.segment_table, &[45]);
    }

    #[test]
    fn bad_sequence() {
        let mut data = Vec::from(include_bytes!("test/split.ogg"));
        data[0x12d] = 9;
        let result = Page::skip(&data);
        assert_eq!(
            result,
            Err(OggError::InvalidStream(
                ErrorValues::SequenceNumberMismatch(16, 9)
            ))
        );
        assert!(result.unwrap_err().source().is_none());
        let result = Packets::<512>::parse(&data);
        assert_eq!(
            result,
            Err(OggError::InvalidStream(
                ErrorValues::SequenceNumberMismatch(16, 9)
            ))
        );
        assert!(result.unwrap_err().source().is_none());
    }

    #[test]
    fn bitstream_changed() {
        let mut data = Vec::from(include_bytes!("test/split.ogg"));
        data[0x129] = 0x81;
        let result = Page::skip(&data);
        assert_eq!(
            result,
            Err(OggError::UnsupportedStream(
                "bitstream serial number changed unexpectedly"
            ))
        );
        assert!(result.unwrap_err().source().is_none());
        let result = Packets::<512>::parse(&data);
        assert_eq!(
            result,
            Err(OggError::UnsupportedStream(
                "bitstream serial number changed unexpectedly"
            ))
        );
        assert!(result.unwrap_err().source().is_none());
    }

    #[test]
    fn too_small_buffer() {
        let data = include_bytes!("test/split.ogg");
        let result = Packets::<64>::parse(data);
        assert_eq!(result, Err(OggError::BufferTooSmallError(64, 300)));
        assert!(result.unwrap_err().source().is_none());
    }
}
