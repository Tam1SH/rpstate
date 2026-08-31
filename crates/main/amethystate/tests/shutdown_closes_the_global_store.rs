use amethystate::store::builder::StoreBuilder;
use amethystate::{IntoGlobalStore, global_store};
use amethystate_core::test_utils::TempPath;
use std::error::Error;

mod common;
use common::shape;

#[test]
fn a_write_after_shutdown_is_refused_and_what_came_before_it_is_on_disk()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("global_shutdown");
    let guard = StoreBuilder::new(path.path()).init_global();

    global_store().kv().set("port", &8080u16)?;

    amethystate::shutdown()?;

    let refused = global_store()
        .kv()
        .set("port", &9090u16)
        .expect_err("a closed global store took a write");
    insta::assert_snapshot!("write_after_shutdown", shape(&refused));

    assert!(amethystate::shutdown().is_ok());

    drop(guard);

    let reopened = StoreBuilder::new(path.path()).build()?;
    assert_eq!(reopened.kv().get::<u16>("port")?, Some(8080));

    Ok(())
}
