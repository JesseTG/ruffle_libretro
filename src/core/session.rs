use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use futures::executor::LocalPool;
use log::info;
use ruffle_core::backend::navigator::SocketMode;
use ruffle_core::backend::storage::{MemoryStorageBackend, StorageBackend};
use ruffle_core::tag_utils::SwfMovie;
use ruffle_core::{Player, PlayerBuilder};
use ruffle_frontend_utils::backends::navigator::ExternalNavigatorBackend;
use ruffle_frontend_utils::backends::storage::DiskStorageBackend;
use ruffle_frontend_utils::content::{ContentDescriptor, PlayingContent};
use ruffle_video_software::backend::SoftwareVideoBackend;
use rust_libretro::anyhow::{self, anyhow, Context};
use rust_libretro::sys::retro_environment_t;
use url::Url;

use crate::backend::audio::{RetroAudioBackend, RetroAudioOutput};
use crate::backend::log::RetroLogBackend;
use crate::backend::navigator::{RetroFutureSpawner, RetroNavigatorInterface};
use crate::backend::render::create_renderer;
use crate::backend::ui::RetroUiBackend;
use crate::core::config::Config;
use crate::core::input::{gamepad_button_mapping, GamepadState};
use crate::util::mouse::MouseState;

/// The core runs at a fixed rate; the player advances by real Flash frames internally.
pub const FPS: u32 = 60;

/// Everything that exists while a movie is loaded.
///
/// Fields drop in declaration order: the player goes first, then the futures that
/// refer to it, and the async runtime last, since network futures run on it.
pub struct Session {
    pub player: Arc<Mutex<Player>>,
    executor: LocalPool,
    pub audio: RetroAudioOutput,
    pub mouse: MouseState,
    pub gamepad: GamepadState,
    /// The last frame read back from the GPU, as XRGB8888.
    pub frame: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub has_frame: bool,
    /// Must be entered while the player runs, since it can start network tasks.
    pub runtime: tokio::runtime::Runtime,
}

impl Session {
    pub fn new(
        path: &Path,
        config: &Config,
        environ_cb: retro_environment_t,
        save_directory: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let path = path
            .canonicalize()
            .with_context(|| format!("Couldn't find {}", path.display()))?;
        let data = std::fs::read(&path).with_context(|| format!("Couldn't read {}", path.display()))?;
        let url = Url::from_file_path(&path).map_err(|()| anyhow!("Invalid path {}", path.display()))?;

        let movie = SwfMovie::from_data(&data, url.to_string(), None, None).context("Not a valid SWF file")?;
        let width = (movie.width().to_pixels().round() as u32).max(1);
        let height = (movie.height().to_pixels().round() as u32).max(1);
        info!(
            "Loaded {} (SWF version {}, {width}x{height}, {} fps)",
            path.display(),
            movie.version(),
            movie.frame_rate().to_f64()
        );

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("ruffle-io")
            .enable_all()
            .build()
            .context("Couldn't start the async runtime")?;
        let _guard = runtime.enter();

        let executor = LocalPool::new();
        let content = PlayingContent::DirectFile(
            ContentDescriptor::new_local(&path, None).ok_or_else(|| anyhow!("Invalid path {}", path.display()))?,
        );
        let navigator = ExternalNavigatorBackend::new(
            url,
            None,
            None,
            RetroFutureSpawner(executor.spawner()),
            None,
            true, // upgrade_to_https
            HashSet::new(),
            SocketMode::Deny,
            Rc::new(content),
            RetroNavigatorInterface {
                environ_cb,
                web_browser_access: config.web_browser_access.clone(),
            },
        );

        let storage: Box<dyn StorageBackend> = match save_directory {
            Some(dir) => Box::new(DiskStorageBackend::new(dir.join("ruffle").join("SharedObjects"))),
            None => Box::new(MemoryStorageBackend::new()),
        };

        let renderer = create_renderer(width, height)?;
        // Every offered sample rate divides evenly by the frame rate.
        debug_assert_eq!(config.sample_rate % FPS, 0);
        let (audio_backend, audio) = RetroAudioBackend::new(config.sample_rate, config.sample_rate / FPS);

        let player = PlayerBuilder::new()
            .with_movie(movie)
            .with_renderer(renderer)
            .with_audio(audio_backend)
            .with_navigator(navigator)
            .with_storage(storage)
            .with_ui(RetroUiBackend::new(environ_cb))
            .with_log(RetroLogBackend)
            .with_video(SoftwareVideoBackend::new())
            .with_autoplay(true)
            .with_max_execution_duration(config.max_execution_duration)
            .with_quality(config.quality)
            .with_viewport_dimensions(width, height, 1.0)
            .with_spoofed_url(config.spoofed_url.clone())
            .with_gamepad_button_mapping(gamepad_button_mapping())
            .build();

        drop(_guard);

        Ok(Self {
            player,
            executor,
            audio,
            mouse: MouseState::default(),
            gamepad: GamepadState::default(),
            frame: vec![0; width as usize * height as usize * 4],
            width,
            height,
            has_frame: false,
            runtime,
        })
    }

    /// Polls loaders and other background work until none of it can make progress.
    /// Must not be called while the player is locked, since those futures lock it.
    pub fn run_futures(&mut self) {
        let _guard = self.runtime.enter();
        self.executor.run_until_stalled();
    }

    /// Writes SharedObjects (Flash saves) to disk, as Flash Player does on exit.
    pub fn flush(&self) {
        let _guard = self.runtime.enter();
        self.player.lock().expect("player lock poisoned").flush_shared_objects();
    }
}
