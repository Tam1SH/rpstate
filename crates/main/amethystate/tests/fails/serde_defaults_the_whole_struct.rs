use amethystate_macros::amethystate;
use serde::{Deserialize, Serialize};

#[amethystate(prefix = "editor")]
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Editor {
    #[amestate(default = 1280)]
    pub width: u32,
}

fn main() {}
