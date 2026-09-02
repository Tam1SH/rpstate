use amethystate::amethystate;

#[amethystate(prefix = "cfg")]
pub struct Cfg {
    #[serde(rename = "nowhere")]
    #[amestate(volatile, default = 1u32)]
    pub tick: u32,
}

fn main() {}
