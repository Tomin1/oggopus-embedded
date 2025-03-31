/*
 * Ogg parsing code.
 *
 * Copyright (c) 2025 Tomi Leppänen
 * SPDX-License-Identifier: BSD-3-Clause
 */

use bitflags::bitflags;
use core::num::NonZeroUsize;
use nom::{
    bytes::complete::{tag, take},
    error::ErrorKind,
    number, Parser,
};

bitflags! {
    #[derive(Debug, PartialEq)]
    pub struct HeaderFlags: u8 {
        const Continuation = 0b001;
        const BeginOfStream = 0b010;
        const EndOfStream = 0b100;
    }
}

#[derive(Debug)]
pub enum OggError {
    UnsupportedVersion(u8),
    ParsingError(ErrorKind),
    EndOfStreamError(Option<NonZeroUsize>),
    InvalidStream(&'static str),
    UnsupportedStream(&'static str),
    NotOggStream,
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
            InvalidStream(issue) => {
                f.write_fmt(format_args!("invalid Ogg stream: {}", issue))?
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
        match error {
            nom::Err::Failure((_, kind)) => ParsingError(kind),
            nom::Err::Error((_, kind)) => ParsingError(kind),
            nom::Err::Incomplete(nom::Needed::Size(size)) => EndOfStreamError(Some(size)),
            nom::Err::Incomplete(nom::Needed::Unknown) => EndOfStreamError(None),
        }
    }
}

pub type Result<'data, O> = core::result::Result<(&'data [u8], O), OggError>;

struct Segment {
    before: usize,
    size: usize,
    complete: bool,
}

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

#[derive(Debug)]
struct PageHeader<'data> {
    version: u8,
    header_type: HeaderFlags,
    _granule_position: u64,
    bitstream_serial_number: u32,
    page_sequence_number: u32,
    segment_table: &'data [u8],
}

impl PageHeader<'_> {
    pub fn parse(input: &[u8]) -> Result<PageHeader<'_>> {
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

pub struct Page<'data> {
    header: PageHeader<'data>,
    pub data: &'data [u8],
}

impl Page<'_> {
    pub fn parse(input: &[u8]) -> Result<'_, Page<'_>> {
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

    pub fn bitstream_serial_number(&self) -> u32 {
        self.header.bitstream_serial_number
    }

    pub fn page_sequence_number(&self) -> u32 {
        self.header.page_sequence_number
    }

    pub fn skip(data: &[u8]) -> Result<'_, Page> {
        use OggError::*;
        let (mut remaining, mut page) = Self::parse(data)?;
        let mut page_sequence_number = page.page_sequence_number();
        let bitstream_serial_number = page.bitstream_serial_number();
        while page.last_packet_continues() {
            (remaining, page) = Self::parse(remaining)?;
            if page.page_sequence_number() != page_sequence_number + 1 {
                return Err(InvalidStream("page sequence numbers are not sequential"));
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

pub struct Packets<'data, const BUFFER_SIZE: usize> {
    data: &'data [u8],
    page: Page<'data>,
    segments: SegmentTableIterator<'data>,
    buffer: [u8; BUFFER_SIZE],
}

pub struct Packet<'buffer> {
    pub data: &'buffer [u8],
}

impl<const BUFFER_SIZE: usize> Packets<'_, BUFFER_SIZE> {
    pub fn parse(data: &[u8]) -> Result<'_, Packets<'_, BUFFER_SIZE>> {
        use OggError::*;
        let (mut remaining, mut page) = Page::parse(data)?;
        let (mut max_segment, mut acc) = page.max_segment_size(0, 0);
        let mut page_sequence_number = page.page_sequence_number();
        let bitstream_serial_number = page.bitstream_serial_number();
        while page.last_packet_continues() {
            (remaining, page) = Page::parse(remaining)?;
            (max_segment, acc) = page.max_segment_size(max_segment, acc);
            if page.page_sequence_number() != page_sequence_number + 1 {
                return Err(InvalidStream("page sequence numbers are not sequential"));
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

    pub fn current_page_sequence_number(&self) -> u32 {
        self.page.page_sequence_number()
    }

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

    pub fn bitstream_serial_number(&self) -> u32 {
        self.page.bitstream_serial_number()
    }

    pub fn end_of_stream(&self) -> bool {
        self.page
            .header
            .header_type
            .contains(HeaderFlags::EndOfStream)
    }

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

    #[test]
    fn parse_empty_page() {
        let data = [
            79, 103, 103, 83, // "OggS"
            0,  // version, always 0
            2,  // begin of stream
            0, 0, 0, 0, 0, 0, 0, 0, // granule position
            1, 0, 0, 0, // bitstream serial number
            0, 0, 0, 0, // page sequence number
            0, 0, 0, 0, // CRC, not verified
            1, // number of page segments
            0, // segment table
        ];
        let result = Page::parse(&data);
        assert!(result.is_ok());
        let (remaining, page) = result.unwrap();
        assert_eq!(remaining.len(), 0);
        assert_eq!(page.data.len(), 0);
        assert_eq!(page.header.version, 0);
        assert_eq!(page.header.header_type, HeaderFlags::BeginOfStream);
        assert_eq!(page.header._granule_position, 0);
        assert_eq!(page.header.bitstream_serial_number, 1);
        assert_eq!(page.header.page_sequence_number, 0);
        assert_eq!(page.header.segment_table, &[0]);
    }

    #[test]
    fn parse_single_segment() {
        let data = [
            0x4F, 0x67, 0x67, 0x53, // "OggS"
            0x00, // version, always 0
            0x02, // begin of stream
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // granule position
            0x0E, 0x66, 0xD1, 0xFF, // bitstream serial number
            0x00, 0x00, 0x00, 0x00, // page sequence number
            0xDD, 0xD4, 0x9F, 0x50, // CRC
            0x01, 0x13, // number of segments and page segments table
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10, 0x11, 0x12, 0x13, // end of data
        ];
        let result = Page::parse(&data);
        assert!(result.is_ok());
        let (remaining, page) = result.unwrap();
        assert_eq!(remaining.len(), 0);
        assert_eq!(page.data.len(), 0x13);
        assert_eq!(page.header.version, 0);
        assert_eq!(page.header.header_type, HeaderFlags::BeginOfStream);
        assert_eq!(page.header._granule_position, 0);
        assert_eq!(page.header.bitstream_serial_number, 4291913230);
        assert_eq!(page.header.page_sequence_number, 0);
        assert_eq!(page.header.segment_table, &[0x13]);
        for (a, b) in (1u8..=0x19).zip(page.data) {
            assert_eq!(a, *b);
        }
    }

    #[test]
    fn parse_packet() -> core::result::Result<(), String> {
        let data = [
            79, 103, 103, 83, // "OggS"
            0,  // version, always 0
            0,  // header flags
            10, 0, 0, 0, 0, 0, 0, 0, // granule position
            1, 0, 0, 0, // bitstream serial number
            10, 0, 0, 0, // page sequence number
            0, 0, 0, 0,   // CRC
            1,   // number of page segments
            255, // segment table, followed by data
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
            23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43,
            44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
            65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85,
            86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 0, 1, 2, 3, 4, 5, 6, 7, 8,
            9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29,
            30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50,
            51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71,
            72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92,
            93, 94, 95, 96, 97, 98, 99, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
            16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36,
            37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53,
            54, // end of data
            79, 103, 103, 83, // "OggS"
            0,  // version, always 0
            1,  // header flags
            10, 0, 0, 0, 0, 0, 0, 0, // granule position
            1, 0, 0, 0, // bitstream serial number
            11, 0, 0, 0, // page sequence number
            0, 0, 0, 0,  // CRC
            1,  // number of page segments
            45, // segment table, followed by data
            55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75,
            76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96,
            97, 98, 99, // end of data
        ];
        let result = Packets::<512>::parse(&data);
        assert!(result.is_ok());
        let (remaining, mut packets) = result.unwrap();
        assert_eq!(remaining.len(), 0);
        let result = packets.next();
        assert!(result.is_some());
        let packet = result.unwrap();
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
}
