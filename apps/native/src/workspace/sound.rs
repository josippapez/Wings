//! The desktop's notification sounds (`lib/sound.ts`): short tones made on the spot, so no files ship. The desktop
//! plays its notes through Web Audio; here the same notes and fades become a WAV in memory that AppKit's `NSSound`
//! plays.

use crate::saved::NotifySound;

/// A notification sound, saved by its lowercase label.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SoundId {
    #[default]
    Chime,
    Pop,
    Ping,
    Drop,
}

/// One tone: its pitch in Hz, when it starts and how long it lasts, in seconds.
struct Note {
    hz: f32,
    at: f32,
    len: f32,
}

impl SoundId {
    pub const ALL: [SoundId; 4] = [SoundId::Chime, SoundId::Pop, SoundId::Ping, SoundId::Drop];

    pub fn label(self) -> &'static str {
        match self {
            SoundId::Chime => "Chime",
            SoundId::Pop => "Pop",
            SoundId::Ping => "Ping",
            SoundId::Drop => "Drop",
        }
    }

    fn notes(self) -> &'static [Note] {
        match self {
            SoundId::Chime => &[
                Note {
                    hz: 880.0,
                    at: 0.0,
                    len: 0.25,
                },
                Note {
                    hz: 1318.5,
                    at: 0.09,
                    len: 0.35,
                },
            ],
            SoundId::Pop => &[Note {
                hz: 520.0,
                at: 0.0,
                len: 0.1,
            }],
            SoundId::Ping => &[Note {
                hz: 1568.0,
                at: 0.0,
                len: 0.5,
            }],
            SoundId::Drop => &[
                Note {
                    hz: 660.0,
                    at: 0.0,
                    len: 0.12,
                },
                Note {
                    hz: 440.0,
                    at: 0.1,
                    len: 0.22,
                },
            ],
        }
    }
}

/// The desktop's `notifySound` setting. `watching`: also for an agent in the pane you're looking at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundSettings {
    pub enabled: bool,
    pub sound: SoundId,
    pub watching: bool,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            sound: SoundId::Chime,
            watching: true,
        }
    }
}

impl SoundSettings {
    /// The saved setting, with anything missing or unknown replaced by the default, as the desktop's `cleanSound`.
    pub fn from_saved(saved: Option<&NotifySound>) -> Self {
        let default = Self::default();
        let Some(saved) = saved else { return default };
        Self {
            enabled: saved.enabled.unwrap_or(default.enabled),
            sound: SoundId::ALL
                .into_iter()
                .find(|id| Some(id.label().to_lowercase()) == saved.sound)
                .unwrap_or(default.sound),
            watching: saved.watching.unwrap_or(default.watching),
        }
    }

    pub fn to_saved(self) -> NotifySound {
        NotifySound {
            enabled: Some(self.enabled),
            sound: Some(self.sound.label().to_lowercase()),
            watching: Some(self.watching),
        }
    }
}

/// Notifications this close together share one sound, as the desktop's `BURST_MS`.
const BURST_MS: u64 = 1000;

/// Says whether a sound is due: the first one is, then none until a second has passed.
#[derive(Default)]
pub struct Burst {
    last: Option<u64>,
}

impl Burst {
    pub fn due(&mut self, now_ms: u64) -> bool {
        if self
            .last
            .is_some_and(|last| now_ms.saturating_sub(last) < BURST_MS)
        {
            return false;
        }
        self.last = Some(now_ms);
        true
    }
}

const RATE: u32 = 44_100;

/// Web Audio's gain for one note, `t` seconds after it starts: from 0.0001 up to 0.18 in 10 ms, then an exponential
/// fade back to 0.0001 at its end, as the desktop sets with `exponentialRampToValueAtTime`.
fn gain(t: f32, len: f32) -> f32 {
    const QUIET: f32 = 0.0001;
    const PEAK: f32 = 0.18;
    const ATTACK: f32 = 0.01;
    if t < ATTACK {
        QUIET * (PEAK / QUIET).powf(t / ATTACK)
    } else if t < len {
        PEAK * (QUIET / PEAK).powf((t - ATTACK) / (len - ATTACK))
    } else {
        QUIET
    }
}

/// The sound as a 16-bit mono WAV file. Each oscillator stops 20 ms after its fade, as in the desktop.
pub fn wav(id: SoundId) -> Vec<u8> {
    let notes = id.notes();
    let seconds = notes
        .iter()
        .map(|note| note.at + note.len + 0.02)
        .fold(0.0, f32::max);
    let frames = (seconds * RATE as f32).ceil() as u32;
    let mut out = Vec::with_capacity(44 + frames as usize * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + frames * 2).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(frames * 2).to_le_bytes());
    for frame in 0..frames {
        let time = frame as f32 / RATE as f32;
        let sample: f32 = notes
            .iter()
            .filter(|note| time >= note.at && time < note.at + note.len + 0.02)
            .map(|note| {
                let t = time - note.at;
                (std::f32::consts::TAU * note.hz * t).sin() * gain(t, note.len)
            })
            .sum();
        let sample = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[cfg(test)]
thread_local! {
    /// What tests asked to play; they never make a sound.
    pub static PLAYED: std::cell::RefCell<Vec<SoundId>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Plays one sound now, from the main thread.
pub fn play(id: SoundId) {
    #[cfg(test)]
    PLAYED.with(|played| played.borrow_mut().push(id));
    #[cfg(all(target_os = "macos", not(test)))]
    mac::play(id);
    #[cfg(not(any(target_os = "macos", test)))]
    let _ = id;
}

#[cfg(all(target_os = "macos", not(test)))]
mod mac {
    use std::{cell::RefCell, collections::HashMap};

    use objc2::{AnyThread, rc::Retained};
    use objc2_app_kit::NSSound;
    use objc2_foundation::NSData;

    use super::{SoundId, wav};

    thread_local! {
        /// Each sound is made once and kept: `play` returns at once and the sound must live until it has played.
        static SOUNDS: RefCell<HashMap<SoundId, Retained<NSSound>>> = RefCell::new(HashMap::new());
    }

    pub(super) fn play(id: SoundId) {
        SOUNDS.with(|sounds| {
            let mut sounds = sounds.borrow_mut();
            let sound = match sounds.get(&id) {
                Some(sound) => sound.clone(),
                None => {
                    let data = NSData::from_vec(wav(id));
                    let Some(sound) = NSSound::initWithData(NSSound::alloc(), &data) else {
                        eprintln!("[sound] AppKit couldn't read the {} sound", id.label());
                        return;
                    };
                    sounds.insert(id, sound.clone());
                    sound
                }
            };
            // A sound still playing would refuse to start again, as a quick second Preview would find.
            if sound.isPlaying() {
                sound.stop();
            }
            if !sound.play() {
                eprintln!("[sound] AppKit didn't play the {} sound", id.label());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(wav: &[u8]) -> Vec<i16> {
        let (pairs, _) = wav[44..].as_chunks::<2>();
        pairs.iter().map(|pair| i16::from_le_bytes(*pair)).collect()
    }

    #[test]
    fn each_sound_is_a_wav_as_long_as_its_notes_and_fades_out() {
        for id in SoundId::ALL {
            let wav = wav(id);
            assert_eq!(&wav[..4], b"RIFF");
            assert_eq!(&wav[8..16], b"WAVEfmt ");
            let data = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
            assert_eq!(data, wav.len() - 44);
            let samples = samples(&wav);
            let loudest = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
            // 0.18 of full scale, or near it where two notes overlap.
            assert!(
                loudest > 4_000 && loudest < 12_000,
                "{id:?} peaks at {loudest}"
            );
            let last = samples.last().unwrap().unsigned_abs();
            assert!(last < 10, "{id:?} ends at {last}");
        }
        // Chime: its second note ends at 0.09 + 0.35 s, plus 20 ms.
        assert_eq!(
            samples(&wav(SoundId::Chime)).len(),
            (0.46 * 44_100.0f32).ceil() as usize
        );
    }

    #[test]
    fn notifications_within_a_second_share_one_sound() {
        let mut burst = Burst::default();
        assert!(burst.due(10_000));
        assert!(!burst.due(10_400));
        assert!(!burst.due(10_999));
        assert!(burst.due(11_000));
    }

    #[test]
    fn a_saved_setting_with_missing_or_unknown_values_takes_the_defaults() {
        let clean = |json: &str| {
            let saved: NotifySound = serde_json::from_str(json).unwrap();
            SoundSettings::from_saved(Some(&saved))
        };
        assert_eq!(
            clean(r#"{"enabled":false}"#),
            SoundSettings {
                enabled: false,
                ..SoundSettings::default()
            }
        );
        let drop = clean(r#"{"sound":"drop","watching":false}"#);
        assert_eq!((drop.sound, drop.watching), (SoundId::Drop, false));
        assert_eq!(clean(r#"{"sound":"bell"}"#), SoundSettings::default());
        assert_eq!(SoundSettings::from_saved(Some(&drop.to_saved())), drop);
    }
}
