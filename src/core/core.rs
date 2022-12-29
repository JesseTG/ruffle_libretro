use std::borrow::Borrow;
use std::cell::Cell;
use std::error::Error;
use std::ffi::CString;
use std::mem::transmute;
use std::ops::Deref;
use std::ptr;
use std::slice::from_raw_parts;
use std::sync::Arc;
use std::time::Duration;

use log::{debug, error, info, trace};
use ruffle_core::backend::navigator::NullNavigatorBackend;
use ruffle_core::backend::storage::MemoryStorageBackend;
use ruffle_core::config::Letterbox;
use ruffle_core::tag_utils::SwfMovie;
use ruffle_core::{LoadBehavior, PlayerBuilder, PlayerEvent};
use ruffle_render::backend::ViewportDimensions;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_video_software::backend::SoftwareVideoBackend;
use rust_libretro::contexts::*;
use rust_libretro::core::Core;
use rust_libretro::environment::get_save_directory;
use rust_libretro::sys::*;
use rust_libretro::types::{PixelFormat, SystemInfo};
use rust_libretro::{environment, retro_hw_context_destroyed_callback, retro_hw_context_reset_callback};

use crate::backend::audio::RetroAudioBackend;
use crate::backend::log::RetroLogBackend;
use crate::backend::render::target::RetroRenderTarget;
use crate::backend::storage::RetroVfsStorageBackend;
use crate::backend::ui::RetroUiBackend;
use crate::core::config::defaults;
use crate::core::Ruffle;
use crate::options::{FileAccessPolicy, WebBrowserAccess};
use crate::{built_info, util};

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

    fn on_get_av_info(&mut self, ctx: &mut GetAvInfoContext) -> retro_system_av_info {
        self.av_info.expect("Shouldn't be called until after on_load_game")
    }

    fn on_set_environment(&mut self, initial: bool, ctx: &mut SetEnvironmentContext) {
        if initial {
            ctx.set_support_no_game(false);
            let vfs_interface_version = match { unsafe { ctx.enable_vfs_interface(3) } } {
                Ok(version) => Some(version),
                Err(error) => {
                    error!("Failed to initialize VFS interface: {error}");
                    None
                }
            };
        }

        self.environ_cb.set({
            let ctx = GenericContext::from(ctx);
            let environ_cb = unsafe { ctx.environment_callback() };
            if let None = environ_cb {
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

    fn on_run(&mut self, ctx: &mut RunContext, delta_us: Option<i64>) {
        ctx.poll_input();
        // TODO: Handle input
        ctx.get_joypad_state(0, 0);
        if let Some(player) = self.player.borrow() {
            let mut player = player.lock().unwrap();

            player.run_frame();
        }

        let av_info = self.av_info.expect("av_info should've been initialized by now");
        ctx.draw_hardware_frame(av_info.geometry.max_width, av_info.geometry.max_height, 0);

        // TODO: Write out audio
        // TODO: React to changed settings
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

        let hw_render = retro_hw_render_callback {
            context_type: retro_hw_context_type::RETRO_HW_CONTEXT_OPENGL,
            bottom_left_origin: true,
            version_major: 4,
            version_minor: 0,
            cache_context: true,
            debug_context: true,

            depth: false,   // obsolete
            stencil: false, // obsolete

            context_reset: Some(retro_hw_context_reset_callback),
            context_destroy: Some(retro_hw_context_destroyed_callback),

            // Set by the frontend
            get_current_framebuffer: None,
            get_proc_address: None,
        };

        unsafe {
            // Using ctx.set_hw_render doesn't set the proc address
            match environment::set_ptr(self.environ_cb.get(), RETRO_ENVIRONMENT_SET_HW_RENDER, &hw_render) {
                Some(true) => {}
                _ => return Err("Failed to get hw render".into()),
            };
        }
        debug!("{hw_render:?}");

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

        let get_proc_address = hw_render.get_proc_address.unwrap();
        let descriptors = futures::executor::block_on(match hw_render.context_type {
            retro_hw_context_type::RETRO_HW_CONTEXT_OPENGL
            | retro_hw_context_type::RETRO_HW_CONTEXT_OPENGLES2
            | retro_hw_context_type::RETRO_HW_CONTEXT_OPENGLES3
            | retro_hw_context_type::RETRO_HW_CONTEXT_OPENGL_CORE
            | retro_hw_context_type::RETRO_HW_CONTEXT_OPENGLES_VERSION => unsafe {
                WgpuRenderBackend::<RetroRenderTarget>::build_descriptors_for_gl(
                    |sym| {
                        CString::new(sym)
                            .ok() // Get the symbol name ready for C...
                            .and_then(|sym| {
                                let address = get_proc_address(sym.as_ptr());
                                trace!("get_proc_address({sym:?}) = {address:?}");
                                address
                            }) // Then get the function address from libretro...
                            .map(|f| f as *const libc::c_void) // Then cast it to the right pointer type...
                            .unwrap_or(ptr::null()) // ...or if all else fails, return a null pointer (gl will handle it)
                    },
                    None,
                )
            },
            _ => Err("Context not available")?,
        })?;
        let builder = PlayerBuilder::new()
            .with_movie(movie)
            .with_ui(RetroUiBackend::new(self.environ_cb.clone()))
            .with_log(RetroLogBackend::new())
            .with_audio(RetroAudioBackend::new(2, self.config.sample_rate))
            //.with_renderer(WgpuRenderBackend::new(Arc::new(descriptors), RetroRenderTarget::new())?)
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

        let mut player = builder.build();
        player.lock().unwrap().set_is_playing(true);
        self.player = Some(player);

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

        if let Some(player) = &self.player {
            let mut player = player.lock().unwrap();

            player.set_letterbox(self.config.letterbox); // TODO: What if old letterbox != new letterbox?
            player.set_max_execution_duration(self.config.max_execution_duration);
        }
    }

    fn on_keyboard_event(&mut self, down: bool, keycode: retro_key, character: u32, key_modifiers: retro_mod) {
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
        self.player.as_mut().unwrap().lock().unwrap().handle_event(event);
        // TODO: Add these events to a queue, then give them all to the emulator in the main loop
    }

    fn on_write_audio(&mut self, ctx: &mut AudioContext) {}

    fn on_hw_context_reset(&mut self) {
        /*
        When the frontend has created a context or reset the context, retro_hw_context_reset_t is called.
        Here, OpenGL resources can be initialized. The frontend can reset the context at will
        (e.g. when changing from fullscreen to windowed mode and vice versa).
        The core should take this into account. It will be notified when reinitialization needs to happen.
         */
        // TODO: Initialize the function pointers and rendering backend here
    }

    fn on_hw_context_destroyed(&mut self) {
        todo!()
    }

    fn on_core_options_update_display(&mut self) -> bool {
        todo!()
    }
}
