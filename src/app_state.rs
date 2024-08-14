use once_cell::sync::Lazy;
use std::sync::Mutex;

pub static APP_STATE: Lazy<Mutex<AppState>> = Lazy::new(|| {
    Mutex::new(AppState::default())
});

pub struct AppState {
}

impl Default for AppState {
    fn default() -> Self {
        Self {}
    }
}