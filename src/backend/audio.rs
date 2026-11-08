use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ruffle_core::backend::audio::{
    swf, AudioBackend, AudioMixer, AudioMixerProxy, DecodeError, RegisterError, SoundHandle, SoundInstanceHandle,
    SoundStreamInfo, SoundTransform,
};
use ruffle_core::impl_audio_mixer_backend;

/// Audio backend handed to the player. It owns the mixer; the core pulls
/// mixed samples out through a [`RetroAudioOutput`] once per `retro_run`.
pub struct RetroAudioBackend {
    mixer: AudioMixer,
    playing: Arc<AtomicBool>,
}

impl RetroAudioBackend {
    pub fn new(sample_rate: u32) -> (Self, RetroAudioOutput) {
        let mixer = AudioMixer::new(2, sample_rate);
        let playing = Arc::new(AtomicBool::new(true));
        let output = RetroAudioOutput {
            mixer: mixer.proxy(),
            playing: playing.clone(),
            sample_rate,
            fps: 0.0,
            remainder: 0.0,
            buffer: Vec::new(),
        };

        (Self { mixer, playing }, output)
    }
}

impl AudioBackend for RetroAudioBackend {
    impl_audio_mixer_backend!(mixer);

    fn play(&mut self) {
        self.playing.store(true, Ordering::Relaxed);
    }

    fn pause(&mut self) {
        self.playing.store(false, Ordering::Relaxed);
    }
}

/// Produces exactly one video frame's worth of samples on each call to [`RetroAudioOutput::mix_frame`].
pub struct RetroAudioOutput {
    mixer: AudioMixerProxy,
    playing: Arc<AtomicBool>,
    sample_rate: u32,
    fps: f64,
    /// Fractional sample frames carried over between video frames, for rates
    /// that don't divide evenly by the frame rate.
    remainder: f64,
    buffer: Vec<i16>,
}

impl RetroAudioOutput {
    pub fn set_fps(&mut self, fps: f64) {
        self.fps = fps;
        self.remainder = 0.0;
    }

    /// Returns interleaved stereo samples covering one frame at the configured rate.
    pub fn mix_frame(&mut self) -> &[i16] {
        let exact = f64::from(self.sample_rate) / self.fps + self.remainder;
        let frames = exact.floor();
        self.remainder = exact - frames;

        self.buffer.clear();
        self.buffer.resize(frames as usize * 2, 0);

        if self.playing.load(Ordering::Relaxed) {
            self.mixer.mix(&mut self.buffer);
        }

        &self.buffer
    }
}
