use std::time::Duration;

pub enum FileAccessPolicy {
    Never,
    Notify,
    Always,
}

pub enum WebBrowserAccess {
    Ignore,
    Notify,
    OpenInBrowser,
}
