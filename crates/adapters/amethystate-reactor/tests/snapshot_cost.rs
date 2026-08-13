use amethystate::{ReactiveCell, SignalSubscription};
use amethystate_reactor::{AmeCx, Observe};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_reactor::{ChannelDispatcher, RenderCx};

/// Counts how often the hook asks the source for a value.
#[derive(Clone)]
struct Counted {
    cell: ReactiveCell<u64>,
    reads: Arc<AtomicUsize>,
}

impl Observe for Counted {
    type Value = u64;

    fn snapshot(&self) -> u64 {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.cell.get()
    }

    fn watch<F>(&self, on_change: F) -> SignalSubscription
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.cell.subscribe(move |_| on_change())
    }
}

#[test]
fn a_quiet_render_does_not_read_the_source() {
    let dispatcher = ChannelDispatcher::new();
    let mut cx = RenderCx::for_test();
    cx.set_marshaller(Some(dispatcher.marshaller()));

    let source = Counted {
        cell: ReactiveCell::new(1u64),
        reads: Arc::new(AtomicUsize::new(0)),
    };

    cx.begin_render();
    assert_eq!(cx.use_ame(&source), 1);
    cx.flush_effects();

    let after_mount = source.reads.load(Ordering::Relaxed);

    for _ in 0..5 {
        cx.begin_render();
        cx.use_ame(&source);
    }

    assert_eq!(
        source.reads.load(Ordering::Relaxed),
        after_mount,
        "renders with nothing to report must not re-read the store"
    );
}
