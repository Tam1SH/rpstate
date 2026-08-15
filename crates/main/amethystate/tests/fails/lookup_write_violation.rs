use amethystate_macros::amethystate;

#[amethystate(prefix = "net")]
pub struct NetworkState {
    #[amestate(default = "127.0.0.1".to_string())]
    pub host: String,
}

#[amethystate(prefix = "ui")]
pub struct UiState {
    #[amestate(lookup = "host", parent = NetworkState, export_mut)]
    pub proxy_host: String,
}

fn main() {
    let dummy_field: amethystate::Field<u16, amethystate::ReadOnlyMode> =
        unsafe { std::mem::zeroed() };
    dummy_field.set(9090);
}
