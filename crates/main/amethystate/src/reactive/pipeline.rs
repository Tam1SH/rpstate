pub use amethystate_core::primitives::pipeline::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WritableMode;
    use crate::reactive::Field;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    fn field<T>(value: T) -> Field<T, WritableMode>
    where
        T: serde::de::DeserializeOwned + serde::Serialize + Clone + Send + Sync + 'static,
    {
        Field::new_volatile(Arc::from("pipeline.test"), value)
    }

    #[test]
    fn single_source_pipeline_maps_changes() {
        let source = field(1);
        let doubled = source.clone().pipe().map(|v| v * 2);

        assert_eq!(doubled.get(), 2);

        source.set(3).unwrap();
        assert_eq!(doubled.get(), 6);
    }

    #[test]
    fn filter_map_uses_default_initially_and_suppresses_none() {
        let source = field(1);
        let even = source
            .clone()
            .pipe()
            .filter_map(|v| (v % 2 == 0).then_some(v * 10));

        assert_eq!(even.get(), 0);

        source.set(2).unwrap();
        assert_eq!(even.get(), 20);

        source.set(3).unwrap();
        assert_eq!(even.get(), 20);
    }

    #[test]
    fn inspect_observes_initial_and_changed_values() {
        let source = field(5);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);

        let inspected = source.clone().pipe().inspect(move |v| {
            seen.lock().unwrap().push(*v);
        });

        source.set(8).unwrap();
        assert_eq!(inspected.get(), 8);
        assert_eq!(*calls.lock().unwrap(), vec![5, 8]);
    }

    #[test]
    fn dedupe_skips_repeated_values() {
        let source = field(1);
        let deduped = source.clone().pipe().dedupe();
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);

        let _sub = deduped.subscribe(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
        });

        source.set(1).unwrap();
        source.set(2).unwrap();
        source.set(2).unwrap();
        source.set(3).unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(deduped.get(), 3);
    }

    #[test]
    fn tuple_pipeline_combines_latest_values() {
        let host = field("localhost".to_string());
        let port = field(8080u16);
        let address = (host.clone(), port.clone())
            .pipe()
            .map(|(host, port)| format!("{host}:{port}"));

        assert_eq!(address.get(), "localhost:8080");

        port.set(9090).unwrap();
        assert_eq!(address.get(), "localhost:9090");

        host.set("127.0.0.1".to_string()).unwrap();
        assert_eq!(address.get(), "127.0.0.1:9090");
    }

    #[test]
    fn pipelines_compose_as_sources() {
        let port = field(8080u16);
        let display_port = port.clone().pipe().map(|p| format!(":{p}"));
        let host = field("localhost".to_string());
        let address = (host.clone(), display_port)
            .pipe()
            .map(|(host, port)| format!("{host}{port}"));

        assert_eq!(address.get(), "localhost:8080");

        port.set(9090).unwrap();
        assert_eq!(address.get(), "localhost:9090");
    }

    #[test]
    fn dropping_pipeline_releases_upstream_subscription() {
        let source = field(1);
        let calls = Arc::new(AtomicUsize::new(0));

        let sub = {
            let calls = Arc::clone(&calls);
            source.clone().pipe().map(|v| v * 2).subscribe(move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
            })
        };

        source.set(2).unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(sub);
    }

    #[test]
    fn complex_pipeline_graph_propagates_predictably() {
        let host = field("localhost".to_string());
        let port = field(8080u16);
        let enabled = field(false);

        let display_port = port
            .clone()
            .pipe()
            .filter_map(|p| (p >= 1024).then_some(format!(":{p}")))
            .dedupe();

        let address = (host.clone(), display_port.clone(), enabled.clone())
            .pipe()
            .filter_map(|(host, port, enabled)| enabled.then_some(format!("{host}{port}")))
            .dedupe();

        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&events);
        let _sub = address.subscribe(move |value| {
            seen.lock().unwrap().push(value.clone());
        });

        assert_eq!(display_port.get(), ":8080");
        assert_eq!(address.get(), "");

        port.set(80).unwrap();
        assert_eq!(display_port.get(), ":8080");
        assert!(events.lock().unwrap().is_empty());

        enabled.set(true).unwrap();
        assert_eq!(address.get(), "localhost:8080");
        assert_eq!(*events.lock().unwrap(), vec!["localhost:8080".to_string()]);

        port.set(9090).unwrap();
        assert_eq!(display_port.get(), ":9090");
        assert_eq!(address.get(), "localhost:9090");

        host.set("127.0.0.1".to_string()).unwrap();
        assert_eq!(address.get(), "127.0.0.1:9090");

        enabled.set(false).unwrap();
        assert_eq!(address.get(), "127.0.0.1:9090");
        assert_eq!(events.lock().unwrap().len(), 3);

        enabled.set(true).unwrap();
        assert_eq!(address.get(), "127.0.0.1:9090");
        assert_eq!(events.lock().unwrap().len(), 3);

        assert_eq!(
            *events.lock().unwrap(),
            vec![
                "localhost:8080".to_string(),
                "localhost:9090".to_string(),
                "127.0.0.1:9090".to_string(),
            ]
        );
    }

    #[test]
    fn reactive_scope_owns_and_clears_subscriptions() {
        let source = field(1);
        let doubled = source.clone().pipe().map(|v| v * 2);
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let mut scope = ReactiveScope::new();

        scope.watch(doubled.subscribe(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
        }));

        assert_eq!(scope.len(), 1);

        source.set(2).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        scope.clear();
        assert!(scope.is_empty());

        source.set(3).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
