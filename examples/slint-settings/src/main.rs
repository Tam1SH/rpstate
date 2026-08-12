use amethystate::{IntoPipeline, ReactiveScope, StoreBuilder, amethystate};
use slint::ComponentHandle;

slint::include_modules!();

#[amethystate(prefix = "slint_settings")]
pub struct SettingsState {
    #[amestate(default = "127.0.0.1".to_string())]
    pub host: String,

    #[amestate(default = 8080)]
    pub port: u16,
}

fn main() -> Result<(), slint::PlatformError> {
    let store = StoreBuilder::new("./slint-settings.redb")
        .build()
        .expect("failed to open store");
    let state = SettingsState::new(&store).expect("failed to create settings");

    let ui = AppWindow::new()?;
    ui.set_host(state.host().get().into());
    ui.set_port_text(state.port().get().to_string().into());

    let address = (state.host(), state.port())
        .pipe()
        .map(|(host, port)| format!("{host}:{port}"))
        .dedupe();

    ui.set_address(address.get().into());

    let state_for_apply = state.clone();
    ui.on_apply(move |host, port_text| {
        let _ = state_for_apply.host().set(host.to_string());
        if let Ok(port) = port_text.parse::<u16>() {
            let _ = state_for_apply.port().set(port);
        }
    });

    let mut scope = ReactiveScope::new();
    let ui_weak = ui.as_weak();
    scope.watch(address.subscribe(move |address| {
        let ui_weak = ui_weak.clone();
        // Subscribers borrow the value; this one outlives the callback by
        // hopping onto the event loop, so it needs its own copy.
        let address = address.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_address(address.into());
            }
        });
    }));

    ui.run()
}
