/*
 * Copyright (c) 2025 Tomi Leppänen
 *
 * A simple example player that uses oggopus-embedded and opus-embedded.
 */

use alsa::{
    pcm::{Access, Format, HwParams},
    Direction, ValueOr, PCM,
};
use oggopus_embedded::{opus::ChannelMapping, states, Bitstream};
use opus_embedded::Decoder;

fn main() -> Result<(), Box<dyn core::error::Error>> {
    let pcm = PCM::new("default", Direction::Playback, false)?;
    let hwp = HwParams::any(&pcm)?;
    hwp.set_channels(1)?;
    hwp.set_rate(16_000, ValueOr::Nearest)?;
    hwp.set_format(Format::s16())?;
    hwp.set_access(Access::RWInterleaved)?;
    pcm.hw_params(&hwp)?;
    let io = pcm.io_i16()?;

    // NB: We cannot have non-static data with BitstreamReader
    let stream_content = std::fs::read(std::env::args().nth(1).ok_or("Argument missing")?)?.leak();

    let stream: Bitstream = Bitstream::new(stream_content);
    let reader = stream.reader();
    let states::Either::Continued((mut reader, header)) = reader
        .read_header()
        .inspect_err(|err| println!("Failed to read header or comments packet: {err:?}"))?
    else {
        return Err("Stream ended after header or comments packet".into());
    };
    let channels = match header.channels {
        ChannelMapping::Family0 { channels } => channels,
        _ => return Err("Unsupported channel mapping family".into()),
    };

    let mut decoder = Decoder::new(16_000, channels)?;

    loop {
        let mut sum = 0;
        let (new_reader, mut packets) = reader
            .next_packets::<1024>()
            .inspect_err(|err| println!("Failed to read packets: {err:?}"))?;

        while let Some(packet) = packets.next() {
            let mut output = Vec::default();
            output.resize(decoder.get_nb_samples(packet.data)?, 0i16);
            let samples = decoder.decode(packet.data, output.as_mut_slice())?;
            io.writei(&output[..samples])?;
            sum += samples;
        }
        println!("Decoded {sum} samples");
        match new_reader {
            states::Either::Ended(_reader) => {
                break;
            }
            states::Either::Continued(new_reader) => {
                reader = new_reader;
            }
        }
    }

    Ok(())
}
