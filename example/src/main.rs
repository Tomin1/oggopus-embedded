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

    let stream_content = std::fs::read(std::env::args().nth(1).ok_or("Argument missing")?)?;

    let stream: Bitstream = Bitstream::new(stream_content.as_slice());
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

    hwp.set_channels(channels.into())?;
    hwp.set_rate(header.sample_rate, ValueOr::Nearest)?;
    hwp.set_format(Format::s16())?;
    hwp.set_access(Access::RWInterleaved)?;
    pcm.hw_params(&hwp)?;
    let io = pcm.io_i16()?;

    let hwp = pcm.hw_params_current()?;
    let swp = pcm.sw_params_current()?;
    swp.set_start_threshold(hwp.get_buffer_size()?)?;
    pcm.sw_params(&swp)?;
    let sample_rate = hwp.get_rate()?;

    println!("Playing in {} at rate of {}", if channels == 1 { "mono" } else { "stereo" }, sample_rate);

    let mut decoder = Decoder::new(sample_rate.try_into()?, channels)?;
    let mut output = Vec::default();
    let mut total = 0;

    loop {
        let mut sum = 0;
        let (new_reader, mut packets) = reader
            .next_packets::<1024>()
            .inspect_err(|err| println!("Failed to read packets: {err:?}"))?;

        while let Some(packet) = packets.next() {
            output.resize(
                decoder.get_nb_samples(packet.data)? * usize::from(channels),
                0i16,
            );
            let samples = decoder.decode(packet.data, output.as_mut_slice())?;
            io.writei(&output[..samples])?;
            sum += samples;
        }
        println!("Decoded {sum} samples");
        total += sum;
        match new_reader {
            states::Either::Ended(next_reader) => {
                if let Some(next_reader) = next_reader.next_reader() {
                    let states::Either::Continued((next_reader, header)) =
                        next_reader.read_header().inspect_err(|err| {
                            println!("Failed to read header or comments packet: {err:?}")
                        })?
                    else {
                        return Err("New stream ended after header or comments packet".into());
                    };
                    println!("New stream started");
                    match header.channels {
                        ChannelMapping::Family0 { channels } => {
                            decoder = Decoder::new(sample_rate.try_into()?, channels)?;
                        }
                        _ => return Err("Unsupported channel mapping family".into()),
                    };
                    reader = next_reader;
                } else {
                    break;
                }
            }
            states::Either::Continued(new_reader) => {
                reader = new_reader;
            }
        }
    }

    println!("Total decoded samples: {total}");

    pcm.wait(None)?;
    pcm.drain()?;
    println!("Playback finished");

    Ok(())
}
