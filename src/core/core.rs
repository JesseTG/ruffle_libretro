use std::borrow::BorrowMut;
use std::error::Error;
use std::ffi::{CStr, CString};
use std::slice::from_raw_parts;
use std::sync::Arc;

use log::error;
use ruffle_core::backend::navigator::NullNavigatorBackend;
use ruffle_core::backend::storage::MemoryStorageBackend;
use ruffle_core::events::KeyCode;
use ruffle_core::tag_utils::SwfMovie;
use ruffle_core::{PlayerBuilder, PlayerEvent};
use ruffle_render::backend::null::NullRenderer;
use ruffle_render::backend::ViewportDimensions;
use ruffle_render_wgpu::backend::WgpuRenderBackend;
use ruffle_render_wgpu::descriptors::Descriptors;
use ruffle_video_software::backend::SoftwareVideoBackend;
use rust_libretro::contexts::*;
use rust_libretro::core::Core;
use rust_libretro::environment::{get_save_directory, set_pixel_format};
use rust_libretro::sys::{
    retro_game_geometry, retro_game_info, retro_hw_context_type, retro_hw_render_callback, retro_hw_render_interface,
    retro_key, retro_mod, retro_pixel_format, retro_proc_address_t, retro_system_av_info, retro_system_timing,
};
use rust_libretro::types::{PixelFormat, SystemInfo};
use rust_libretro::{retro_hw_context_destroyed_callback, retro_hw_context_reset_callback};
use wgpu::Adapter;

use crate::backend::audio::RetroAudioBackend;
use crate::backend::log::RetroLogBackend;
use crate::backend::navigator::RetroNavigatorBackend;
use crate::backend::render::gl::RetroRenderGlowBackend;
use crate::backend::storage::RetroVfsStorageBackend;
use crate::backend::ui::RetroUiBackend;
use crate::core::Ruffle;
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
        if !initial {
            return;
        }

        if !ctx.enable_proc_address_interface() {
            error!("enable_proc_address_interface failed");
            return;
        }
        ctx.set_support_no_game(false);
        self.vfs_interface_version = match ctx.enable_vfs_interface(3) {
            Ok(version) => Some(version),
            Err(error) => {
                error!("[ruffle] Failed to initialize VFS interface: {error}");
                None
            }
        };
    }

    fn on_init(&mut self, _ctx: &mut InitContext) {
        todo!()
    }

    fn on_deinit(&mut self, _ctx: &mut DeinitContext) {
        todo!()
    }

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
        let mut player = self.player.expect("TODO").get_mut().unwrap();

        player.run_frame();
        ctx.draw_hardware_frame(0, 0, 0);

        // TODO: Write out audio
        // TODO: React to changed settings
    }

    fn on_load_game(&mut self, game: Option<retro_game_info>, ctx: &mut LoadGameContext) -> Result<(), Box<dyn Error>> {
        let game = game.ok_or_else(|| "No game was provided")?;
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
            if !ctx.set_hw_render(hw_render) {
                return Err("OpenGL context not supported".into());
            }
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

        let get_proc = hw_render.get_proc_address.unwrap();
        let descriptors: Descriptors = futures::executor::block_on(match hw_render.context_type {
            retro_hw_context_type::RETRO_HW_CONTEXT_OPENGL
            | retro_hw_context_type::RETRO_HW_CONTEXT_OPENGLES2
            | retro_hw_context_type::RETRO_HW_CONTEXT_OPENGLES3
            | retro_hw_context_type::RETRO_HW_CONTEXT_OPENGL_CORE
            | retro_hw_context_type::RETRO_HW_CONTEXT_OPENGLES_VERSION => unsafe {
                WgpuRenderBackend::build_descriptors_for_gl(get_proc, None)?
            },
            _ => Err("Context not available")?,
        });
        let descriptors = Arc::new(descriptors);

        let ctx = GenericContext::from(ctx);
        let builder = PlayerBuilder::new()
            .with_movie(movie)
            .with_ui(RetroUiBackend::new(ctx))
            .with_log(RetroLogBackend::new())
            .with_audio(RetroAudioBackend::new(2, SAMPLE_RATE as u32))
            .with_renderer(WgpuRenderBackend::new(descriptors))
            .with_navigator(NullNavigatorBackend::new())
            .with_video(SoftwareVideoBackend::new())
            .with_storage(MemoryStorageBackend::new())
            .with_autoplay(self.config.autoplay)
            .with_letterbox(self.config.letterbox)
            .with_max_execution_duration(self.config.max_execution_duration)
            .with_warn_on_unsupported_content(self.config.warn_on_unsupported_content)
            .with_viewport_dimensions(dimensions.width, dimensions.height, dimensions.scale_factor)
            .with_fullscreen(true)
            .with_load_behavior(self.config.load_behavior)
            .with_spoofed_url(self.config.spoofed_url.clone());

        // let environment_callback = unsafe { ctx.environment_callback() };
        // let save_directory = unsafe { get_save_directory(*environment_callback) };
        // let builder = match (save_directory, self.vfs_interface_version) {
        //     (Some(base_path), Some(_)) => builder.with_storage(RetroVfsStorageBackend::new(base_path, ctx)?),
        //     _ => builder.with_storage(MemoryStorageBackend::new()),
        // };

        let player = builder.build();
        player.into_inner().unwrap().set_is_playing(true);
        self.player = Some(player);

        Ok(())
    }

    fn on_unload_game(&mut self, _ctx: &mut UnloadGameContext) {
        self.player.expect("TODO").into_inner().unwrap().destroy();
    }

    fn on_options_changed(&mut self, ctx: &mut OptionsChangedContext) {
        match ctx.get_variable("ruffle_autoplay") {};
        todo!()
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
        self.player.expect("TODO").into_inner().unwrap().handle_event(event);
        // TODO: Add these events to a queue, then give them all to the emulator in the main loop
    }

    fn on_write_audio(&mut self, ctx: &mut AudioContext) {
        let player = self.player.expect("TODO").into_inner().unwrap().audio().borrow_mut();
    }

    fn on_hw_context_reset(&mut self) {
        /*
        When the frontend has created a context or reset the context, retro_hw_context_reset_t is called.
        Here, OpenGL resources can be initialized. The frontend can reset the context at will
        (e.g. when changing from fullscreen to windowed mode and vice versa).
        The core should take this into account. It will be notified when reinitialization needs to happen.
         */
        todo!()
    }

    fn on_hw_context_destroyed(&mut self) {
        todo!()
    }

    fn on_core_options_update_display(&mut self) -> bool {
        todo!()
    }
}

const SAMPLE_RATE: f64 = 44100.0;
