use amethystate::amethystate;

#[amethystate(prefix = "cfg")]
pub struct Cfg {
    #[serde(flatten)]
    #[amestate(default = "localhost".to_string())]
    pub host: String,
}

fn main() {}
