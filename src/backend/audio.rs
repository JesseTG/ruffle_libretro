use ruffle_core::backend::audio::{AudioBackend, AudioMixer, DecodeError, RegisterError, SoundHandle, SoundInstanceHandle, SoundTransform};
use ruffle_core::swf::{Sound, SoundFormat, SoundInfo, SoundStreamHead};
use ruffle_core::tag_utils::SwfSlice;

pub struct RetroAudioBackend {

    mixer: AudioMixer,
    playing: bool,
    output: [i16; 8192],
}

impl RetroAudioBackend {
    pub fn new(num_output_channels: u8, output_samplerate: u32) -> Self {
        let mut mixer = AudioMixer::new(num_output_channels, output_samplerate);

        Self {
            mixer,
            playing: false,
            output: [0; 8192],
        }
    }

    pub fn mix(&mut self) {
        self.mixer.mix(&mut self.output);
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
        self.mixer.register_sound(swf_sound)
    }

    fn register_mp3(&mut self, data: &[u8]) -> Result<SoundHandle, DecodeError> {
        self.mixer.register_mp3(data)
    }

    fn start_sound(&mut self, sound: SoundHandle, settings: &SoundInfo) -> Result<SoundInstanceHandle, DecodeError> {
        self.mixer.start_sound(sound, settings)
    }

    fn start_stream(&mut self, stream_handle: Option<SoundHandle>, clip_frame: u16, clip_data: SwfSlice, handle: &SoundStreamHead) -> Result<SoundInstanceHandle, DecodeError> {
        self.mixer.start_stream(stream_handle, clip_frame, clip_data, handle)
    }

    fn stop_sound(&mut self, sound: SoundInstanceHandle) {
        self.mixer.stop_sound(sound)
    }

    fn stop_all_sounds(&mut self) {
        self.mixer.stop_all_sounds()
    }

    fn get_sound_position(&self, instance: SoundInstanceHandle) -> Option<f64> {
        self.mixer.get_sound_position(instance)
    }

    fn get_sound_duration(&self, sound: SoundHandle) -> Option<f64> {
        self.mixer.get_sound_duration(sound)
    }

    fn get_sound_size(&self, sound: SoundHandle) -> Option<u32> {
        self.mixer.get_sound_size(sound)
    }

    fn get_sound_format(&self, sound: SoundHandle) -> Option<&SoundFormat> {
        self.mixer.get_sound_format(sound)
    }

    fn set_sound_transform(&mut self, instance: SoundInstanceHandle, transform: SoundTransform) {
        self.mixer.set_sound_transform(instance, transform)
    }

    fn get_sound_peak(&mut self, instance: SoundInstanceHandle) -> Option<[f32; 2]> {
        self.mixer.get_sound_peak(instance)
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