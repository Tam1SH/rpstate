use crate::primitives::error::{FieldError, ReactiveFieldResult};
use crate::primitives::field_core::FieldValue;
use crate::{AmeBackendAsync, FieldCore};
use std::sync::Arc;
use uuid::Uuid;

pub async fn field_set_async<B, T>(
    backend: &B,
    core: &FieldCore<T>,
    path: Arc<str>,
    value: T,
    source: Option<Uuid>,
) -> ReactiveFieldResult<(), B::Error>
where
    B: AmeBackendAsync,
    T: FieldValue,
{
    let change = core
        .run_interceptors(path.clone(), value, source)
        .map_err(|_| FieldError::Intercepted)?;

    backend
        .set_owned_with_source(path, &change.new_value, change.source)
        .await?;

    core.signal.set(change.new_value.clone(), change.source);

    Ok(())
}
