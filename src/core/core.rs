use std::error::Error;
use std::ffi::CString;
use std::ops::DerefMut;
use std::panic;
use std::ptr;
use std::slice::from_raw_parts;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::executor::block_on;
use log::{debug, error, info, warn};

#[cfg(feature = "profiler")]
use profiling;

#[cfg(feature = "profiler")]
use profiling::tracy_client;
use ruffle_core::backend::navigator::NullNavigatorBackend;
use ruffle_core::backend::storage::MemoryStorageBackend;
use ruffle_core::config::Letterbox;
use ruffle_core::tag_utils::SwfMovie;
use ruffle_core::{LoadBehavior, Player, PlayerBuilder, PlayerEvent};
use ruffle_render::backend::ViewportDimensions;
use ruffle_video_software::backend::SoftwareVideoBackend;
use rust_libretro::contexts::*;
use rust_libretro::core::Core;
use rust_libretro::environment::get_save_directory;
use rust_libretro::sys::retro_hw_context_type::*;
use rust_libretro::sys::*;
use rust_libretro::types::{MessageProgress, PixelFormat, SystemInfo};
use rust_libretro::{anyhow, environment};
use thiserror::Error as ThisError;

use crate::backend::audio::RetroAudioBackend;
use crate::backend::log::RetroLogBackend;
use crate::backend::navigator::RetroNavigatorBackend;
use crate::backend::render::opengl::OpenGlWgpuRenderBackend;
use crate::backend::render::vulkan::VulkanWgpuRenderBackend;
use crate::backend::render::HardwareRenderError::UnsupportedHardwareContext;
use crate::backend::render::{enable_hw_render, enable_hw_render_negotiation_interface};
use crate::backend::storage::RetroVfsStorageBackend;
use crate::backend::ui::RetroUiBackend;
use crate::core::config::defaults;
use crate::core::state::PlayerState::*;
use crate::core::{input, Ruffle};
use crate::options::{FileAccessPolicy, WebBrowserAccess};
use crate::{built_info, util};

#[derive(ThisError, Debug)]
pub enum CoreError {
    #[error("No game was provided")]
    NoGameProvided,

    #[error("Failed to load SWF")]
    FailedToLoadSwf,
}

impl Core for Ruffle {
    fn get_info(&self) -> SystemInfo {
        SystemInfo {
            library_name: CString::new("Ruffle").unwrap(),
            library_version: CString::new(built_info::PKG_VERSION).unwrap(),
            valid_extensions: CString::new("swf").unwrap(),
            need_fullpath: false,
            block_extract: false,
        }
    }

    fn on_get_av_info(&mut self, _ctx: &mut GetAvInfoContext) -> retro_system_av_info {
        debug!("Ruffle::on_get_av_info");
        self.av_info.expect("Shouldn't be called until after on_load_game")
    }

    fn on_set_environment(&mut self, ctx: &mut SetEnvironmentContext) {
        #[cfg(feature = "profiler")]
        if self.tracy_client.is_none() {
            self.tracy_client = Some(tracy_client::Client::start());
            profiling::register_thread!("Main Thread");
        }

        #[cfg(feature = "profiler")]
        profiling::scope!("retro_set_environment");

        if let Err(e) = ctx.set_support_no_game(false) {
            warn!("RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME failed: {e}");
        }

        unsafe {
            if let Err(e) = ctx.enable_vfs_interface(3) {
                panic!("RETRO_ENVIRONMENT_GET_VFS_INTERFACE failed: {e}");
            }
        }

        let ctx = GenericContext::from(ctx);
        self.environ_cb.set({
            let environ_cb = unsafe { ctx.environment_callback() };
            if environ_cb.is_none() {
                panic!("Frontend passed an invalid environment callback");
            }
            *environ_cb
        });

        self.vfs.replace(unsafe {
            let vfs = environment::get_vfs_interface(
                self.environ_cb.get(),
                retro_vfs_interface_info {
                    required_interface_version: 3,
                    iface: ptr::null_mut(),
                },
            );

            match vfs {
                Ok(vfs) if vfs.iface.is_null() => None,
                Ok(vfs) => Some(*vfs.iface),
                _ => None,
            }
        });

        if let Err(e) = ctx.set_controller_info(input::CONTROLLER_INFO) {
            warn!("RETRO_ENVIRONMENT_SET_CONTROLLER_INFO failed: {e}");
        }
    }

    fn on_init(&mut self, ctx: &mut InitContext) {
        #[cfg(feature = "profiler")]
        info!("Profiling with tracy is enabled in this build");

        #[cfg(feature = "profiler")]
        profiling::scope!("retro_init");

        let existing_panic_handler = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            error!("{info}");
            existing_panic_handler(info);
        }));

        let ctx = GenericContext::from(ctx);
        self.frontend_preferred_hw_render = ctx.get_preferred_hw_render().unwrap();
    }

    fn on_deinit(&mut self, _ctx: &mut DeinitContext) {
        debug!("Core::on_deinit()");

        #[cfg(feature = "profiler")]
        {
            self.tracy_client = None;

            unsafe {
                tracy_client::sys::___tracy_shutdown_profiler();
            }
        }
    }

    fn on_set_controller_port_device(&mut self, _port: u32, _device: u32, _ctx: &mut GenericContext) {
    }

    fn on_reset(&mut self, _ctx: &mut ResetContext) {
        debug!("Core::on_reset()");
    }

    fn on_run(&mut self, ctx: &mut RunContext, delta_us: Option<i64>) {
        #[cfg(feature = "profiler")]
        let run_span = tracy_client::span!("retro_run", 0);
        if let Exiting = self.player {
            error!("Hardware context lost, core will now exit.");
            let ctx = GenericContext::from(ctx);
            self.notify_context_lost(&ctx);
            ctx.shutdown();
            return;
        }

        #[cfg(feature = "profiler")]
        if let Some(delta) = delta_us {
            run_span.emit_value(delta as u64);
        }

        if let (Active(player), Some(delta)) = (&mut self.player, delta_us) {
            let mut player = player.lock().expect("Cannot reenter");
            Self::handle_input(&mut player, &mut self.queued_events, ctx);

            {
                #[cfg(feature = "profiler")]
                profiling::scope!("Player::tick");

                player.tick((delta as f64) / 1000.0);
                // Ruffle wants milliseconds, we have microseconds.
            }

            let av_info = self.av_info.as_ref().expect("av_info should've been initialized");
            Self::render_graphics(&mut player, av_info, ctx);

            Self::send_audio(&mut player, ctx);

            // TODO: React to changed settings
        }

        #[cfg(feature = "profiler")]
        profiling::finish_frame!();
    }

    fn on_load_game(&mut self, game: Option<retro_game_info>, ctx: &mut LoadGameContext) -> anyhow::Result<()> {
        // TODO: Catch panics in here to we can exit gracefully

        #[cfg(feature = "profiler")]
        profiling::scope!("retro_load_game");
        ctx.set_pixel_format(PixelFormat::XRGB8888)?;
        ctx.enable_frame_time_callback((1000000.0f64 / 60.0).round() as retro_usec_t)?;

        enable_hw_render(ctx, self.frontend_preferred_hw_render)?;
        enable_hw_render_negotiation_interface(ctx, self.frontend_preferred_hw_render)?;
        let ctx = GenericContext::from(ctx);

        ctx.set_input_descriptors(input::INPUT_DESCRIPTORS)?;
        ctx.enable_keyboard_callback()?;

        let game = game.ok_or(CoreError::NoGameProvided)?;

        // TODO: log the game's name to the profiler with Span.emit_value

        let buffer = unsafe { from_raw_parts(game.data as *const u8, game.size as usize) };
        let movie = SwfMovie::from_data(buffer, "http://localhost".to_string(), None)
            .ok()
            .ok_or(CoreError::FailedToLoadSwf)?;

        let dimensions = ViewportDimensions {
            width: movie.width().to_pixels().round() as u32,
            height: movie.height().to_pixels().round() as u32,
            scale_factor: 1.0f64, // TODO: figure this out
        };

        let environ_cb = self.environ_cb.get();

        self.av_info = Some(retro_system_av_info {
            geometry: retro_game_geometry {
                base_width: dimensions.width,
                base_height: dimensions.height,
                max_width: dimensions.width,
                max_height: dimensions.height,
                aspect_ratio: (dimensions.width as f32) / (dimensions.height as f32),
            },
            timing: retro_system_timing {
                fps: f64::from(movie.frame_rate()),
                sample_rate: self.config.sample_rate as f64,
            },
        });

        let builder = PlayerBuilder::new()
            .with_movie(movie)
            .with_ui(RetroUiBackend::new(self.environ_cb.clone()))
            .with_log(RetroLogBackend::new())
            .with_audio(RetroAudioBackend::new(2, self.config.sample_rate))
            //.with_navigator(RetroNavigatorBackend::new())
            .with_video(SoftwareVideoBackend::new())
            .with_autoplay(self.config.autoplay)
            .with_letterbox(self.config.letterbox)
            .with_max_execution_duration(self.config.max_execution_duration)
            .with_warn_on_unsupported_content(self.config.warn_on_unsupported_content)
            .with_viewport_dimensions(dimensions.width, dimensions.height, dimensions.scale_factor)
            .with_fullscreen(true)
            .with_load_behavior(self.config.load_behavior)
            .with_spoofed_url(self.config.spoofed_url.clone());

        let save_directory = unsafe { get_save_directory(environ_cb) };
        let builder = match save_directory {
            Ok(Some(base_path)) => builder.with_storage(RetroVfsStorageBackend::new(base_path, self.vfs.clone())?),
            _ => builder.with_storage(MemoryStorageBackend::new()),
        };

        // Renderer not initialized here, because we can't do so
        // until the frontend calls on_hw_context_reset

        self.player = Pending(builder.into());

        Ok(())
    }

    fn on_unload_game(&mut self, _ctx: &mut UnloadGameContext) {
        // TODO: Call vfs_flush
        #[cfg(feature = "profiler")]
        profiling::scope!("retro_unload_game");
        debug!("Ruffle::on_unload_game()");
        self.player = Uninitialized;
    }

    fn on_options_changed(&mut self, ctx: &mut OptionsChangedContext) {
        #[cfg(feature = "profiler")]
        profiling::scope!("Ruffle::on_options_changed");
        self.config.autoplay = match ctx.get_variable("ruffle_autoplay") {
            Ok(Some("true")) => true,
            Ok(Some("false")) => false,
            _ => defaults::AUTOPLAY,
        };

        self.config.letterbox = match ctx.get_variable("ruffle_letterbox") {
            Ok(Some("off")) => Letterbox::Off,
            Ok(Some("fullscreen")) => Letterbox::Fullscreen,
            Ok(Some("on")) => Letterbox::On,
            _ => defaults::LETTERBOX,
        }; // TODO: Should I reset the driver if this changed?

        self.config.max_execution_duration = ctx
            .get_variable("ruffle_max_execution_duration")
            .unwrap_or(None)
            .and_then(|s: &str| s.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(defaults::MAX_EXECUTION_DURATION);

        self.config.msaa = ctx
            .get_variable("ruffle_msaa")
            .unwrap_or(None)
            .and_then(|s: &str| s.parse::<u8>().ok())
            .unwrap_or(defaults::MSAA);

        self.config.warn_on_unsupported_content = match ctx.get_variable("ruffle_warn_on_unsupported_content") {
            Ok(Some("true")) => true,
            Ok(Some("false")) => false,
            _ => defaults::WARN_ON_UNSUPPORTED_CONTENT,
        };

        self.config.file_access_policy = match ctx.get_variable("ruffle_file_access_policy") {
            Ok(Some("never")) => FileAccessPolicy::Never,
            Ok(Some("notify")) => FileAccessPolicy::Notify,
            Ok(Some("always")) => FileAccessPolicy::Always,
            _ => defaults::FILE_ACCESS_POLICY,
        };

        self.config.web_browser_access = match ctx.get_variable("ruffle_web_browser_access") {
            Ok(Some("off")) => WebBrowserAccess::Ignore,
            Ok(Some("off-notify")) => WebBrowserAccess::Notify,
            Ok(Some("external")) => WebBrowserAccess::OpenInBrowser,
            _ => defaults::WEB_BROWSER_ACCESS,
        };

        self.config.sample_rate = ctx
            .get_variable("ruffle_audio_sample_rate")
            .unwrap_or(None)
            .and_then(|s: &str| s.parse::<u32>().ok())
            .unwrap_or(defaults::SAMPLE_RATE);

        self.config.load_behavior = match ctx.get_variable("ruffle_load_behavior") {
            Ok(Some("streaming")) => LoadBehavior::Streaming,
            Ok(Some("blocking")) => LoadBehavior::Blocking,
            Ok(Some("delayed")) => LoadBehavior::Delayed,
            _ => defaults::LOAD_BEHAVIOR,
        };

        if let Active(player) = &self.player {
            let mut player = player.lock().unwrap();

            player.set_letterbox(self.config.letterbox); // TODO: What if old letterbox == new letterbox?
            player.set_max_execution_duration(self.config.max_execution_duration);
        }
    }

    fn on_keyboard_event(&mut self, down: bool, keycode: retro_key, _character: u32, _key_modifiers: retro_mod) {
        #[cfg(feature = "profiler")]
        profiling::scope!("retro_keyboard_callback::callback");
        let event = match (down, keycode) {
            (true, keycode) => PlayerEvent::KeyDown {
                key_code: util::keyboard::to_key_code(keycode),
                key_char: None,
            },
            (false, keycode) => PlayerEvent::KeyUp {
                key_code: util::keyboard::to_key_code(keycode),
                key_char: None,
            },
        };

        self.queued_events.push_back(event);
    }

    fn on_write_audio(&mut self, ctx: &mut AudioContext) {
        #[cfg(feature = "profiler")]
        profiling::scope!("retro_audio_callback::callback");
        if let Active(player) = &self.player {
            let mut player = player.lock().unwrap();
            let player = player.deref_mut();

            let audio = player.audio_mut();

            let resampled_samples: [i16; 2048] = [0; 2048];
            ctx.batch_audio_samples(&resampled_samples);
        }
    }

    fn on_audio_set_state(&mut self, enabled: bool) {
        #[cfg(feature = "profiler")]
        profiling::scope!("retro_audio_callback::set_state");
        if let Active(player) = &self.player {
            let mut player = player.lock().unwrap();
            let player = player.deref_mut();

            if enabled {
                player.audio_mut().play();
            } else {
                player.audio_mut().pause();
            }
        } else {
            warn!("on_audio_set_state({enabled}) called before player was ready");
        }
    }

    fn on_hw_context_reset(&mut self, context: &mut GenericContext) {
        debug!("Core::on_hw_context_reset()");
        #[cfg(feature = "profiler")]
        profiling::scope!("retro_hw_render_callback::context_reset");
        match &self.player {
            Active(_) => {
                // Game is already running
                self.player = Exiting;
                context.shutdown();
            }
            Pending(builder) => {
                // Game is waiting for hardware context to be ready
                self.player = match self.finalize_player(builder.take(), context) {
                    // We take ownership of the builder, then throw it out after the player is built
                    Ok(player) => {
                        info!("Initialized render backend and finalized player");
                        Active(player)
                    }
                    Err(error) => {
                        error!("Failed to initialize render backend: {error}");
                        Uninitialized
                    }
                };
            }
            Uninitialized => {
                warn!("Resetting hardware context before core is ready");
            }
            Exiting => {
                warn!("Resetting hardware context after a fatal error");
            }
        };
    }

    fn on_hw_context_destroyed(&mut self, ctx: &mut GenericContext) {
        debug!("Core::on_hw_context_destroyed()");
        #[cfg(feature = "profiler")]
        profiling::scope!("retro_hw_render_callback::context_destroy");
        match &self.player {
            Active(_) => {
                self.player = Exiting;
                ctx.shutdown();
            }
            _ => {
                warn!("Destroying hardware context without an active player");
            }
        }
    }

    fn on_core_options_update_display(&mut self) -> bool {
        todo!()
    }
}

const CONTEXT_LOST_MESSAGE: &'static str =
    "Hardware context lost, and Ruffle can't reinitialize it.\nPlease reload the game.";
const CONTEXT_LOST_PRIORITY: u32 = 32;
const CONTEXT_LOST_DURATION: u32 = 3000;

impl Ruffle {
    fn finalize_player(
        &self,
        mut builder: PlayerBuilder,
        ctx: &mut GenericContext,
    ) -> Result<Arc<Mutex<Player>>, Box<dyn Error>> {
        #[cfg(feature = "profiler")]
        profiling::scope!("Ruffle::finalize_player");
        let av_info = &self
            .av_info
            .expect("av_info should've been initialized in on_load_game");

        let hw_render_callback = unsafe {
            ctx.interfaces()
                .read()
                .expect("Only one thread should access this")
                .hw_render_callback
                .unwrap()
        };

        builder = match hw_render_callback.context_type {
            RETRO_HW_CONTEXT_OPENGL
            | RETRO_HW_CONTEXT_OPENGLES2
            | RETRO_HW_CONTEXT_OPENGLES3
            | RETRO_HW_CONTEXT_OPENGL_CORE
            | RETRO_HW_CONTEXT_OPENGLES_VERSION => {
                builder.with_renderer(block_on(OpenGlWgpuRenderBackend::new(&hw_render_callback, &av_info.geometry))?)
            }
            RETRO_HW_CONTEXT_VULKAN => {
                let render_interface = unsafe { ctx.get_hw_render_interface_vulkan()? };
                builder.with_renderer(VulkanWgpuRenderBackend::new(&av_info.geometry, &render_interface)?)
            }
            other => Err(UnsupportedHardwareContext(other))?,
        };

        Ok(builder.build())
    }

    fn handle_input(player: &mut Player, queued_events: &mut VecDeque<PlayerEvent>, ctx: &mut RunContext) {
        #[cfg(feature = "profiler")]
        profiling::scope!("Ruffle::handle_input");
        {
            #[cfg(feature = "profiler")]
            profiling::scope!("retro_input_poll_t");
            ctx.poll_input();
        }

        let mouse_left_button = ctx.get_input_state(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_LEFT) != 0;
        let mouse_right_button = ctx.get_input_state(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_RIGHT) != 0;
        let mouse_middle_button = ctx.get_input_state(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_MIDDLE) != 0;
        let mouse_wheel_down = ctx.get_input_state(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_WHEELDOWN) != 0;
        let mouse_wheel_up = ctx.get_input_state(0, RETRO_DEVICE_MOUSE, 0, RETRO_DEVICE_ID_MOUSE_WHEELUP) != 0;

        let mouse_state = MouseState::from_context(ctx);

        for e in &mut *queued_events {
            player.handle_event(*e);
        }

        queued_events.clear();
    }

    fn render_graphics(player: &mut Player, av_info: &retro_system_av_info, ctx: &mut RunContext) {
        if player.needs_render() {
            #[cfg(feature = "profiler")]
            profiling::scope!("Player::render");
            player.render();
        }

        {
            #[cfg(feature = "profiler")]
            profiling::scope!("retro_video_refresh_t");

            ctx.draw_hardware_frame(av_info.geometry.max_width, av_info.geometry.max_height, 0);
        }
    }

    fn send_audio(player: &mut Player, ctx: &mut RunContext) {
        #[cfg(feature = "profiler")]
        profiling::scope!("retro_run::handle_audio");
        let audio = player
            .audio()
            .downcast_ref::<RetroAudioBackend>()
            .expect("Unexpected AudioBackend implementation");

        if let Some(samples) = audio.current_samples() {
            let ctx = AudioContext::from(ctx);

            #[cfg(feature = "profiler")]
            profiling::scope!("retro_audio_sample_batch_t");

            ctx.batch_audio_samples(&samples[..]);
        }
    }

    fn notify_context_lost(&self, ctx: &GenericContext) {
        let message_sent = ctx.set_message_ext(
            CONTEXT_LOST_MESSAGE,
            CONTEXT_LOST_DURATION,
            CONTEXT_LOST_PRIORITY,
            retro_log_level::RETRO_LOG_ERROR,
            retro_message_target::RETRO_MESSAGE_TARGET_ALL,
            retro_message_type::RETRO_MESSAGE_TYPE_NOTIFICATION,
            MessageProgress::Indeterminate,
        );

        if let Err(e) = message_sent {
            error!("{CONTEXT_LOST_MESSAGE}");
            error!("Additionally, RETRO_ENVIRONMENT_SET_MESSAGE_EXT failed: {e}");
        }
    }
}
