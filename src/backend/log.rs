use ruffle_core::backend::log::LogBackend;

/// Sends ActionScript `trace()` output and AVM warnings to the frontend's log.
pub struct RetroLogBackend;

impl LogBackend for RetroLogBackend {
    fn avm_trace(&self, message: &str) {
        log::info!(target: "avm_trace", "{message}");
    }

    fn avm_warning(&self, message: &str) {
        log::warn!(target: "avm_warning", "{message}");
    }
}
