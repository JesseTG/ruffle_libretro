use std::error::Error;
use std::ffi::CString;
use std::ops::DerefMut;
use std::ptr;
use std::slice::from_raw_parts;
use std::time::Duration;

use ash::vk;
use log::{debug, error, info, warn};
use ruffle_core::{LoadBehavior, PlayerBuilder, PlayerEvent};
use ruffle_core::backend::navigator::NullNavigatorBackend;
use ruffle_core::backend::storage::MemoryStorageBackend;
use ruffle_core::config::Letterbox;
use ruffle_core::tag_utils::SwfMovie;
use ruffle_render::backend::ViewportDimensions;
use ruffle_video_software::backend::SoftwareVideoBackend;
use rust_libretro::{environment, retro_hw_context_destroyed_callback, retro_hw_context_reset_callback};
use rust_libretro::contexts::*;
use rust_libretro::core::Core;
use rust_libretro::environment::get_save_directory;
use rust_libretro::sys::*;
use rust_libretro::sys::retro_hw_context_type::*;
use rust_libretro::types::{PixelFormat, SystemInfo};

use crate::{built_info, util};
use crate::backend::audio::RetroAudioBackend;
use crate::backend::log::RetroLogBackend;
use crate::backend::storage::RetroVfsStorageBackend;
use crate::backend::ui::RetroUiBackend;
use crate::core::config::defaults;
use crate::core::render::RenderInterfaceError::NoRenderState;
use crate::core::Ruffle;
use crate::core::state::PlayerState;
use crate::core::state::PlayerState::{Active, Pending};
use crate::options::{FileAccessPolicy, WebBrowserAccess};

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
        self.av_info.expect("Shouldn't be called until after on_load_game")
    }

    fn on_set_environment(&mut self, initial: bool, ctx: &mut SetEnvironmentContext) {
        if initial {
            ctx.set_support_no_game(false);
        }

        self.environ_cb.set({
            let ctx = GenericContext::from(ctx);
            let environ_cb = unsafe { ctx.environment_callback() };
            if environ_cb.is_none() {
                panic!("Frontend passed an invalid environment callback");
            }
            *environ_cb
        });
        /*
        if initial {
            let exclusions = vec![String::from("naga::front")];
            let logger = match { unsafe { environment::get_log_callback(self.environ_cb.get()) } } {
                Ok(log_cb) => logger::RetroLogger::new(log_cb.unwrap(), exclusions),
                Err(_) => logger::RetroLogger::new(retro_log_callback { log: None }, exclusions),
            };

            log::set_max_level(LevelFilter::Trace);
            log::set_boxed_logger(Box::new(logger)).expect("could not set logger");
        }*/

        self.vfs.replace(unsafe {
            let vfs = environment::get_vfs_interface(
                self.environ_cb.get(),
                retro_vfs_interface_info {
                    required_interface_version: 3,
                    iface: ptr::null_mut(),
                },
            );

            match vfs {
                Some(vfs) if vfs.iface.is_null() => None,
                Some(vfs) => Some(*vfs.iface),
                None => None,
            }
        });
    }

    fn on_init(&mut self, _ctx: &mut InitContext) {}

    fn on_deinit(&mut self, _ctx: &mut DeinitContext) {}

    fn on_set_controller_port_device(&mut self, _port: u32, _device: u32, _ctx: &mut GenericContext) {
        todo!()
    }

    fn on_reset(&mut self, _ctx: &mut ResetContext) {
        todo!()
    }

    fn on_run(&mut self, ctx: &mut RunContext, _delta_us: Option<i64>) {
        if let Active(player) = &self.player {
            ctx.poll_input();
            // TODO: Handle input
            ctx.get_joypad_state(0, 0);
            let mut player = player.lock().unwrap();

            player.run_frame();

            if let Err(error) = self
                .render_state
                .as_ref()
                .ok_or(NoRenderState)
                .unwrap()
                .render(player.deref_mut())
            {
                error!("Error when rendering: {error}");
            }

            // TODO: Write out audio
            // TODO: React to changed settings
        }

        let av_info = self.av_info.expect("av_info should've been initialized by now");
        ctx.draw_hardware_frame(av_info.geometry.max_width, av_info.geometry.max_height, 0);
    }

    fn on_load_game(&mut self, game: Option<retro_game_info>, ctx: &mut LoadGameContext) -> Result<(), Box<dyn Error>> {
        let game = game.ok_or("No game was provided")?;
        let buffer = unsafe { from_raw_parts(game.data as *const u8, game.size as usize) };
        let movie = SwfMovie::from_data(buffer, None, None)?;
        let dimensions = ViewportDimensions {
            width: movie.width().to_pixels().round() as u32,
            height: movie.height().to_pixels().round() as u32,
            scale_factor: 1.0f64, // TODO: figure this out
        };

        if !ctx.set_pixel_format(PixelFormat::XRGB8888) {
            return Err("RETRO_PIXEL_FORMAT_XRGB8888 not supported by this frontend".into());
        }

        let preferred_renderer = match { unsafe { environment::get_preferred_hw_render(self.environ_cb.get()) } } {
            1 => RETRO_HW_CONTEXT_OPENGL,
            2 => RETRO_HW_CONTEXT_OPENGLES2,
            3 => RETRO_HW_CONTEXT_OPENGL_CORE,
            4 => RETRO_HW_CONTEXT_OPENGLES3,
            5 => RETRO_HW_CONTEXT_OPENGLES_VERSION,
            6 => RETRO_HW_CONTEXT_VULKAN,
            7 => RETRO_HW_CONTEXT_DIRECT3D,
            unknown => Err(format!("Frontend prefers unrecognized hardware context type {unknown}"))?,
        };

        self.hw_render_callback = Some(retro_hw_render_callback {
            context_type: match preferred_renderer {
                RETRO_HW_CONTEXT_OPENGL => RETRO_HW_CONTEXT_OPENGLES2,
                RETRO_HW_CONTEXT_OPENGLES_VERSION | RETRO_HW_CONTEXT_OPENGL_CORE => RETRO_HW_CONTEXT_OPENGLES3,
                // wgpu supports OpenGL ES, but *not* plain OpenGL.
                _ => preferred_renderer,
            },
            bottom_left_origin: true,
            version_major: match preferred_renderer {
                RETRO_HW_CONTEXT_OPENGLES3 => 3,
                RETRO_HW_CONTEXT_OPENGLES2 | RETRO_HW_CONTEXT_OPENGL => 2,
                RETRO_HW_CONTEXT_DIRECT3D => 11, // Direct3D 12 is buggy in RetroArch
                RETRO_HW_CONTEXT_VULKAN => vk::make_version(1, 0, 18),
                _ => 0, // Other video contexts don't need a major version number
            },
            version_minor: match preferred_renderer {
                RETRO_HW_CONTEXT_OPENGLES3 => 1,
                _ => 0, // Other video contexts don't need a minor version number
            },
            cache_context: true,
            debug_context: true,

            depth: false,   // obsolete
            stencil: false, // obsolete

            context_reset: Some(retro_hw_context_reset_callback),
            context_destroy: Some(retro_hw_context_destroyed_callback),

            // Set by the frontend
            get_current_framebuffer: None,
            get_proc_address: None,
        });

        unsafe {
            // Using ctx.set_hw_render doesn't set the proc address
            match environment::set_ptr(
                self.environ_cb.get(),
                RETRO_ENVIRONMENT_SET_HW_RENDER,
                self.hw_render_callback.as_ref().unwrap(),
            ) {
                Some(true) => debug!("{:?}", self.hw_render_callback),
                _ => Err("Failed to get hw render")?,
            };
        }

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
            .with_navigator(NullNavigatorBackend::new())
            .with_video(SoftwareVideoBackend::new())
            .with_autoplay(self.config.autoplay)
            .with_letterbox(self.config.letterbox)
            .with_max_execution_duration(self.config.max_execution_duration)
            .with_warn_on_unsupported_content(self.config.warn_on_unsupported_content)
            .with_viewport_dimensions(dimensions.width, dimensions.height, dimensions.scale_factor)
            .with_fullscreen(true)
            .with_load_behavior(self.config.load_behavior)
            .with_spoofed_url(self.config.spoofed_url.clone());

        let save_directory = unsafe { get_save_directory(self.environ_cb.get()) };
        let builder = match save_directory {
            Some(base_path) => builder.with_storage(RetroVfsStorageBackend::new(base_path, self.vfs.clone())?),
            _ => builder.with_storage(MemoryStorageBackend::new()),
        };

        // Renderer not initialized here, because we can't do so
        // until the frontend calls on_hw_context_reset

        self.player = Pending(builder.into());

        Ok(())
    }

    fn on_unload_game(&mut self, _ctx: &mut UnloadGameContext) {}

    fn on_options_changed(&mut self, ctx: &mut OptionsChangedContext) {
        self.config.autoplay = match ctx.get_variable("ruffle_autoplay") {
            Some("true") => true,
            Some("false") => false,
            _ => defaults::AUTOPLAY,
        };

        self.config.letterbox = match ctx.get_variable("ruffle_letterbox") {
            Some("off") => Letterbox::Off,
            Some("fullscreen") => Letterbox::Fullscreen,
            Some("on") => Letterbox::On,
            _ => defaults::LETTERBOX,
        }; // TODO: Should I reset the driver if this changed?

        self.config.max_execution_duration = ctx
            .get_variable("ruffle_max_execution_duration")
            .and_then(|s: &str| s.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(defaults::MAX_EXECUTION_DURATION);

        self.config.msaa = ctx
            .get_variable("ruffle_msaa")
            .and_then(|s: &str| s.parse::<u8>().ok())
            .unwrap_or(defaults::MSAA);

        self.config.warn_on_unsupported_content = match ctx.get_variable("ruffle_warn_on_unsupported_content") {
            Some("true") => true,
            Some("false") => false,
            _ => defaults::WARN_ON_UNSUPPORTED_CONTENT,
        };

        self.config.file_access_policy = match ctx.get_variable("ruffle_file_access_policy") {
            Some("never") => FileAccessPolicy::Never,
            Some("notify") => FileAccessPolicy::Notify,
            Some("always") => FileAccessPolicy::Always,
            _ => defaults::FILE_ACCESS_POLICY,
        };

        self.config.web_browser_access = match ctx.get_variable("ruffle_web_browser_access") {
            Some("off") => WebBrowserAccess::Ignore,
            Some("off-notify") => WebBrowserAccess::Notify,
            Some("external") => WebBrowserAccess::OpenInBrowser,
            _ => defaults::WEB_BROWSER_ACCESS,
        };

        self.config.sample_rate = ctx
            .get_variable("ruffle_audio_sample_rate")
            .and_then(|s: &str| s.parse::<u32>().ok())
            .unwrap_or(defaults::SAMPLE_RATE);

        self.config.load_behavior = match ctx.get_variable("ruffle_load_behavior") {
            Some("streaming") => LoadBehavior::Streaming,
            Some("blocking") => LoadBehavior::Blocking,
            Some("delayed") => LoadBehavior::Delayed,
            _ => defaults::LOAD_BEHAVIOR,
        };

        if let Active(player) = &self.player {
            let mut player = player.lock().unwrap();

            player.set_letterbox(self.config.letterbox); // TODO: What if old letterbox != new letterbox?
            player.set_max_execution_duration(self.config.max_execution_duration);
        }
    }

    fn on_keyboard_event(&mut self, down: bool, keycode: retro_key, _character: u32, _key_modifiers: retro_mod) {
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

        if let Active(player) = &self.player {
            player.lock().unwrap().handle_event(event);
        }
        // TODO: Add these events to a queue, then give them all to the emulator in the main loop
    }

    fn on_write_audio(&mut self, _ctx: &mut AudioContext) {}

    fn on_hw_context_reset(&mut self) {
        info!("on_hw_context_reset");
        match &self.player {
            Active(_player) => {
                // Game is already running
                // TODO: Refresh all existing backend resources
                if let Some(render_state) = self.render_state.as_mut() {
                    match render_state.reset() {
                        Ok(_) => {
                            info!("Updated hardware render interface in active player");
                        }
                        Err(error) => {
                            error!("Failed to update hardware render interface in active player: {error}");
                        }
                    };
                } else {
                    warn!("Reset hardware context before hw_render_callback was ready");
                }
            }
            Pending(builder) => {
                // Game is waiting for hardware context to be ready
                match self.get_render_backend() {
                    Ok((backend, render_state)) => {
                        // We take ownership of the builder, then throw it out after the player is built
                        self.player = Active(builder.take().with_renderer(backend).build());
                        self.render_state = render_state.into();
                        info!("Initialized player with render backend");
                    }
                    Err(error) => {
                        error!("Failed to initialize render backend: {error}");
                    }
                };
            }
            PlayerState::Uninitialized => {
                debug!("Resetting hardware context before core is ready (this is normal)");
            }
        };
    }

    fn on_hw_context_destroyed(&mut self) {}

    fn on_core_options_update_display(&mut self) -> bool {
        todo!()
    }
}
