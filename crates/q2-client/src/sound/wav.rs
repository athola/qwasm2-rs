//! RIFF/WAVE loading (C `GetWavinfo`, client/sound/wave.c, and the sample
//! conversion of `SDL_Cache`, client/sound/sdl.c).
//!
//! The samples are decoded here rather than by the browser because Quake 2
//! keeps its loop point in a `cue ` chunk that browser decoders ignore.

use q2_common::error::{Q2Error, Q2Result};

/// What the wave header says (C `wavinfo_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavInfo {
    pub rate: i32,
    /// Bytes per sample.
    pub width: i32,
    pub channels: i32,
    /// Sample the sound loops back to, or -1 for a one-shot.
    pub loopstart: i32,
    pub samples: i32,
    /// Byte offset of the first sample in the file.
    pub dataofs: usize,
}

/// A decoded sound: mono samples in -1..1 at the file's rate.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    pub rate: u32,
    pub samples: Vec<f32>,
    /// Sample index the sound loops back to when it reaches its end.
    pub loop_start: Option<usize>,
}

impl Sound {
    /// Length in milliseconds.
    pub fn duration_ms(&self) -> f64 {
        self.samples.len() as f64 * 1000.0 / f64::from(self.rate)
    }
}

fn le_i16(wav: &[u8], at: usize) -> Option<i32> {
    let bytes = wav.get(at..at.checked_add(2)?)?;
    Some(i32::from(i16::from_le_bytes([bytes[0], bytes[1]])))
}

fn le_i32(wav: &[u8], at: usize) -> Option<i32> {
    let bytes = wav.get(at..at.checked_add(4)?)?;
    Some(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// C `FindNextChunk`: walk the chunks from `*last_chunk` to the first one
/// called `name`; returns its start and leaves `*last_chunk` after it.
/// Chunk bodies are padded to an even length.
fn find_next_chunk(
    wav: &[u8],
    end: usize,
    last_chunk: &mut usize,
    name: &[u8; 4],
) -> Option<usize> {
    loop {
        let chunk = *last_chunk;
        if chunk.checked_add(4)? >= end {
            return None;
        }
        let len = le_i32(wav, chunk + 4)?;
        if len < 0 {
            return None;
        }
        *last_chunk = chunk + 8 + ((len as usize + 1) & !1);
        if wav.get(chunk..chunk + 4) == Some(name) {
            return Some(chunk);
        }
    }
}

/// C `GetWavinfo`. Missing chunks and non-PCM data are errors here where
/// C prints and returns an empty header, which its caller then rejects.
pub fn wav_info(name: &str, wav: &[u8]) -> Q2Result<WavInfo> {
    let end = wav.len();
    let bad = |what: &str| Err(Q2Error::Drop(format!("{name}: {what}")));
    let mut last = 0;
    let riff = find_next_chunk(wav, end, &mut last, b"RIFF");
    let Some(riff) = riff.filter(|&r| wav.get(r + 8..r + 12) == Some(b"WAVE")) else {
        return bad("missing RIFF/WAVE chunks");
    };
    let iff_data = riff + 12;

    let mut last = iff_data;
    let Some(fmt) = find_next_chunk(wav, end, &mut last, b"fmt ") else {
        return bad("missing fmt chunk");
    };
    if le_i16(wav, fmt + 8) != Some(1) {
        return bad("Microsoft PCM format only");
    }
    let (Some(channels), Some(rate), Some(bits)) = (
        le_i16(wav, fmt + 10),
        le_i32(wav, fmt + 12),
        le_i16(wav, fmt + 22),
    ) else {
        return bad("short fmt chunk");
    };
    let width = bits / 8;

    let mut loopstart = -1;
    let mut samples = 0;
    let mut last = iff_data;
    if let Some(cue) = find_next_chunk(wav, end, &mut last, b"cue ") {
        loopstart = le_i32(wav, cue + 32).unwrap_or(0);
        // A LIST chunk right after may hold the loop length ("mark"); C's
        // own comment: not a proper parse, but it works with cooledit.
        if let Some(list) = find_next_chunk(wav, end, &mut last, b"LIST") {
            if list + 32 <= end && wav.get(list + 28..list + 32) == Some(b"mark") {
                samples = loopstart + le_i32(wav, list + 24).unwrap_or(0);
            }
        }
    }

    let mut last = iff_data;
    let Some(data) = find_next_chunk(wav, end, &mut last, b"data") else {
        return bad("missing data chunk");
    };
    if width <= 0 {
        return bad("bad sample width");
    }
    let data_samples = le_i32(wav, data + 4).unwrap_or(0) / width;
    if samples != 0 {
        if data_samples < samples {
            return bad("bad loop length");
        }
    } else {
        samples = data_samples;
    }
    Ok(WavInfo {
        rate,
        width,
        channels,
        loopstart,
        samples,
        dataofs: data + 8,
    })
}

/// Load a sound file (the checks of C `S_LoadSound` and `SDL_Cache`):
/// one or two channels, at least one sample. 8-bit samples are unsigned
/// and widened as C does (`(b - 128) << 8`); a stereo file keeps its
/// first channel, where C's mixer reads the interleaved data as mono.
pub fn decode(name: &str, wav: &[u8]) -> Q2Result<Sound> {
    let info = wav_info(name, wav)?;
    let bad = |what: &str| Err(Q2Error::Drop(format!("{name}: {what}")));
    if !(1..=2).contains(&info.channels) {
        return bad("invalid number of channels");
    }
    if info.samples <= 0 || info.rate <= 0 {
        return bad("zero length sound");
    }
    let width = info.width as usize;
    let step = width * info.channels as usize;
    let frames = info.samples as usize / info.channels as usize;
    let Some(bytes) = wav.get(info.dataofs..info.dataofs + frames * step) else {
        return bad("truncated data chunk");
    };
    let samples = bytes
        .chunks_exact(step)
        .map(|frame| {
            let sample = if width == 2 {
                i32::from(i16::from_le_bytes([frame[0], frame[1]]))
            } else {
                (i32::from(frame[0]) - 128) << 8
            };
            sample as f32 / 32768.0
        })
        .collect::<Vec<_>>();
    let loop_start = usize::try_from(info.loopstart)
        .ok()
        .filter(|&s| s < samples.len());
    Ok(Sound {
        rate: info.rate as u32,
        samples,
        loop_start,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A RIFF/WAVE file from chunks, each padded to an even length.
    pub(crate) fn riff(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut body = b"WAVE".to_vec();
        for (id, data) in chunks {
            body.extend_from_slice(*id);
            body.extend_from_slice(&(data.len() as i32).to_le_bytes());
            body.extend_from_slice(data);
            if data.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(body.len() as i32).to_le_bytes());
        file.extend(body);
        file
    }

    pub(crate) fn fmt(channels: i16, rate: i32, bits: i16) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(&1i16.to_le_bytes());
        f.extend_from_slice(&channels.to_le_bytes());
        f.extend_from_slice(&rate.to_le_bytes());
        let width = i32::from(bits / 8) * i32::from(channels);
        f.extend_from_slice(&(rate * width).to_le_bytes());
        f.extend_from_slice(&(width as i16).to_le_bytes());
        f.extend_from_slice(&bits.to_le_bytes());
        f
    }

    fn pcm16(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    /// A `cue ` chunk with one cue point at `start` (read at offset 32
    /// from the chunk start, as C does: its sample offset field).
    fn cue(start: i32) -> Vec<u8> {
        let mut c = vec![0u8; 28];
        c[0..4].copy_from_slice(&1i32.to_le_bytes());
        c[24..28].copy_from_slice(&start.to_le_bytes());
        c
    }

    /// A LIST/adtl chunk with an ltxt "mark" giving the loop length.
    fn mark_list(length: i32) -> Vec<u8> {
        let mut l = b"adtl".to_vec();
        l.extend_from_slice(b"ltxt");
        l.extend_from_slice(&20i32.to_le_bytes());
        l.extend_from_slice(&1i32.to_le_bytes()); // cue id
        l.extend_from_slice(&length.to_le_bytes()); // chunk + 24
        l.extend_from_slice(b"mark"); // chunk + 28
        l.extend_from_slice(&[0u8; 8]);
        l
    }

    #[test]
    fn sixteen_bit_mono_decodes_to_unit_range_samples() {
        let wav = riff(&[
            (b"fmt ", fmt(1, 22050, 16)),
            (b"data", pcm16(&[0, 16384, -32768])),
        ]);
        let sound = decode("a.wav", &wav).unwrap();
        assert_eq!(sound.rate, 22050);
        assert_eq!(sound.samples, vec![0.0, 0.5, -1.0]);
        assert_eq!(sound.loop_start, None);
    }

    #[test]
    fn eight_bit_samples_are_unsigned_and_widened_like_c() {
        let wav = riff(&[(b"fmt ", fmt(1, 11025, 8)), (b"data", vec![128, 255, 0])]);
        let sound = decode("a.wav", &wav).unwrap();
        assert_eq!(sound.rate, 11025);
        assert_eq!(sound.samples, vec![0.0, (127 << 8) as f32 / 32768.0, -1.0]);
    }

    #[test]
    fn odd_length_chunks_are_padded_before_the_next() {
        let wav = riff(&[
            (b"fmt ", fmt(1, 11025, 8)),
            (b"junk", vec![1, 2, 3]),
            (b"data", vec![128]),
        ]);
        assert_eq!(decode("a.wav", &wav).unwrap().samples, vec![0.0]);
    }

    #[test]
    fn cue_chunk_sets_the_loop_start() {
        let wav = riff(&[
            (b"fmt ", fmt(1, 11025, 16)),
            (b"cue ", cue(2)),
            (b"data", pcm16(&[1, 2, 3, 4])),
        ]);
        let info = wav_info("a.wav", &wav).unwrap();
        assert_eq!((info.loopstart, info.samples), (2, 4));
        assert_eq!(decode("a.wav", &wav).unwrap().loop_start, Some(2));
    }

    #[test]
    fn list_mark_after_cue_shortens_the_sound_to_the_loop_end() {
        let wav = riff(&[
            (b"fmt ", fmt(1, 11025, 16)),
            (b"cue ", cue(1)),
            (b"LIST", mark_list(2)),
            (b"data", pcm16(&[1, 2, 3, 4, 5])),
        ]);
        assert_eq!(wav_info("a.wav", &wav).unwrap().samples, 3);
        assert_eq!(decode("a.wav", &wav).unwrap().samples.len(), 3);
    }

    #[test]
    fn loop_longer_than_the_data_is_an_error() {
        // C: ERR_DROP "Sound %s has a bad loop length".
        let wav = riff(&[
            (b"fmt ", fmt(1, 11025, 16)),
            (b"cue ", cue(1)),
            (b"LIST", mark_list(9)),
            (b"data", pcm16(&[1, 2])),
        ]);
        assert!(wav_info("a.wav", &wav).is_err());
    }

    #[test]
    fn missing_riff_fmt_or_data_and_non_pcm_are_errors() {
        assert!(wav_info("a.wav", b"RIFX\x04\x00\x00\x00WAVE").is_err());
        assert!(wav_info("a.wav", &riff(&[(b"data", vec![0])])).is_err());
        assert!(wav_info("a.wav", &riff(&[(b"fmt ", fmt(1, 11025, 8))])).is_err());
        let mut adpcm = fmt(1, 11025, 8);
        adpcm[0] = 2;
        assert!(wav_info("a.wav", &riff(&[(b"fmt ", adpcm), (b"data", vec![0])])).is_err());
    }

    #[test]
    fn bad_channel_count_empty_and_truncated_data_are_errors() {
        let three = riff(&[(b"fmt ", fmt(3, 11025, 8)), (b"data", vec![0; 3])]);
        assert!(decode("a.wav", &three).is_err());
        let empty = riff(&[(b"fmt ", fmt(1, 11025, 8)), (b"data", vec![])]);
        assert!(decode("a.wav", &empty).is_err());
        let mut short = riff(&[(b"fmt ", fmt(1, 11025, 16)), (b"data", pcm16(&[1, 2]))]);
        short.truncate(short.len() - 1);
        assert!(decode("a.wav", &short).is_err());
    }

    #[test]
    fn stereo_keeps_the_first_channel() {
        let wav = riff(&[
            (b"fmt ", fmt(2, 11025, 16)),
            (b"data", pcm16(&[16384, -16384, 0, 1])),
        ]);
        assert_eq!(decode("a.wav", &wav).unwrap().samples, vec![0.5, 0.0]);
    }

    #[test]
    fn every_sound_in_the_demo_pak_decodes() {
        let pak =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gamedata/baseq2/pak0.pak");
        let Ok(pack) = q2_common::filesystem::Pack::load(&pak) else {
            return; // stock data not installed
        };
        let mut fs = q2_common::filesystem::FileSystem::new("baseq2");
        fs.add_pack(pack);
        let sounds: Vec<String> = fs
            .list_files("wav")
            .into_iter()
            .filter(|f| f.starts_with("sound/"))
            .collect();
        // 377 files under sound/, two of them not wave files.
        assert_eq!(sounds.len(), 375);
        let mut looping = 0;
        for name in &sounds {
            let sound = decode(name, &fs.load_file(name).unwrap())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(matches!(sound.rate, 11025 | 22050 | 44100), "{name}");
            looping += usize::from(sound.loop_start.is_some());
        }
        assert!(looping > 0, "ambient loops carry cue chunks");
    }
}
