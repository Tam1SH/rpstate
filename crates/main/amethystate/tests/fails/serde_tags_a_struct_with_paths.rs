use amethystate::amethystate;

#[amethystate(prefix = "cfg")]
#[serde(tag = "kind")]
pub struct Cfg {
    #[amestate(default = "localhost".to_string())]
    pub host: String,
}

fn main() {}
