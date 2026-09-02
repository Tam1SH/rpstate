use amethystate::amethystate;

#[amethystate(prefix = "cfg")]
#[serde(rename = "SomethingElse")]
pub struct Cfg {
    #[amestate(default = 8080u16)]
    pub port: u16,
}

fn main() {}
