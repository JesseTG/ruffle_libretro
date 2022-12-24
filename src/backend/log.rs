use ruffle_core::backend::log::LogBackend;


pub struct RetroLogBackend {
}

impl RetroLogBackend {
    pub fn new() -> Self {
        Self {}
    }
}

impl LogBackend for RetroLogBackend {
    fn avm_trace(&self, message: &str) {
        log::info!("{}", message);
    }
}