use amethystate::amethystate;

#[amethystate(prefix = "cfg")]
#[serde(deny_unknown_fields)]
pub struct Cfg {
    #[amestate(default = "localhost".to_string())]
    pub host: String,
}

fn main() {}
