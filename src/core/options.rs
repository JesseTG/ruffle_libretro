use rust_libretro::contexts::SetEnvironmentContext;
use rust_libretro::core::CoreOptions;
use crate::core::Ruffle;

impl<'a> CoreOptions for Ruffle<'a>
{
    fn set_core_options(&self, _ctx: &SetEnvironmentContext) -> bool {
        todo!()
    }
}

