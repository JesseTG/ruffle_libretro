use std::ffi::{CStr, CString};
use std::path::PathBuf;
use std::time::Duration;

use log::{debug, info, warn, Level, LevelFilter};
use ruffle_core::FloatDuration;
use ruffle_render::quality::StageQuality;
use rust_libretro::anyhow::{self, bail, Context};
use rust_libretro::contexts::*;
use rust_libretro::core::Core;
use rust_libretro::environment;
use rust_libretro::sys::*;
use rust_libretro::types::{PixelFormat, SystemInfo};

use crate::backend::render::read_frame;
use crate::built_info;
use crate::core::config::WebBrowserAccess;
use crate::core::session::{Session, FPS};
use crate::core::{input, Ruffle};
use crate::util::message;

impl Core for Ruffle {
    fn get_info(&self) -> SystemInfo {
        SystemInfo {
            library_name: CString::new("Ruffle").unwrap(),
            library_version: CString::new(built_info::PKG_VERSION).unwrap(),
            valid_extensions: CString::new("swf").unwrap(),
            // Movies load neighbouring files relative to their own URL, so Ruffle needs a real path.
            need_fullpath: true,
            block_extract: false,
        }
    }

    fn on_get_av_info(&mut self, _ctx: &mut GetAvInfoContext) -> retro_system_av_info {
        let (width, height) = self
            .session
            .as_ref()
            .map(|session| (session.width, session.height))
            .unwrap_or((550, 400));

        retro_system_av_info {
            geometry: retro_game_geometry {
                base_width: width,
                base_height: height,
                max_width: width,
                max_height: height,
                aspect_ratio: width as f32 / height as f32,
            },
            timing: retro_system_timing {
                fps: f64::from(FPS),
                sample_rate: f64::from(self.config.sample_rate),
            },
        }
    }

    fn on_set_environment(&mut self, ctx: &mut SetEnvironmentContext) {
        if let Err(e) = ctx.set_support_no_game(false) {
            warn!("RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME failed: {e}");
        }

        let ctx = GenericContext::from(ctx);
        self.environ_cb = unsafe { *ctx.environment_callback() };

        if let Err(e) = ctx.set_controller_info(input::CONTROLLER_INFO) {
            warn!("RETRO_ENVIRONMENT_SET_CONTROLLER_INFO failed: {e}");
        }
    }

    fn on_init(&mut self, _ctx: &mut InitContext) {
        #[cfg(feature = "profiler")]
        profiling::tracy_client::Client::start();

        // rust-libretro enables every level, which would send wgpu's per-call
        // trace output through the frontend's log. RUST_LOG still overrides this.
        if std::env::var_os("RUST_LOG").is_none() {
            log::set_max_level(LevelFilter::Info);
        }
    }

    fn on_set_controller_port_device(&mut self, port: u32, device: u32, _ctx: &mut GenericContext) {
        if port != 0 {
            return;
        }

        debug!("Port 0 device set to {device}");
        self.port_device = device;
        if device != RETRO_DEVICE_JOYPAD {
            if let Some(session) = &mut self.session {
                session.gamepad.release_all(&mut self.queued_events);
            }
        }
    }

    fn on_reset(&mut self, _ctx: &mut ResetContext) {
        // Ruffle can't rewind a movie to its initial state, so load it again instead.
        let Some(path) = self.content_path.clone() else {
            return;
        };

        if let Some(session) = self.session.take() {
            session.flush();
        }
        self.queued_events.clear();

        match Session::new(&path, &self.config, self.environ_cb, self.save_directory.clone()) {
            Ok(session) => self.session = Some(session),
            Err(e) => message::show(self.environ_cb, &format!("Ruffle: {e:#}"), 5000, Level::Error),
        }
    }

    fn on_run(&mut self, ctx: &mut RunContext, _delta_us: Option<i64>) {
        profiling::scope!("retro_run");
        let Some(session) = &mut self.session else {
            return;
        };

        // Let finished loads and other background work update the player first.
        session.run_futures();

        let rendered = {
            profiling::scope!("Player");
            let _guard = session.runtime.enter();
            let mut player = session.player.lock().expect("player lock poisoned");

            session
                .mouse
                .update(ctx, session.width, session.height, &mut self.queued_events);
            if self.port_device == RETRO_DEVICE_JOYPAD {
                session.gamepad.update(ctx, &mut self.queued_events);
            }
            for event in self.queued_events.drain(..) {
                player.handle_event(event);
            }

            player.tick(FloatDuration::from_millis(1000.0 / f64::from(FPS)));

            if player.needs_render() {
                player.render();
                read_frame(&mut player, &mut session.frame)
            } else {
                false
            }
        };

        if rendered || !session.has_frame || !ctx.can_dupe() {
            session.has_frame = true;
            let pitch = session.width as usize * 4;
            ctx.draw_frame(&session.frame, session.width, session.height, pitch);
        } else {
            ctx.dupe_frame();
        }

        let samples = session.audio.mix_frame();
        AudioContext::from(&mut *ctx).batch_audio_samples(samples);

        profiling::finish_frame!();
    }

    fn on_load_game(&mut self, game: Option<retro_game_info>, ctx: &mut LoadGameContext) -> anyhow::Result<()> {
        ctx.set_pixel_format(PixelFormat::XRGB8888)?;

        let Some(game) = game else {
            bail!("No game was provided");
        };
        if game.path.is_null() {
            bail!("The frontend didn't provide a path to the content");
        }
        let path = unsafe { CStr::from_ptr(game.path) }
            .to_str()
            .context("Content path isn't valid UTF-8")?;
        let path = PathBuf::from(path);

        let ctx = GenericContext::from(ctx);
        if let Err(e) = ctx.set_input_descriptors(&input::input_descriptors()) {
            warn!("RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS failed: {e}");
        }
        if let Err(e) = ctx.enable_keyboard_callback() {
            warn!("RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK failed, keyboard input won't work: {e}");
        }

        self.save_directory = match unsafe { environment::get_save_directory(self.environ_cb) } {
            Ok(Some(dir)) => Some(dir.to_path_buf()),
            _ => {
                warn!("No save directory available, SharedObjects won't persist");
                None
            }
        };

        match Session::new(&path, &self.config, self.environ_cb, self.save_directory.clone()) {
            Ok(session) => {
                self.session = Some(session);
                self.content_path = Some(path);
                Ok(())
            }
            Err(e) => {
                message::show(self.environ_cb, &format!("Ruffle: {e:#}"), 5000, Level::Error);
                Err(e)
            }
        }
    }

    fn on_unload_game(&mut self, _ctx: &mut UnloadGameContext) {
        if let Some(session) = self.session.take() {
            session.flush();
        }
        self.content_path = None;
        self.queued_events.clear();
    }

    fn on_options_changed(&mut self, ctx: &mut OptionsChangedContext) {
        let config = &mut self.config;

        let quality = match ctx.get_variable("ruffle_quality") {
            Ok(Some("low")) => StageQuality::Low,
            Ok(Some("medium")) => StageQuality::Medium,
            Ok(Some("best")) => StageQuality::Best,
            Ok(Some("high8x8")) => StageQuality::High8x8,
            Ok(Some("high16x16")) => StageQuality::High16x16,
            _ => StageQuality::High,
        };

        let max_execution_duration = match ctx.get_variable("ruffle_max_execution_duration") {
            Ok(Some("unlimited")) => Duration::MAX,
            Ok(Some(value)) => value.parse().map(Duration::from_secs).unwrap_or(Duration::from_secs(15)),
            _ => Duration::from_secs(15),
        };

        config.web_browser_access.set(match ctx.get_variable("ruffle_web_browser_access") {
            Ok(Some("off")) => WebBrowserAccess::Ignore,
            Ok(Some("external")) => WebBrowserAccess::OpenInBrowser,
            _ => WebBrowserAccess::Notify,
        });

        config.spoofed_url = match ctx.get_variable("ruffle_spoofed_url") {
            Ok(Some(url)) if url != "none" => Some(url.to_owned()),
            _ => None,
        };

        // The frontend fixes the sample rate when content loads, so only read it before then.
        if self.session.is_none() {
            config.sample_rate = match ctx.get_variable("ruffle_audio_sample_rate") {
                Ok(Some("48000")) => 48000,
                _ => 44100,
            };
        }

        // Only push settings that changed; content may have picked its own quality since.
        if let Some(session) = &self.session {
            let mut player = session.player.lock().expect("player lock poisoned");
            if quality != config.quality {
                info!("Changing quality to {quality:?}");
                player.set_quality(quality);
            }
            if max_execution_duration != config.max_execution_duration {
                player.set_max_execution_duration(max_execution_duration);
            }
        }

        config.quality = quality;
        config.max_execution_duration = max_execution_duration;
    }

    fn on_keyboard_event(&mut self, down: bool, keycode: retro_key, character: u32, key_modifiers: retro_mod) {
        self.keyboard
            .handle(down, keycode, character, key_modifiers, &mut self.queued_events);
    }
}
