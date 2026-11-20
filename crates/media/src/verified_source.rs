// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded source identity and decoding of the exact bytes that were verified.

use std::path::Path;

use sha2::{Digest, Sha256};
use suhina_model::InstrumentSourceIdentity;

use crate::{AudioData, MediaError};

pub const MAX_VERIFIED_SOURCE_BYTES: usize = 128 * 1024 * 1024;

pub fn source_identity(bytes: &[u8]) -> InstrumentSourceIdentity {
    InstrumentSourceIdentity { sha256: Sha256::digest(bytes).into(), bytes: bytes.len() as u64 }
}

/// Only plain files are read; the bound applies while reading, not only to metadata.
pub fn read_source(path: &Path) -> Result<Vec<u8>, MediaError> {
    let bytes = suhina_fs::read_plain(path, MAX_VERIFIED_SOURCE_BYTES)
        .map_err(|e| e.message(path, MAX_VERIFIED_SOURCE_BYTES))?;
    if bytes.is_empty() {
        return Err("an Instrument source cannot be empty".into());
    }
    Ok(bytes)
}

pub fn decode_source_bytes(
    path: &Path,
    bytes: Vec<u8>,
    identity: InstrumentSourceIdentity,
) -> Result<AudioData, MediaError> {
    if bytes.len() > MAX_VERIFIED_SOURCE_BYTES || source_identity(&bytes) != identity {
        return Err("Instrument source bytes do not match the saved length and SHA-256".into());
    }
    let stream = symphonia::core::io::MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
    let mut hint = crate::Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    crate::decode_stream(stream, &hint, None)
}

pub fn decode_verified_source(path: &Path, identity: InstrumentSourceIdentity) -> Result<AudioData, MediaError> {
    decode_source_bytes(path, read_source(path)?, identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_and_decoder_consume_the_same_owned_bytes() {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(
                &mut cursor,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 48000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for sample in [0i16, 16384, -16384, 0] {
                writer.write_sample(sample).unwrap();
            }
            writer.finalize().unwrap();
        }
        let bytes = cursor.into_inner();
        let identity = source_identity(&bytes);
        let path = Path::new("not-read-from-disk.wav");
        let audio = decode_source_bytes(path, bytes.clone(), identity).unwrap();
        assert_eq!(audio.channels, 1);
        assert_eq!(audio.sample_rate, 48000);
        assert_eq!(audio.samples, [0.0, 0.5, -0.5, 0.0]);
        let mut wrong = identity;
        wrong.sha256[0] ^= 1;
        assert!(decode_source_bytes(path, bytes.clone(), wrong).is_err());
        wrong = identity;
        wrong.bytes += 1;
        assert!(decode_source_bytes(path, bytes, wrong).is_err());
    }

    #[test]
    fn empty_directory_and_oversized_plain_sources_are_refused() {
        let folder = std::env::temp_dir().join(format!("suhina-verified-source-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        assert!(read_source(&folder).is_err());
        let path = folder.join("oversized.wav");
        let file = std::fs::File::create(&path).unwrap();
        assert!(read_source(&path).is_err());
        file.set_len(MAX_VERIFIED_SOURCE_BYTES as u64 + 1).unwrap();
        assert!(read_source(&path).is_err());
        drop(file);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(folder).unwrap();
    }
}
