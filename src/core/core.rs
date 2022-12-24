use std::borrow::BorrowMut;
use std::error::Error;
use std::ffi::{c_uint, c_void, CString};
use std::slice;

use ruffle_core::{PlayerBuilder, PlayerEvent};
use ruffle_core::backend::storage::MemoryStorageBackend;
use ruffle_core::events::KeyCode;
use ruffle_core::tag_utils::SwfMovie;
use rust_libretro::contexts::*;
use rust_libretro::core::Core;
use rust_libretro::environment::get_save_directory;
use rust_libretro::sys::{retro_game_geometry, retro_game_info, retro_key, retro_mod, retro_system_av_info, retro_system_timing, size_t};
use rust_libretro::types::SystemInfo;

use crate::{built_info, util};
use crate::backend::audio::RetroAudioBackend;
use crate::backend::log::RetroLogBackend;
use crate::backend::navigator::RetroNavigatorBackend;
use crate::backend::render::RetroRenderBackend;
use crate::backend::storage::RetroVfsStorageBackend;
use crate::backend::ui::RetroUiBackend;
use crate::core::Ruffle;

impl<'a> Core for Ruffle<'a>
{
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
        self.av_info.expect("Not called until after initialization")
    }

    fn on_set_environment(&mut self, initial: bool, ctx: &mut SetEnvironmentContext) {
        if !initial {
            return;
        }

        ctx.set_support_no_game(false);
        self.vfs_interface_version = match ctx.enable_vfs_interface(3)
        {
            Ok(version) => Some(version),
            Err(error) => {
                log::error!("[ruffle] Failed to initialize VFS interface: {error}");
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
        // TODO: Read input
        // TODO: Handle input
        // TODO: Step engine
        // TODO: Render graphics
        // TODO: Write out audio
        // TODO: React to changed settings
        ctx.poll_input();
        ctx.get_joypad_state(0, 0);
        let ctx: AudioContext = ctx.into();
        let &mut player = self.player.expect("TODO").get_mut().unwrap();

        self.player.expect("TODO").into_inner().unwrap().run_frame();
    }

    fn get_serialize_size(&mut self, _ctx: &mut GetSerializeSizeContext) -> size_t {
        todo!()
    }


    fn on_load_game(&mut self, game: Option<retro_game_info>, ctx: &mut LoadGameContext) -> Result<(), Box<dyn Error>> {
        if let Some(game) = game {
            let buffer = unsafe { slice::from_raw_parts(game.data as *const u8, game.size as usize) };
            let movie = SwfMovie::from_data(buffer, None, None)?;
            let movie_size = (movie.width().to_pixels(), movie.height().to_pixels());
            self.context = Some(GenericContext::from(ctx));

            self.av_info = Some(retro_system_av_info {
                geometry: retro_game_geometry {
                    base_width: movie_size.0.round() as u32,
                    base_height: movie_size.1.round() as u32,
                    max_width: movie_size.0.round() as u32,
                    max_height: movie_size.1.round() as u32,
                    aspect_ratio: (movie_size.0 / movie_size.1) as f32,
                },
                timing: retro_system_timing {
                    fps: f64::from(movie.frame_rate()),
                    sample_rate: SAMPLE_RATE, // TODO: Configure
                },
            });

            let builder = PlayerBuilder::new()
                .with_movie(movie)
                .with_ui(RetroUiBackend::new(self.context.as_ref().unwrap()))
                .with_log(RetroLogBackend::new())
                .with_audio(RetroAudioBackend::new(2, SAMPLE_RATE as u32))
                .with_renderer(RetroRenderBackend::new())
                .with_navigator(RetroNavigatorBackend::new());

            let environment_callback = unsafe {self.context.unwrap().environment_callback()};
            let builder = match (unsafe { get_save_directory(*environment_callback) }, self.vfs_interface_version)
            {
                (Some(base_path), Some(_)) => builder.with_storage(
                    RetroVfsStorageBackend::new(base_path, ctx)
                ),
                _ => builder.with_storage(MemoryStorageBackend::new()),
            };

            self.player = Some(builder.build());
            self.player.expect("TODO").into_inner().unwrap().set_is_playing(true);
        }


        return Ok(()); // TODO: Return an error here
    }

    fn on_unload_game(&mut self, _ctx: &mut UnloadGameContext) {
        self.player.expect("TODO").into_inner().unwrap().destroy();
    }


    fn get_memory_data(&mut self, _id: c_uint, _ctx: &mut GetMemoryDataContext) -> *mut c_void {
        todo!()
    }

    fn get_memory_size(&mut self, _id: c_uint, _ctx: &mut GetMemorySizeContext) -> size_t {
        todo!()
    }

    fn on_options_changed(&mut self, _ctx: &mut OptionsChangedContext) {
        todo!()
    }

    fn on_keyboard_event(&mut self, down: bool, keycode: retro_key, character: u32, key_modifiers: retro_mod) {
        let event = match (down, keycode) {
            (true, keycode) => PlayerEvent::KeyDown {
                key_code: <KeyCode as util::From<retro_key>>::from(keycode),
                key_char: None,
            },
            (false, keycode) => PlayerEvent::KeyUp {
                key_code: <KeyCode as util::From<retro_key>>::from(keycode),
                key_char: None,
            }
        };
        self.player.expect("TODO").into_inner().unwrap().handle_event(event);
        // TODO: Add these events to a queue, then give them all to the emulator in the main loop
    }

    fn on_write_audio(&mut self, ctx: &mut AudioContext) {
        let player = self.player.expect("TODO").into_inner().unwrap().audio().borrow_mut();
    }

    fn on_hw_context_reset(&mut self) {
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
