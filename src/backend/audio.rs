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
    /// Mixes `sample_rate / fps` stereo sample frames on each `retro_run`.
    /// The rate must divide evenly, or the frontend would receive too few samples.
    pub fn new(sample_rate: u32, fps: u32) -> (Self, RetroAudioOutput) {
        assert_eq!(sample_rate % fps, 0, "sample rate {sample_rate} doesn't divide by {fps} fps");
        let samples_per_frame = sample_rate / fps;
        let mixer = AudioMixer::new(2, sample_rate);
        let playing = Arc::new(AtomicBool::new(true));
        let output = RetroAudioOutput {
            mixer: mixer.proxy(),
            playing: playing.clone(),
            buffer: vec![0; samples_per_frame as usize * 2],
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
    buffer: Vec<i16>,
}

impl RetroAudioOutput {
    /// Returns interleaved stereo samples covering one frame.
    pub fn mix_frame(&mut self) -> &[i16] {
        self.buffer.fill(0);
        if self.playing.load(Ordering::Relaxed) {
            self.mixer.mix(&mut self.buffer);
        }

        &self.buffer
    }
}
