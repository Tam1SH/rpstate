use crate::event::Event;
use amethystate_core::primitives::map_core::{ReactiveMapKey, ReactiveMapValue};
use amethystate_core::{AmeBackendAsync, AsyncSubscriptionBackend, SubscriptionHandle};
use amethystate_core::{FieldCore, MapChange, ReactiveMapCore};
use futures::StreamExt;
use futures::future::AbortHandle;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use uuid::Uuid;
pub(crate) type TauriResult<T> = std::result::Result<T, Error>;

mod core;
mod error;
mod event;
pub use error::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TauriBackend;

impl TauriBackend {
    pub fn new() -> Self {
        Self
    }
}

impl AmeBackendAsync for TauriBackend {
    type Error = String;
    type Raw = serde_json::Value;

    async fn get<T>(&self, path: &str) -> Result<Option<T>, Self::Error>
    where
        T: DeserializeOwned,
    {
        #[derive(Serialize)]
        struct GetArgs<'a> {
            key: &'a str,
        }

        let raw = core::invoke_result::<Option<serde_json::Value>, String>(
            "plugin:amethystate|amethystate_get",
            &GetArgs { key: path },
        )
        .await?;

        raw.map(serde_json::from_value)
            .transpose()
            .map_err(|e| e.to_string())
    }

    async fn set<T>(&self, path: &str, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize,
    {
        self.set_with_source(path, value, None).await
    }

    async fn set_with_source<T: Serialize>(
        &self,
        path: &str,
        value: &T,
        source: Option<Uuid>,
    ) -> Result<(), Self::Error> {
        #[derive(Serialize)]
        struct SetArgs<'a> {
            key: &'a str,
            value: serde_json::Value,
            source: Option<Uuid>,
        }

        let value = serde_json::to_value(value).map_err(|e| e.to_string())?;
        core::invoke_result::<(), String>(
            "plugin:amethystate|amethystate_set",
            &SetArgs {
                key: path,
                value,
                source,
            },
        )
        .await
    }

    async fn set_owned_with_source<T: Serialize>(
        &self,
        path: Arc<str>,
        value: &T,
        source: Option<Uuid>,
    ) -> Result<(), Self::Error> {
        self.set_with_source(&path, value, source).await
    }

    async fn delete(&self, path: &str) -> Result<(), Self::Error> {
        self.delete_with_source(path, None).await
    }

    async fn delete_with_source(
        &self,
        path: &str,
        source: Option<Uuid>,
    ) -> Result<(), Self::Error> {
        #[derive(Serialize)]
        struct DeleteArgs<'a> {
            key: &'a str,
            source: Option<Uuid>,
        }

        core::invoke_result::<(), String>(
            "plugin:amethystate|amethystate_delete",
            &DeleteArgs { key: path, source },
        )
        .await
    }

    async fn scan_prefix(&self, prefix: &str) -> Result<Vec<(String, Self::Raw)>, Self::Error> {
        #[derive(Serialize)]
        struct PrefixArgs<'a> {
            prefix: &'a str,
        }

        let raw: std::collections::HashMap<String, serde_json::Value> =
            core::invoke_result::<_, Self::Error>(
                "plugin:amethystate|amethystate_get_prefix",
                &PrefixArgs { prefix },
            )
            .await?;

        Ok(raw.into_iter().collect())
    }

    fn decode<T>(&self, raw: &Self::Raw) -> Result<T, Self::Error>
    where
        T: DeserializeOwned + Default,
    {
        serde_json::from_value(raw.clone()).map_err(|e| e.to_string())
    }
}

impl AsyncSubscriptionBackend for TauriBackend {
    fn subscribe_field<T>(&self, path: Arc<str>, core: FieldCore<T>) -> SubscriptionHandle
    where
        T: DeserializeOwned + Clone + Send + Sync + 'static,
    {
        let event_channel = format!("amethystate://{}", path.replace('.', ":"));
        let (abort_handle, abort_registration) = AbortHandle::new_pair();

        wasm_bindgen_futures::spawn_local(async move {
            #[derive(Serialize)]
            struct SubArgs<'a> {
                key: &'a str,
            }

            let _ = core::invoke_result::<(), String>(
                "plugin:amethystate|amethystate_subscribe",
                &SubArgs { key: &path },
            )
            .await;

            if let Ok(stream) = event::listen::<T>(&event_channel).await {
                let mut aborted_stream =
                    futures::stream::Abortable::new(stream, abort_registration);
                while let Some(Event { payload, .. }) = aborted_stream.next().await {
                    amethystate_core::field_apply_remote_value(&core, payload, None);
                }
            }
        });

        SubscriptionHandle::new(move || abort_handle.abort())
    }

    fn subscribe_map<K, V>(&self, path: Arc<str>, core: ReactiveMapCore<K, V>) -> SubscriptionHandle
    where
        K: ReactiveMapKey + for<'de> Deserialize<'de>,
        V: ReactiveMapValue,
    {
        let event_channel = format!("amethystate://{}", path.replace('.', ":"));
        let (abort_handle, abort_registration) = AbortHandle::new_pair();

        wasm_bindgen_futures::spawn_local(async move {
            #[derive(Serialize)]
            struct SubArgs<'a> {
                key: &'a str,
            }

            let _ = core::invoke_result::<(), Self::Error>(
                "plugin:amethystate|amethystate_subscribe",
                &SubArgs { key: &path },
            )
            .await;

            if let Ok(stream) = event::listen::<serde_json::Value>(&event_channel).await {
                let mut aborted_stream =
                    futures::stream::Abortable::new(stream, abort_registration);
                while let Some(Event { payload, .. }) = aborted_stream.next().await {
                    if let Ok(change) = serde_json::from_value::<MapChangeHelper<K, V>>(payload) {
                        let core_change = change.into_core();
                        amethystate_core::map_apply_remote_change(&core, &core_change);
                        core.notify(&core_change);
                    }
                }
            }
        });

        SubscriptionHandle::new(move || abort_handle.abort())
    }
}

#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum MapChangeHelper<K, V> {
    Insert {
        key: K,
        value: V,
        source: Option<Uuid>,
    },
    Update {
        key: K,
        #[serde(rename = "oldValue")]
        old_value: V,
        #[serde(rename = "newValue")]
        new_value: V,
        source: Option<Uuid>,
    },
    Remove {
        key: K,
        #[serde(rename = "oldValue")]
        old_value: V,
        source: Option<Uuid>,
    },
    Clear {
        source: Option<Uuid>,
    },
}

impl<K, V> MapChangeHelper<K, V> {
    fn into_core(self) -> MapChange<K, V> {
        match self {
            MapChangeHelper::Insert { key, value, source } => {
                MapChange::Insert { key, value, source }
            }
            MapChangeHelper::Update {
                key,
                old_value,
                new_value,
                source,
            } => MapChange::Update {
                key,
                old_value,
                new_value,
                source,
            },
            MapChangeHelper::Remove {
                key,
                old_value,
                source,
            } => MapChange::Remove {
                key,
                old_value,
                source,
            },
            MapChangeHelper::Clear { source } => MapChange::Clear { source },
        }
    }
}
