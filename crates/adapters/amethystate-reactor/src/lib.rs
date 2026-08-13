//! [`windows-reactor`](https://github.com/microsoft/windows-rs) bindings for amethystate.
//!
//! Reactor renders from hook state on a single UI thread, and amethystate
//! delivers changes from whichever thread wrote them - including a file watcher
//! for edits made outside the process. [`AmeCx::use_ame`] bridges the two: it
//! mirrors a reactive source into component state, using the host's
//! `UiMarshaller` to cross the thread boundary.
//!
//! ```rust,ignore
//! use amethystate_reactor::AmeCx;
//!
//! fn app(cx: &mut RenderCx) -> Element {
//!     let state: NetworkState = cx.use_ame_state();
//!     let port = cx.use_ame(&state.port());
//!
//!     let bump = state.port();
//!     vstack((
//!         text_block(format!("port = {port}")),
//!         button("+1").on_click(move || {
//!             let _ = bump.set(port + 1);
//!         }),
//!     ))
//!     .into()
//! }
//! ```
//!
//! Writes go straight to the field, not through a setter the hook hands back -
//! the component already holds the handle, and the write comes back as a change
//! like any other.

mod hooks;
mod observe;

pub use hooks::AmeCx;
pub use observe::{Entry, Observe, entry};
