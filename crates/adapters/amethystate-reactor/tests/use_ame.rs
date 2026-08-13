use amethystate::ReactiveCell;
use amethystate_reactor::AmeCx;
use std::cell::Cell;
use std::rc::Rc;
use std::thread;
use windows_reactor::{ChannelDispatcher, RenderCx, UiRerenderGuard};

struct Host {
    cx: RenderCx,
    dispatcher: ChannelDispatcher,
    rerenders: Rc<Cell<u32>>,
    _guard: UiRerenderGuard,
}

fn host() -> Host {
    let dispatcher = ChannelDispatcher::new();
    let mut cx = RenderCx::for_test();
    cx.set_marshaller(Some(dispatcher.marshaller()));

    let rerenders = Rc::new(Cell::new(0));
    let counted = Rc::clone(&rerenders);
    let guard = UiRerenderGuard::install(
        cx.host_id(),
        Rc::new(move || counted.set(counted.get() + 1)),
    );

    Host {
        cx,
        dispatcher,
        rerenders,
        _guard: guard,
    }
}

#[test]
fn the_first_render_sees_the_current_value() {
    let cell = ReactiveCell::new(7u64);
    let mut host = host();

    host.cx.begin_render();
    assert_eq!(host.cx.use_ame(&cell), 7);
}

#[test]
fn a_change_reaches_the_next_render() {
    let cell = ReactiveCell::new(1u64);
    let mut host = host();

    host.cx.begin_render();
    assert_eq!(host.cx.use_ame(&cell), 1);
    host.cx.flush_effects();

    cell.set(2).unwrap();
    host.dispatcher.drain();

    host.cx.begin_render();
    assert_eq!(host.cx.use_ame(&cell), 2);
}

#[test]
fn nothing_lands_until_the_ui_thread_drains() {
    let cell = ReactiveCell::new(1u64);
    let mut host = host();

    host.cx.begin_render();
    host.cx.use_ame(&cell);
    host.cx.flush_effects();

    cell.set(2).unwrap();

    assert_eq!(host.rerenders.get(), 0, "the write is still in the queue");
    host.cx.begin_render();
    assert_eq!(host.cx.use_ame(&cell), 1);

    host.dispatcher.drain();
    assert_eq!(host.rerenders.get(), 1);

    host.cx.begin_render();
    assert_eq!(host.cx.use_ame(&cell), 2);
}

#[test]
fn a_write_from_another_thread_is_marshalled() {
    let cell = ReactiveCell::new(0u64);
    let mut host = host();

    host.cx.begin_render();
    host.cx.use_ame(&cell);
    host.cx.flush_effects();

    let writer = cell.clone();
    thread::spawn(move || writer.set(42).unwrap())
        .join()
        .unwrap();

    host.dispatcher.drain();

    host.cx.begin_render();
    assert_eq!(host.cx.use_ame(&cell), 42);
}

#[test]
fn a_change_between_the_first_render_and_the_effect_is_not_lost() {
    let cell = ReactiveCell::new(1u64);
    let mut host = host();

    host.cx.begin_render();
    assert_eq!(host.cx.use_ame(&cell), 1);

    cell.set(2).unwrap();
    host.cx.flush_effects();
    host.dispatcher.drain();

    host.cx.begin_render();
    assert_eq!(
        host.cx.use_ame(&cell),
        2,
        "the effect re-reads on subscribe, so the gap is covered"
    );
}

#[test]
fn an_unchanged_value_does_not_rerender() {
    let cell = ReactiveCell::new(5u64);
    let mut host = host();

    host.cx.begin_render();
    host.cx.use_ame(&cell);
    host.cx.flush_effects();

    cell.set(5).unwrap();
    assert_eq!(
        host.dispatcher.pending(),
        1,
        "the write was still delivered"
    );

    host.dispatcher.drain();
    assert_eq!(host.rerenders.get(), 0, "but it changed nothing");
}
