use amethystate_macros::amethystate;

#[amethystate(prefix = "editor")]
#[serde(crate = "other::serde")]
pub struct Editor {
    #[amestate(default = 1280)]
    pub width: u32,
}

fn main() {}
