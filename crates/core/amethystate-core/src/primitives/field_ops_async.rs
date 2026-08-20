use crate::path::StorePath;
use crate::primitives::error::{FieldError, ReactiveFieldResult};
use crate::primitives::field_core::FieldValue;
use crate::{AmeBackendAsync, FieldCore};
use error_stack::{Report, ResultExt};
use uuid::Uuid;

pub async fn field_set_async<B, T>(
    backend: &B,
    core: &FieldCore<T>,
    path: StorePath,
    value: T,
    source: Option<Uuid>,
) -> ReactiveFieldResult<()>
where
    B: AmeBackendAsync,
    T: FieldValue,
{
    let change = core
        .run_interceptors(path.clone(), value, source)
        .map_err(|_| Report::new(FieldError::Intercepted))
        .attach_with(|| format!("field: {path}"))?;

    // The cache is left to the backend's subscription, as in the sync path.
    // Writing it here too made the field its own second writer: subscribers
    // heard about one write twice, and a write the backend refused still
    // showed up in get().
    backend
        .set_owned_with_source(path.clone(), &change.new_value, change.source)
        .await
        .change_context(FieldError::Storage)
        .attach_with(|| format!("field: {path}"))?;

    Ok(())
}
