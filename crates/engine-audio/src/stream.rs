//! Background, bounded audio decoding for long-form assets.

use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;

use engine_core::{EngineError, EngineResult};

/// One decoded streaming block.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioStreamBlock {
    /// Interleaved floating-point samples.
    pub samples: Arc<[f32]>,
    /// Channel count.
    pub channels: u16,
    /// Sample rate.
    pub sample_rate: u32,
}

enum StreamMessage {
    Block(AudioStreamBlock),
    End,
    Error(String),
}

/// Consumer for background-decoded audio blocks.
///
/// Decoding and container parsing occur on a worker thread. The bounded queue
/// applies backpressure so long assets do not decode entirely into memory.
pub struct AudioStreamReader {
    receiver: Receiver<StreamMessage>,
    ended: bool,
}

/// Result of a non-blocking stream poll.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioStreamPoll {
    /// A decoded block is ready.
    Block(AudioStreamBlock),
    /// No block is ready yet.
    Pending,
    /// The decoder reached the end of the stream.
    End,
}

impl std::fmt::Debug for AudioStreamReader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AudioStreamReader")
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

impl AudioStreamReader {
    /// Starts a background decoder with a bounded number of queued packets.
    pub fn spawn(
        name: impl Into<String>,
        bytes: Arc<[u8]>,
        queued_blocks: usize,
    ) -> EngineResult<Self> {
        if queued_blocks == 0 {
            return Err(EngineError::config(
                "audio stream queued_blocks must be greater than zero",
            ));
        }
        let name = name.into();
        let (sender, receiver) = sync_channel(queued_blocks);
        std::thread::Builder::new()
            .name("aster-audio-decode".to_string())
            .spawn(move || {
                let result = crate::decode::decode_packets(
                    &name,
                    &bytes,
                    |samples, channels, sample_rate| {
                        sender
                            .send(StreamMessage::Block(AudioStreamBlock {
                                samples: Arc::from(samples),
                                channels,
                                sample_rate,
                            }))
                            .map_err(|_| EngineError::other("audio stream consumer disconnected"))
                    },
                );
                match result {
                    Ok(()) => {
                        let _ = sender.send(StreamMessage::End);
                    }
                    Err(error) => {
                        let _ = sender.send(StreamMessage::Error(error.to_string()));
                    }
                }
            })
            .map_err(|error| {
                EngineError::other(format!("start audio decode worker failed: {error}"))
            })?;
        Ok(Self {
            receiver,
            ended: false,
        })
    }

    /// Blocks until the next decoded block is available.
    ///
    /// Returns `Ok(None)` after the stream ends.
    pub fn next_block(&mut self) -> EngineResult<Option<AudioStreamBlock>> {
        if self.ended {
            return Ok(None);
        }
        match self.receiver.recv() {
            Ok(StreamMessage::Block(block)) => Ok(Some(block)),
            Ok(StreamMessage::End) => {
                self.ended = true;
                Ok(None)
            }
            Ok(StreamMessage::Error(error)) => {
                self.ended = true;
                Err(EngineError::other(error))
            }
            Err(_) => {
                self.ended = true;
                Err(EngineError::other("audio decode worker disconnected"))
            }
        }
    }

    /// Polls the decoder without blocking the caller.
    pub fn try_next_block(&mut self) -> EngineResult<AudioStreamPoll> {
        if self.ended {
            return Ok(AudioStreamPoll::End);
        }
        match self.receiver.try_recv() {
            Ok(StreamMessage::Block(block)) => Ok(AudioStreamPoll::Block(block)),
            Ok(StreamMessage::End) => {
                self.ended = true;
                Ok(AudioStreamPoll::End)
            }
            Ok(StreamMessage::Error(error)) => {
                self.ended = true;
                Err(EngineError::other(error))
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(AudioStreamPoll::Pending),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.ended = true;
                Err(EngineError::other("audio decode worker disconnected"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_wav_bytes() -> Arc<[u8]> {
        let samples = [0_i16, i16::MAX, i16::MIN, 0_i16];
        let data_len = (samples.len() * 2) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&48_000_u32.to_le_bytes());
        bytes.extend_from_slice(&96_000_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        Arc::from(bytes)
    }

    #[test]
    fn zero_queue_capacity_is_rejected() {
        assert!(AudioStreamReader::spawn("empty.wav", Arc::from([]), 0).is_err());
    }

    #[test]
    fn stream_decoder_yields_bounded_pcm_blocks() {
        let mut stream = AudioStreamReader::spawn("test.wav", test_wav_bytes(), 1).unwrap();
        let block = stream.next_block().unwrap().unwrap();
        assert_eq!(block.channels, 1);
        assert_eq!(block.sample_rate, 48_000);
        assert_eq!(block.samples.len(), 4);
        assert!(stream.next_block().unwrap().is_none());
    }
}
