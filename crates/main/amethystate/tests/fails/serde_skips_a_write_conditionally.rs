use amethystate::amethystate;

#[amethystate(prefix = "net")]
pub struct Net {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[amestate(default = None)]
    pub proxy: Option<String>,
}

fn main() {}
