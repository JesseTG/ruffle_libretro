use log::debug;
use ruffle_core::backend::audio::{
    AudioBackend, AudioMixer, DecodeError, RegisterError, SoundHandle, SoundInstanceHandle, SoundTransform,
};
use ruffle_core::swf::{Sound, SoundFormat, SoundInfo, SoundStreamHead};
use ruffle_core::tag_utils::SwfSlice;
use std::time::Duration;

pub struct RetroAudioBackend {
    mixer: AudioMixer,
    playing: bool,
    output: [i16; Self::MAX_SAMPLES],
    output_samplerate: u32,
    fps: f64,
}

impl RetroAudioBackend {
    pub const MAX_SAMPLES: usize = 8192;

    pub fn new(num_output_channels: u8, output_samplerate: u32) -> Self {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::new");
        let mixer = AudioMixer::new(num_output_channels, output_samplerate);

        Self {
            mixer,
            playing: false,
            output: [0; Self::MAX_SAMPLES],
            output_samplerate,
            fps: 0.0,
        }
    }

    pub fn current_samples(&self) -> Option<&[i16]> {
        if !self.fps.is_finite() || self.fps < 1.0 {
            None
        } else {
            let num_samples = ((self.output_samplerate as usize) / (self.fps as usize)) * 2;
            debug!("output_samplerate={}, fps={}, num_samples={}", self.output_samplerate, self.fps, num_samples);

            Some(&self.output[..num_samples.min(Self::MAX_SAMPLES)])
        }

        // samples per frame = samples per second / frames per second
    }
}

impl AudioBackend for RetroAudioBackend {
    fn play(&mut self) {
        self.playing = true;
    }

    fn pause(&mut self) {
        self.playing = false;
    }

    fn register_sound(&mut self, swf_sound: &Sound) -> Result<SoundHandle, RegisterError> {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::register_sound");
        self.mixer.register_sound(swf_sound)
    }

    fn register_mp3(&mut self, data: &[u8]) -> Result<SoundHandle, DecodeError> {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::register_mp3");
        self.mixer.register_mp3(data)
    }

    fn start_sound(&mut self, sound: SoundHandle, settings: &SoundInfo) -> Result<SoundInstanceHandle, DecodeError> {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::start_sound");
        self.mixer.start_sound(sound, settings)
    }

    fn start_stream(
        &mut self,
        stream_handle: Option<SoundHandle>,
        clip_frame: u16,
        clip_data: SwfSlice,
        handle: &SoundStreamHead,
    ) -> Result<SoundInstanceHandle, DecodeError> {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::start_stream");
        self.mixer.start_stream(stream_handle, clip_frame, clip_data, handle)
    }

    fn stop_sound(&mut self, sound: SoundInstanceHandle) {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::stop_sound");
        self.mixer.stop_sound(sound)
    }

    fn stop_all_sounds(&mut self) {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::stop_all_sounds");
        self.mixer.stop_all_sounds()
    }

    fn get_sound_position(&self, instance: SoundInstanceHandle) -> Option<f64> {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::get_sound_position");
        self.mixer.get_sound_position(instance)
    }

    fn get_sound_duration(&self, sound: SoundHandle) -> Option<f64> {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::get_sound_duration");
        self.mixer.get_sound_duration(sound)
    }

    fn get_sound_size(&self, sound: SoundHandle) -> Option<u32> {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::get_sound_size");
        self.mixer.get_sound_size(sound)
    }

    fn get_sound_format(&self, sound: SoundHandle) -> Option<&SoundFormat> {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::get_sound_format");
        self.mixer.get_sound_format(sound)
    }

    fn set_sound_transform(&mut self, instance: SoundInstanceHandle, transform: SoundTransform) {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::set_sound_transform");
        self.mixer.set_sound_transform(instance, transform)
    }

    fn get_sound_peak(&mut self, instance: SoundInstanceHandle) -> Option<[f32; 2]> {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::get_sound_peak");
        self.mixer.get_sound_peak(instance)
    }

    fn is_loading_complete(&self) -> bool {
        true
    }

    fn tick(&mut self) {
        #[cfg(feature = "profiler")]
        profiling::scope!("RetroAudioBackend::tick");
        if self.fps.is_finite() && self.fps > 1.0 {
            let num_samples = ((self.output_samplerate as usize) / (self.fps as usize)) * 2;
            let interval = &mut self.output[..num_samples.min(Self::MAX_SAMPLES)];

            #[cfg(feature = "profiler")]
            profiling::scope!("AudioMixer::mix");
            self.mixer.mix(interval);
        }
    }

    fn set_frame_rate(&mut self, frame_rate: f64) {
        self.fps = frame_rate;
    }

    fn position_resolution(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(f64::from(Self::MAX_SAMPLES as u32) / f64::from(self.output_samplerate)))
    }

    fn volume(&self) -> f32 {
        self.mixer.volume()
    }

    fn set_volume(&mut self, volume: f32) {
        self.mixer.set_volume(volume)
    }

    fn get_sample_history(&self) -> [[f32; 2]; 1024] {
        self.mixer.get_sample_history()
    }
}
