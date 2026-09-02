use amethystate::amethystate;

#[amethystate(prefix = "net")]
pub struct Net {
    #[serde(alias = "listen_port")]
    #[amestate(default = 8080u16)]
    pub port: u16,
}

fn main() {}
