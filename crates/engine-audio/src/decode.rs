//! Encoded audio decoding through Symphonia.

use std::io::Cursor;

use engine_core::{EngineError, EngineResult};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Decodes supported encoded bytes to interleaved `f32` PCM.
pub fn decode(name: &str, bytes: &[u8]) -> EngineResult<(Vec<f32>, u16, u32)> {
    let mut samples = Vec::new();
    let mut format = None;
    decode_packets(name, bytes, |packet, channels, sample_rate| {
        format = Some((channels, sample_rate));
        samples.extend_from_slice(packet);
        Ok(())
    })?;
    let (channels, sample_rate) = format
        .ok_or_else(|| EngineError::other(format!("audio `{name}` decoded to no samples")))?;
    Ok((samples, channels, sample_rate))
}

pub(crate) fn decode_packets(
    name: &str,
    bytes: &[u8],
    mut on_packet: impl FnMut(&[f32], u16, u32) -> EngineResult<()>,
) -> EngineResult<()> {
    let mut hint = Hint::new();
    if let Some(extension) = name.rsplit('.').next().filter(|part| *part != name) {
        hint.with_extension(extension);
    }
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|error| EngineError::other(format!("decode `{name}` probe failed: {error}")))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| EngineError::other(format!("audio `{name}` has no default track")))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| EngineError::other(format!("decode `{name}` codec failed: {error}")))?;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| EngineError::other(format!("audio `{name}` has no sample rate")))?;
    let channels = track
        .codec_params
        .channels
        .map(|channels| channels.count() as u16)
        .ok_or_else(|| EngineError::other(format!("audio `{name}` has no channel layout")))?;
    let mut decoded_any = false;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                return Err(EngineError::other(format!(
                    "audio `{name}` changed format while decoding"
                )));
            }
            Err(error) => {
                return Err(EngineError::other(format!(
                    "decode `{name}` packet failed: {error}"
                )));
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buffer.copy_interleaved_ref(decoded);
                decoded_any = true;
                on_packet(buffer.samples(), channels, sample_rate)?;
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => {
                return Err(EngineError::other(format!(
                    "decode `{name}` samples failed: {error}"
                )));
            }
        }
    }

    if !decoded_any {
        return Err(EngineError::other(format!(
            "audio `{name}` decoded to no samples"
        )));
    }
    Ok(())
}
