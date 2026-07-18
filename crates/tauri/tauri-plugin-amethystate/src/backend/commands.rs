use amethystate::store::Store;
use amethystate::store::SubscriptionId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Runtime, State};

pub struct PluginState {
    pub store: amethystate::DefaultStore,
    pub subscriptions: Mutex<HashMap<String, SubscriptionId>>,
}

#[tauri::command]
pub async fn amethystate_get(
    store: State<'_, PluginState>,
    key: String,
) -> Result<Option<serde_json::Value>, String> {
    store.store.get(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn amethystate_set(
    store: State<'_, PluginState>,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    store.store.set(&key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn amethystate_get_prefix(
    store: State<'_, PluginState>,
    prefix: String,
) -> Result<HashMap<String, serde_json::Value>, String> {
    let raw = amethystate::Store::scan_prefix(&store.store, &prefix).map_err(|e| e.to_string())?;

    let mut map = HashMap::new();
    for (path, bytes) in raw {
        if let Ok(val) = store.store.decode::<serde_json::Value>(&bytes) {
            map.insert(path, val);
        }
    }
    Ok(map)
}

#[tauri::command]
pub async fn amethystate_flush(
    store: State<'_, PluginState>,
    prefix: String,
) -> Result<(), String> {
    amethystate::Store::flush_prefix(&store.store, &prefix).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn amethystate_subscribe<R: Runtime>(
    store: State<'_, PluginState>,
    app: AppHandle<R>,
    key: String,
) -> Result<(), String> {
    let mut subs = store.subscriptions.lock().map_err(|e| e.to_string())?;
    if subs.contains_key(&key) {
        return Ok(());
    }

    let app_handle = app.clone();
    let key_clone = key.clone();
    let store_clone = store.store.clone();

    let sub_id = store.store.subscribe(
        amethystate::SubscriptionKind::Prefix(Arc::from(key.as_str())),
        Arc::new(move |event| {
            let event_name = format!("amethystate://{}", key_clone.replace('.', ":"));
            let store_c = store_clone.clone();

            let prefix_dot = format!("{}.", key_clone);
            if let Some(subkey) = event.path.strip_prefix(&prefix_dot) {
                let old_val = event
                    .old
                    .as_ref()
                    .and_then(|b| store_c.decode::<serde_json::Value>(b).ok());
                let new_val = event
                    .new
                    .as_ref()
                    .and_then(|b| store_c.decode::<serde_json::Value>(b).ok());

                let payload = match event.op {
                    amethystate::StoreOp::Set => {
                        if let Some(old) = old_val {
                            serde_json::json!({
                                "type": "Update",
                                "key": subkey,
                                "oldValue": old,
                                "newValue": new_val.unwrap_or(serde_json::Value::Null),
                            })
                        } else {
                            serde_json::json!({
                                "type": "Insert",
                                "key": subkey,
                                "value": new_val.unwrap_or(serde_json::Value::Null),
                            })
                        }
                    }
                    amethystate::StoreOp::Delete => serde_json::json!({
                        "type": "Remove",
                        "key": subkey,
                        "oldValue": old_val.unwrap_or(serde_json::Value::Null),
                    }),
                };
                let _ = app_handle.emit(&event_name, payload);
            } else if *event.path == *key_clone
                && let Some(new_bytes) = &event.new
                && let Ok(val) = store_c.decode::<serde_json::Value>(new_bytes)
            {
                let _ = app_handle.emit(&event_name, val);
            }
        }),
    );

    subs.insert(key, sub_id);
    Ok(())
}

#[tauri::command]
pub async fn amethystate_unsubscribe(
    state: State<'_, PluginState>,
    key: String,
) -> Result<(), String> {
    let mut subs = state.subscriptions.lock().map_err(|e| e.to_string())?;
    if let Some(sub_id) = subs.remove(&key) {
        state.store.unsubscribe(sub_id);
    }
    Ok(())
}
#[tauri::command]
pub async fn amethystate_delete(store: State<'_, PluginState>, key: String) -> Result<(), String> {
    store.store.delete(&key).map_err(|e| e.to_string())
}
