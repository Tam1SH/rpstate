use amethystate::StoreBuilder;
use amethystate::amethystate;

#[amethystate(prefix = "network", version = 1)]
pub struct NetworkState {
    #[amestate(default = "127.0.0.1".to_string())]
    pub host: String,

    #[amestate(default = 8080)]
    pub port: u16,
}

#[amethystate(prefix = "ui", version = 1)]
pub struct UiState {
    #[amestate(default = "dark".to_string())]
    pub theme: String,

    #[amestate(default = true)]
    pub sidebar_visible: bool,
}

fn main() -> anyhow::Result<()> {
    let store = StoreBuilder::new("./test_data").build()?;

    let network = NetworkState::new_with(&store)?;
    network.host().set("10.0.0.1".to_string())?;
    network.port().set(9090)?;

    let ui = UiState::new_with(&store)?;
    ui.theme().set("light".to_string())?;

    println!("produced test_data.toml");
    Ok(())
}
