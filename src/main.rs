use core::ffi::{c_int, CStr};
use oggopus_embedded::{opus::ChannelMapping, states, Bitstream, BitstreamReader};
use opusic_sys::*;

fn get_opus_error_message(error: c_int) -> &'static str {
    let error = unsafe {
        let error = opus_strerror(error);
        CStr::from_ptr(error)
    };
    error.to_str().unwrap()
}

fn main() -> Result<(), Box<dyn core::error::Error + 'static>> {
    const STREAM: Bitstream = Bitstream::new(include_bytes!("test.ogg"));
    let reader = BitstreamReader::<'_, '_, states::Beginning>::new(&STREAM);
    let states::Either::Continued((mut reader, header)) = reader
        .read_header()
        .inspect_err(|err| println!("Failed to read header or comments packet: {err:?}"))?
    else {
        return Err("Stream ended after header or comments packet".into());
    };
    let channels = match header.channels {
        ChannelMapping::Family0 { channels } => channels.into(),
        _ => -1,
    };

    let mut error: c_int = 0;
    let decoder: *mut OpusDecoder = unsafe { opus_decoder_create(16_000, channels, &mut error) };

    loop {
        let mut sum = 0;
        let (new_reader, mut packets) = reader
            .next_packets::<1024>()
            .inspect_err(|err| println!("Failed to read packets: {err:?}"))?;

        while let Some(packet) = packets.next() {
            let mut output: [i16; 5760] = [0; 5760];

            let samples = unsafe {
                let len: i32 = packet.data.len().try_into().unwrap();
                let data = packet.data.as_ptr();
                let output = output.as_mut_slice().as_mut_ptr();
                opus_decode(decoder, data, len, output, 5760, 0)
            };
            if samples < 0 {
                println!("Error ({samples}): {}", get_opus_error_message(samples));
            } else {
                sum += samples;
            }
        }
        println!("Got {sum} samples");
        match new_reader {
            states::Either::Ended(_reader) => {
                break;
            }
            states::Either::Continued(new_reader) => {
                reader = new_reader;
            }
        }
    }

    unsafe { opus_decoder_destroy(decoder) };
    Ok(())
}
