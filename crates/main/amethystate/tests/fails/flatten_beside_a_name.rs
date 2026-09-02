use amethystate::amethystate;

#[amethystate]
pub struct Inner {
    #[amestate(default = "localhost".to_string())]
    pub host: String,
}

#[amethystate(prefix = "cfg")]
pub struct Cfg {
    #[serde(flatten, rename = "net")]
    #[amestate(nested)]
    pub inner: Inner,
}

fn main() {}
