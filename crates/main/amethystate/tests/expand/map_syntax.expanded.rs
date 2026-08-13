use amethystate::ReactiveMap;
use amethystate_macros::{amethystate, AmeType};
use serde::{Deserialize, Serialize};
pub struct AlertThresholds {
    pub warning: u64,
    pub critical: u64,
}
#[automatically_derived]
impl ::core::fmt::Debug for AlertThresholds {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        ::core::fmt::Formatter::debug_struct_field2_finish(
            f,
            "AlertThresholds",
            "warning",
            &self.warning,
            "critical",
            &&self.critical,
        )
    }
}
#[automatically_derived]
impl ::core::clone::Clone for AlertThresholds {
    #[inline]
    fn clone(&self) -> AlertThresholds {
        AlertThresholds {
            warning: ::core::clone::Clone::clone(&self.warning),
            critical: ::core::clone::Clone::clone(&self.critical),
        }
    }
}
#[doc(hidden)]
#[allow(
    non_upper_case_globals,
    unused_attributes,
    unused_qualifications,
    clippy::absolute_paths,
)]
const _: () = {
    #[allow(unused_extern_crates, clippy::useless_attribute)]
    extern crate serde as _serde;
    #[automatically_derived]
    impl _serde::Serialize for AlertThresholds {
        fn serialize<__S>(
            &self,
            __serializer: __S,
        ) -> _serde::__private228::Result<__S::Ok, __S::Error>
        where
            __S: _serde::Serializer,
        {
            let mut __serde_state = _serde::Serializer::serialize_struct(
                __serializer,
                "AlertThresholds",
                false as usize + 1 + 1,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "warning",
                &self.warning,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "critical",
                &self.critical,
            )?;
            _serde::ser::SerializeStruct::end(__serde_state)
        }
    }
};
#[doc(hidden)]
#[allow(
    non_upper_case_globals,
    unused_attributes,
    unused_qualifications,
    clippy::absolute_paths,
)]
const _: () = {
    #[allow(unused_extern_crates, clippy::useless_attribute)]
    extern crate serde as _serde;
    #[automatically_derived]
    impl<'de> _serde::Deserialize<'de> for AlertThresholds {
        fn deserialize<__D>(
            __deserializer: __D,
        ) -> _serde::__private228::Result<Self, __D::Error>
        where
            __D: _serde::Deserializer<'de>,
        {
            #[allow(non_camel_case_types)]
            #[doc(hidden)]
            enum __Field {
                __field0,
                __field1,
                __ignore,
            }
            #[doc(hidden)]
            struct __FieldVisitor;
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __FieldVisitor {
                type Value = __Field;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "field identifier",
                    )
                }
                fn visit_u64<__E>(
                    self,
                    __value: u64,
                ) -> _serde::__private228::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        0u64 => _serde::__private228::Ok(__Field::__field0),
                        1u64 => _serde::__private228::Ok(__Field::__field1),
                        _ => _serde::__private228::Ok(__Field::__ignore),
                    }
                }
                fn visit_str<__E>(
                    self,
                    __value: &str,
                ) -> _serde::__private228::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        "warning" => _serde::__private228::Ok(__Field::__field0),
                        "critical" => _serde::__private228::Ok(__Field::__field1),
                        _ => _serde::__private228::Ok(__Field::__ignore),
                    }
                }
                fn visit_bytes<__E>(
                    self,
                    __value: &[u8],
                ) -> _serde::__private228::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        b"warning" => _serde::__private228::Ok(__Field::__field0),
                        b"critical" => _serde::__private228::Ok(__Field::__field1),
                        _ => _serde::__private228::Ok(__Field::__ignore),
                    }
                }
            }
            #[automatically_derived]
            impl<'de> _serde::Deserialize<'de> for __Field {
                #[inline]
                fn deserialize<__D>(
                    __deserializer: __D,
                ) -> _serde::__private228::Result<Self, __D::Error>
                where
                    __D: _serde::Deserializer<'de>,
                {
                    _serde::Deserializer::deserialize_identifier(
                        __deserializer,
                        __FieldVisitor,
                    )
                }
            }
            #[doc(hidden)]
            struct __Visitor<'de> {
                marker: _serde::__private228::PhantomData<AlertThresholds>,
                lifetime: _serde::__private228::PhantomData<&'de ()>,
            }
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __Visitor<'de> {
                type Value = AlertThresholds;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "struct AlertThresholds",
                    )
                }
                #[inline]
                fn visit_seq<__A>(
                    self,
                    mut __seq: __A,
                ) -> _serde::__private228::Result<Self::Value, __A::Error>
                where
                    __A: _serde::de::SeqAccess<'de>,
                {
                    let __field0 = match _serde::de::SeqAccess::next_element::<
                        u64,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    0usize,
                                    &"struct AlertThresholds with 2 elements",
                                ),
                            );
                        }
                    };
                    let __field1 = match _serde::de::SeqAccess::next_element::<
                        u64,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    1usize,
                                    &"struct AlertThresholds with 2 elements",
                                ),
                            );
                        }
                    };
                    _serde::__private228::Ok(AlertThresholds {
                        warning: __field0,
                        critical: __field1,
                    })
                }
                #[inline]
                fn visit_map<__A>(
                    self,
                    mut __map: __A,
                ) -> _serde::__private228::Result<Self::Value, __A::Error>
                where
                    __A: _serde::de::MapAccess<'de>,
                {
                    let mut __field0: _serde::__private228::Option<u64> = _serde::__private228::None;
                    let mut __field1: _serde::__private228::Option<u64> = _serde::__private228::None;
                    while let _serde::__private228::Some(__key) = _serde::de::MapAccess::next_key::<
                        __Field,
                    >(&mut __map)? {
                        match __key {
                            __Field::__field0 => {
                                if _serde::__private228::Option::is_some(&__field0) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field(
                                            "warning",
                                        ),
                                    );
                                }
                                __field0 = _serde::__private228::Some(
                                    _serde::de::MapAccess::next_value::<u64>(&mut __map)?,
                                );
                            }
                            __Field::__field1 => {
                                if _serde::__private228::Option::is_some(&__field1) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field(
                                            "critical",
                                        ),
                                    );
                                }
                                __field1 = _serde::__private228::Some(
                                    _serde::de::MapAccess::next_value::<u64>(&mut __map)?,
                                );
                            }
                            _ => {
                                let _ = _serde::de::MapAccess::next_value::<
                                    _serde::de::IgnoredAny,
                                >(&mut __map)?;
                            }
                        }
                    }
                    let __field0 = match __field0 {
                        _serde::__private228::Some(__field0) => __field0,
                        _serde::__private228::None => {
                            _serde::__private228::de::missing_field("warning")?
                        }
                    };
                    let __field1 = match __field1 {
                        _serde::__private228::Some(__field1) => __field1,
                        _serde::__private228::None => {
                            _serde::__private228::de::missing_field("critical")?
                        }
                    };
                    _serde::__private228::Ok(AlertThresholds {
                        warning: __field0,
                        critical: __field1,
                    })
                }
            }
            #[doc(hidden)]
            const FIELDS: &'static [&'static str] = &["warning", "critical"];
            _serde::Deserializer::deserialize_struct(
                __deserializer,
                "AlertThresholds",
                FIELDS,
                __Visitor {
                    marker: _serde::__private228::PhantomData::<AlertThresholds>,
                    lifetime: _serde::__private228::PhantomData,
                },
            )
        }
    }
};
#[automatically_derived]
impl ::core::default::Default for AlertThresholds {
    #[inline]
    fn default() -> AlertThresholds {
        AlertThresholds {
            warning: ::core::default::Default::default(),
            critical: ::core::default::Default::default(),
        }
    }
}
impl ::amethystate::migration::types::AmeType for AlertThresholds {
    const TYPE_HASH: u32 = 0u32
        ^ ::amethystate::migration::types::fnv1a("warning".as_bytes())
        ^ <u64 as ::amethystate::migration::types::AmeType>::TYPE_HASH
        ^ ::amethystate::migration::types::fnv1a("critical".as_bytes())
        ^ <u64 as ::amethystate::migration::types::AmeType>::TYPE_HASH;
    const TYPE_NAME: &'static str = "AlertThresholds";
}
pub struct MonitoringConfig {
    pub enabled: bool,
    pub thresholds: AlertThresholds,
}
#[automatically_derived]
impl ::core::fmt::Debug for MonitoringConfig {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        ::core::fmt::Formatter::debug_struct_field2_finish(
            f,
            "MonitoringConfig",
            "enabled",
            &self.enabled,
            "thresholds",
            &&self.thresholds,
        )
    }
}
#[automatically_derived]
impl ::core::clone::Clone for MonitoringConfig {
    #[inline]
    fn clone(&self) -> MonitoringConfig {
        MonitoringConfig {
            enabled: ::core::clone::Clone::clone(&self.enabled),
            thresholds: ::core::clone::Clone::clone(&self.thresholds),
        }
    }
}
#[doc(hidden)]
#[allow(
    non_upper_case_globals,
    unused_attributes,
    unused_qualifications,
    clippy::absolute_paths,
)]
const _: () = {
    #[allow(unused_extern_crates, clippy::useless_attribute)]
    extern crate serde as _serde;
    #[automatically_derived]
    impl _serde::Serialize for MonitoringConfig {
        fn serialize<__S>(
            &self,
            __serializer: __S,
        ) -> _serde::__private228::Result<__S::Ok, __S::Error>
        where
            __S: _serde::Serializer,
        {
            let mut __serde_state = _serde::Serializer::serialize_struct(
                __serializer,
                "MonitoringConfig",
                false as usize + 1 + 1,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "enabled",
                &self.enabled,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "thresholds",
                &self.thresholds,
            )?;
            _serde::ser::SerializeStruct::end(__serde_state)
        }
    }
};
#[doc(hidden)]
#[allow(
    non_upper_case_globals,
    unused_attributes,
    unused_qualifications,
    clippy::absolute_paths,
)]
const _: () = {
    #[allow(unused_extern_crates, clippy::useless_attribute)]
    extern crate serde as _serde;
    #[automatically_derived]
    impl<'de> _serde::Deserialize<'de> for MonitoringConfig {
        fn deserialize<__D>(
            __deserializer: __D,
        ) -> _serde::__private228::Result<Self, __D::Error>
        where
            __D: _serde::Deserializer<'de>,
        {
            #[allow(non_camel_case_types)]
            #[doc(hidden)]
            enum __Field {
                __field0,
                __field1,
                __ignore,
            }
            #[doc(hidden)]
            struct __FieldVisitor;
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __FieldVisitor {
                type Value = __Field;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "field identifier",
                    )
                }
                fn visit_u64<__E>(
                    self,
                    __value: u64,
                ) -> _serde::__private228::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        0u64 => _serde::__private228::Ok(__Field::__field0),
                        1u64 => _serde::__private228::Ok(__Field::__field1),
                        _ => _serde::__private228::Ok(__Field::__ignore),
                    }
                }
                fn visit_str<__E>(
                    self,
                    __value: &str,
                ) -> _serde::__private228::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        "enabled" => _serde::__private228::Ok(__Field::__field0),
                        "thresholds" => _serde::__private228::Ok(__Field::__field1),
                        _ => _serde::__private228::Ok(__Field::__ignore),
                    }
                }
                fn visit_bytes<__E>(
                    self,
                    __value: &[u8],
                ) -> _serde::__private228::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        b"enabled" => _serde::__private228::Ok(__Field::__field0),
                        b"thresholds" => _serde::__private228::Ok(__Field::__field1),
                        _ => _serde::__private228::Ok(__Field::__ignore),
                    }
                }
            }
            #[automatically_derived]
            impl<'de> _serde::Deserialize<'de> for __Field {
                #[inline]
                fn deserialize<__D>(
                    __deserializer: __D,
                ) -> _serde::__private228::Result<Self, __D::Error>
                where
                    __D: _serde::Deserializer<'de>,
                {
                    _serde::Deserializer::deserialize_identifier(
                        __deserializer,
                        __FieldVisitor,
                    )
                }
            }
            #[doc(hidden)]
            struct __Visitor<'de> {
                marker: _serde::__private228::PhantomData<MonitoringConfig>,
                lifetime: _serde::__private228::PhantomData<&'de ()>,
            }
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __Visitor<'de> {
                type Value = MonitoringConfig;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "struct MonitoringConfig",
                    )
                }
                #[inline]
                fn visit_seq<__A>(
                    self,
                    mut __seq: __A,
                ) -> _serde::__private228::Result<Self::Value, __A::Error>
                where
                    __A: _serde::de::SeqAccess<'de>,
                {
                    let __field0 = match _serde::de::SeqAccess::next_element::<
                        bool,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    0usize,
                                    &"struct MonitoringConfig with 2 elements",
                                ),
                            );
                        }
                    };
                    let __field1 = match _serde::de::SeqAccess::next_element::<
                        AlertThresholds,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    1usize,
                                    &"struct MonitoringConfig with 2 elements",
                                ),
                            );
                        }
                    };
                    _serde::__private228::Ok(MonitoringConfig {
                        enabled: __field0,
                        thresholds: __field1,
                    })
                }
                #[inline]
                fn visit_map<__A>(
                    self,
                    mut __map: __A,
                ) -> _serde::__private228::Result<Self::Value, __A::Error>
                where
                    __A: _serde::de::MapAccess<'de>,
                {
                    let mut __field0: _serde::__private228::Option<bool> = _serde::__private228::None;
                    let mut __field1: _serde::__private228::Option<AlertThresholds> = _serde::__private228::None;
                    while let _serde::__private228::Some(__key) = _serde::de::MapAccess::next_key::<
                        __Field,
                    >(&mut __map)? {
                        match __key {
                            __Field::__field0 => {
                                if _serde::__private228::Option::is_some(&__field0) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field(
                                            "enabled",
                                        ),
                                    );
                                }
                                __field0 = _serde::__private228::Some(
                                    _serde::de::MapAccess::next_value::<bool>(&mut __map)?,
                                );
                            }
                            __Field::__field1 => {
                                if _serde::__private228::Option::is_some(&__field1) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field(
                                            "thresholds",
                                        ),
                                    );
                                }
                                __field1 = _serde::__private228::Some(
                                    _serde::de::MapAccess::next_value::<
                                        AlertThresholds,
                                    >(&mut __map)?,
                                );
                            }
                            _ => {
                                let _ = _serde::de::MapAccess::next_value::<
                                    _serde::de::IgnoredAny,
                                >(&mut __map)?;
                            }
                        }
                    }
                    let __field0 = match __field0 {
                        _serde::__private228::Some(__field0) => __field0,
                        _serde::__private228::None => {
                            _serde::__private228::de::missing_field("enabled")?
                        }
                    };
                    let __field1 = match __field1 {
                        _serde::__private228::Some(__field1) => __field1,
                        _serde::__private228::None => {
                            _serde::__private228::de::missing_field("thresholds")?
                        }
                    };
                    _serde::__private228::Ok(MonitoringConfig {
                        enabled: __field0,
                        thresholds: __field1,
                    })
                }
            }
            #[doc(hidden)]
            const FIELDS: &'static [&'static str] = &["enabled", "thresholds"];
            _serde::Deserializer::deserialize_struct(
                __deserializer,
                "MonitoringConfig",
                FIELDS,
                __Visitor {
                    marker: _serde::__private228::PhantomData::<MonitoringConfig>,
                    lifetime: _serde::__private228::PhantomData,
                },
            )
        }
    }
};
#[automatically_derived]
impl ::core::default::Default for MonitoringConfig {
    #[inline]
    fn default() -> MonitoringConfig {
        MonitoringConfig {
            enabled: ::core::default::Default::default(),
            thresholds: ::core::default::Default::default(),
        }
    }
}
impl ::amethystate::migration::types::AmeType for MonitoringConfig {
    const TYPE_HASH: u32 = 0u32
        ^ ::amethystate::migration::types::fnv1a("enabled".as_bytes())
        ^ <bool as ::amethystate::migration::types::AmeType>::TYPE_HASH
        ^ ::amethystate::migration::types::fnv1a("thresholds".as_bytes())
        ^ <AlertThresholds as ::amethystate::migration::types::AmeType>::TYPE_HASH;
    const TYPE_NAME: &'static str = "MonitoringConfig";
}
pub struct DatabaseConfig<S: ::amethystate::Store = ::amethystate::DefaultStore> {
    __amethystate_instance_id: ::amethystate::uuid::Uuid,
    pub host: ::amethystate::Field<String, S, ::amethystate::WritableMode>,
}
#[automatically_derived]
impl<S: ::core::clone::Clone + ::amethystate::Store> ::core::clone::Clone
for DatabaseConfig<S> {
    #[inline]
    fn clone(&self) -> DatabaseConfig<S> {
        DatabaseConfig {
            __amethystate_instance_id: ::core::clone::Clone::clone(
                &self.__amethystate_instance_id,
            ),
            host: ::core::clone::Clone::clone(&self.host),
        }
    }
}
impl<S: ::amethystate::Store> ::std::fmt::Debug for DatabaseConfig<S>
where
    ::amethystate::Field<String, S, ::amethystate::WritableMode>: ::std::fmt::Debug,
{
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.debug_struct("DatabaseConfig").field("host", &self.host).finish()
    }
}
impl<S: ::amethystate::Store> DatabaseConfig<S> {
    pub fn new(store: &S, namespace: &str) -> ::amethystate::StorageResult<Self> {
        Self::new_with_id(store, namespace, ::amethystate::uuid::Uuid::new_v4())
    }
    pub fn new_with_id(
        store: &S,
        namespace: &str,
        instance_id: ::amethystate::uuid::Uuid,
    ) -> ::amethystate::StorageResult<Self> {
        use ::amethystate::Store;
        ::amethystate::observability::register_instance(
            instance_id,
            ::std::any::type_name::<Self>(),
        );
        let result = Self {
            __amethystate_instance_id: instance_id,
            host: ::amethystate::store::field_with_path(
                store,
                ::std::sync::Arc::from(
                    ::alloc::__export::must_use({
                        ::alloc::fmt::format(format_args!("{0}.{1}", namespace, "host"))
                    }),
                ),
                "localhost".to_string(),
                instance_id,
            )?,
        };
        store.mark_initialized(namespace)?;
        Ok(result)
    }
    #[doc(hidden)]
    pub fn __schema_field_host(&self) -> ::amethystate::ReadOnly<String> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    pub fn host(&self) -> ::amethystate::Field<String, S, ::amethystate::WritableMode> {
        self.host.clone()
    }
    pub fn fork(&self) -> Self {
        self.fork_with_id(::amethystate::uuid::Uuid::new_v4())
    }
    #[doc(hidden)]
    pub fn fork_with_id(&self, new_id: ::amethystate::uuid::Uuid) -> Self {
        Self {
            __amethystate_instance_id: new_id,
            host: self.host.fork_with_id(new_id),
        }
    }
    pub fn subscribe_all<F>(&self, callback: F) -> ::amethystate::ReactiveScope
    where
        F: Fn() + Send + Sync + 'static,
    {
        let cb = ::std::sync::Arc::new(callback);
        let mut scope = ::amethystate::ReactiveScope::new();
        {
            let cb_clone = cb.clone();
            scope.watch(self.host.subscribe(move |_| cb_clone()));
        }
        scope
    }
    pub fn subscribe_all_external<F>(&self, callback: F) -> ::amethystate::ReactiveScope
    where
        F: Fn() + Send + Sync + 'static,
    {
        let cb = ::std::sync::Arc::new(callback);
        let mut scope = ::amethystate::ReactiveScope::new();
        {
            let cb_clone = cb.clone();
            scope
                .watch(
                    self
                        .host
                        .subscription_with()
                        .external()
                        .register(move |_| cb_clone()),
                );
        }
        scope
    }
}
impl<S: ::amethystate::Store> ::amethystate::AmeStateNode<S> for DatabaseConfig<S> {
    fn new_node(store: &S, path: &str) -> ::amethystate::StorageResult<Self> {
        Self::new(store, path)
    }
    fn new_node_with_id(
        store: &S,
        path: &str,
        instance_id: ::amethystate::uuid::Uuid,
    ) -> ::amethystate::StorageResult<Self> {
        Self::new_with_id(store, path, instance_id)
    }
}
#[serde(crate = "::amethystate::serde")]
#[doc(hidden)]
#[allow(non_camel_case_types)]
pub struct DatabaseConfig_Data {
    pub host: String,
}
#[doc(hidden)]
#[allow(
    non_upper_case_globals,
    unused_attributes,
    unused_qualifications,
    clippy::absolute_paths,
)]
const _: () = {
    use ::amethystate::serde as _serde;
    #[automatically_derived]
    impl _serde::Serialize for DatabaseConfig_Data {
        fn serialize<__S>(
            &self,
            __serializer: __S,
        ) -> _serde::__private228::Result<__S::Ok, __S::Error>
        where
            __S: _serde::Serializer,
        {
            let mut __serde_state = _serde::Serializer::serialize_struct(
                __serializer,
                "DatabaseConfig_Data",
                false as usize + 1,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "host",
                &self.host,
            )?;
            _serde::ser::SerializeStruct::end(__serde_state)
        }
    }
};
#[doc(hidden)]
#[allow(
    non_upper_case_globals,
    unused_attributes,
    unused_qualifications,
    clippy::absolute_paths,
)]
const _: () = {
    use ::amethystate::serde as _serde;
    #[automatically_derived]
    impl<'de> _serde::Deserialize<'de> for DatabaseConfig_Data {
        fn deserialize<__D>(
            __deserializer: __D,
        ) -> _serde::__private228::Result<Self, __D::Error>
        where
            __D: _serde::Deserializer<'de>,
        {
            #[allow(non_camel_case_types)]
            #[doc(hidden)]
            enum __Field {
                __field0,
                __ignore,
            }
            #[doc(hidden)]
            struct __FieldVisitor;
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __FieldVisitor {
                type Value = __Field;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "field identifier",
                    )
                }
                fn visit_u64<__E>(
                    self,
                    __value: u64,
                ) -> _serde::__private228::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        0u64 => _serde::__private228::Ok(__Field::__field0),
                        _ => _serde::__private228::Ok(__Field::__ignore),
                    }
                }
                fn visit_str<__E>(
                    self,
                    __value: &str,
                ) -> _serde::__private228::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        "host" => _serde::__private228::Ok(__Field::__field0),
                        _ => _serde::__private228::Ok(__Field::__ignore),
                    }
                }
                fn visit_bytes<__E>(
                    self,
                    __value: &[u8],
                ) -> _serde::__private228::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        b"host" => _serde::__private228::Ok(__Field::__field0),
                        _ => _serde::__private228::Ok(__Field::__ignore),
                    }
                }
            }
            #[automatically_derived]
            impl<'de> _serde::Deserialize<'de> for __Field {
                #[inline]
                fn deserialize<__D>(
                    __deserializer: __D,
                ) -> _serde::__private228::Result<Self, __D::Error>
                where
                    __D: _serde::Deserializer<'de>,
                {
                    _serde::Deserializer::deserialize_identifier(
                        __deserializer,
                        __FieldVisitor,
                    )
                }
            }
            #[doc(hidden)]
            struct __Visitor<'de> {
                marker: _serde::__private228::PhantomData<DatabaseConfig_Data>,
                lifetime: _serde::__private228::PhantomData<&'de ()>,
            }
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __Visitor<'de> {
                type Value = DatabaseConfig_Data;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "struct DatabaseConfig_Data",
                    )
                }
                #[inline]
                fn visit_seq<__A>(
                    self,
                    mut __seq: __A,
                ) -> _serde::__private228::Result<Self::Value, __A::Error>
                where
                    __A: _serde::de::SeqAccess<'de>,
                {
                    let __field0 = match _serde::de::SeqAccess::next_element::<
                        String,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    0usize,
                                    &"struct DatabaseConfig_Data with 1 element",
                                ),
                            );
                        }
                    };
                    _serde::__private228::Ok(DatabaseConfig_Data {
                        host: __field0,
                    })
                }
                #[inline]
                fn visit_map<__A>(
                    self,
                    mut __map: __A,
                ) -> _serde::__private228::Result<Self::Value, __A::Error>
                where
                    __A: _serde::de::MapAccess<'de>,
                {
                    let mut __field0: _serde::__private228::Option<String> = _serde::__private228::None;
                    while let _serde::__private228::Some(__key) = _serde::de::MapAccess::next_key::<
                        __Field,
                    >(&mut __map)? {
                        match __key {
                            __Field::__field0 => {
                                if _serde::__private228::Option::is_some(&__field0) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field("host"),
                                    );
                                }
                                __field0 = _serde::__private228::Some(
                                    _serde::de::MapAccess::next_value::<String>(&mut __map)?,
                                );
                            }
                            _ => {
                                let _ = _serde::de::MapAccess::next_value::<
                                    _serde::de::IgnoredAny,
                                >(&mut __map)?;
                            }
                        }
                    }
                    let __field0 = match __field0 {
                        _serde::__private228::Some(__field0) => __field0,
                        _serde::__private228::None => {
                            _serde::__private228::de::missing_field("host")?
                        }
                    };
                    _serde::__private228::Ok(DatabaseConfig_Data {
                        host: __field0,
                    })
                }
            }
            #[doc(hidden)]
            const FIELDS: &'static [&'static str] = &["host"];
            _serde::Deserializer::deserialize_struct(
                __deserializer,
                "DatabaseConfig_Data",
                FIELDS,
                __Visitor {
                    marker: _serde::__private228::PhantomData::<DatabaseConfig_Data>,
                    lifetime: _serde::__private228::PhantomData,
                },
            )
        }
    }
};
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::default::Default for DatabaseConfig_Data {
    #[inline]
    fn default() -> DatabaseConfig_Data {
        DatabaseConfig_Data {
            host: ::core::default::Default::default(),
        }
    }
}
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::clone::Clone for DatabaseConfig_Data {
    #[inline]
    fn clone(&self) -> DatabaseConfig_Data {
        DatabaseConfig_Data {
            host: ::core::clone::Clone::clone(&self.host),
        }
    }
}
impl DatabaseConfig_Data {
    #[doc(hidden)]
    pub fn __amethystate_load_from<S: ::amethystate::Store>(
        store: &S,
        prefix: &str,
    ) -> ::amethystate::StorageResult<Self> {
        Ok(Self {
            host: <S as ::amethystate::Store>::get::<
                String,
            >(store, &::amethystate::join_path(prefix, "host"))?
                .unwrap_or_else(|| "localhost".to_string()),
        })
    }
    #[doc(hidden)]
    pub fn __amethystate_save_to<S: ::amethystate::Store>(
        &self,
        store: &S,
        prefix: &str,
    ) -> ::amethystate::StorageResult<()> {
        <S as ::amethystate::Store>::set(
            &store,
            &::amethystate::join_path(prefix, "host"),
            &self.host,
        )?;
        Ok(())
    }
}
impl ::amethystate::migration::types::AmeType for DatabaseConfig_Data {
    const TYPE_HASH: u32 = 0u32
        ^ ::amethystate::migration::types::fnv1a("host".as_bytes())
        ^ <String as ::amethystate::migration::types::AmeType>::TYPE_HASH;
    const TYPE_NAME: &'static str = "DatabaseConfig_Data";
}
impl ::amethystate::migration::fields::AmeStateFields for DatabaseConfig_Data {
    const FIELDS: &'static [::amethystate::migration::fields::FieldDescriptor] = &[
        ::amethystate::migration::fields::FieldDescriptor {
            name: "host",
            type_hash: <String as ::amethystate::migration::types::AmeType>::TYPE_HASH,
            type_name: "String",
        },
    ];
    const VERSION: u32 = 0u32;
    const SCHEMA_HASH: u32 = ::amethystate::migration::types::schema_hash(Self::FIELDS);
    const PARENT_PREFIX: &'static str = "";
    const MIGRATION_DEPS: &'static [&'static str] = &[];
    fn load_struct(
        ctx: &mut ::amethystate::MigrationContext,
    ) -> ::amethystate::StorageResult<Self> {
        Ok(Self {
            host: ctx.get::<String>("host")?.unwrap_or_else(|| "localhost".to_string()),
        })
    }
    fn save_struct(
        &self,
        ctx: &mut ::amethystate::MigrationContext,
    ) -> ::amethystate::StorageResult<()> {
        ctx.set("host", &self.host)?;
        Ok(())
    }
}
impl<S: ::amethystate::Store> ::amethystate::AmeState for DatabaseConfig<S> {
    type Data = DatabaseConfig_Data;
}
#[allow(non_upper_case_globals)]
const _: () = {
    static __INVENTORY: ::inventory::Node = ::inventory::Node {
        value: &{
            ::amethystate::observability::SchemaEntry {
                prefix: None,
                struct_name: "DatabaseConfig",
                version: 0u32,
                schema_hash: <DatabaseConfig_Data as ::amethystate::migration::types::AmeType>::TYPE_HASH,
                fields: <DatabaseConfig_Data as ::amethystate::migration::fields::AmeStateFields>::FIELDS,
            }
        },
        next: ::inventory::__private::UnsafeCell::new(
            ::inventory::__private::Option::None,
        ),
    };
    unsafe extern "C" fn __ctor() {
        unsafe { ::inventory::ErasedNode::submit(__INVENTORY.value, &__INVENTORY) }
    }
    #[used]
    #[link_section = ".CRT$XCU"]
    static __CTOR: unsafe extern "C" fn() = __ctor;
};
pub struct SystemSettings<S: ::amethystate::Store = ::amethystate::DefaultStore> {
    __amethystate_instance_id: ::amethystate::uuid::Uuid,
    pub db: ::std::sync::Arc<DatabaseConfig<S>>,
    pub monitoring: ::amethystate::Field<
        MonitoringConfig,
        S,
        ::amethystate::WritableMode,
    >,
    pub limits: ::amethystate::ReactiveMap<
        String,
        AlertThresholds,
        S,
        ::amethystate::WritableMode,
    >,
    pub presets: ::amethystate::Field<
        Vec<AlertThresholds>,
        S,
        ::amethystate::WritableMode,
    >,
}
#[automatically_derived]
impl<S: ::core::clone::Clone + ::amethystate::Store> ::core::clone::Clone
for SystemSettings<S> {
    #[inline]
    fn clone(&self) -> SystemSettings<S> {
        SystemSettings {
            __amethystate_instance_id: ::core::clone::Clone::clone(
                &self.__amethystate_instance_id,
            ),
            db: ::core::clone::Clone::clone(&self.db),
            monitoring: ::core::clone::Clone::clone(&self.monitoring),
            limits: ::core::clone::Clone::clone(&self.limits),
            presets: ::core::clone::Clone::clone(&self.presets),
        }
    }
}
impl<S: ::amethystate::Store> ::std::fmt::Debug for SystemSettings<S>
where
    ::std::sync::Arc<DatabaseConfig<S>>: ::std::fmt::Debug,
    ::amethystate::Field<
        MonitoringConfig,
        S,
        ::amethystate::WritableMode,
    >: ::std::fmt::Debug,
    ::amethystate::ReactiveMap<
        String,
        AlertThresholds,
        S,
        ::amethystate::WritableMode,
    >: ::std::fmt::Debug,
    ::amethystate::Field<
        Vec<AlertThresholds>,
        S,
        ::amethystate::WritableMode,
    >: ::std::fmt::Debug,
{
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.debug_struct("SystemSettings")
            .field("db", &self.db)
            .field("monitoring", &self.monitoring)
            .field("limits", &self.limits)
            .field("presets", &self.presets)
            .finish()
    }
}
impl<S: ::amethystate::Store> ::amethystate::StateScope for SystemSettings<S> {
    const PREFIX: &'static str = "sys";
}
impl<S: ::amethystate::Store> SystemSettings<S> {
    pub fn new_with(store: &S) -> ::amethystate::StorageResult<Self> {
        Self::new_with_id(store, ::amethystate::uuid::Uuid::new_v4())
    }
    pub fn new_with_id(
        store: &S,
        instance_id: ::amethystate::uuid::Uuid,
    ) -> ::amethystate::StorageResult<Self> {
        use ::amethystate::Store;
        ::amethystate::observability::register_instance(
            instance_id,
            ::std::any::type_name::<Self>(),
        );
        let result = Self {
            __amethystate_instance_id: instance_id,
            db: {
                let prefix = <Self as ::amethystate::StateScope>::PREFIX;
                let path = if prefix == "." {
                    "db".to_string()
                } else {
                    ::alloc::__export::must_use({
                        ::alloc::fmt::format(format_args!("{0}.{1}", prefix, "db"))
                    })
                };
                ::std::sync::Arc::new(
                    DatabaseConfig::<S>::new_with_id(store, &path, instance_id)?,
                )
            },
            monitoring: ::amethystate::store::field::<
                Self,
                MonitoringConfig,
                S,
            >(
                store,
                "monitoring",
                MonitoringConfig {
                    enabled: true,
                    thresholds: AlertThresholds {
                        warning: 50,
                        critical: 80,
                    },
                },
                instance_id,
            )?,
            limits: ::amethystate::store::reactive_map::<
                Self,
                String,
                AlertThresholds,
                S,
            >(
                store,
                "limits",
                {
                    let mut __map = ::std::collections::HashMap::default();
                    __map
                        .insert(
                            ::std::convert::Into::into("cpu"),
                            AlertThresholds {
                                warning: 70,
                                critical: 90,
                            },
                        );
                    __map
                        .insert(
                            ::std::convert::Into::into("mem"),
                            AlertThresholds {
                                warning: 80,
                                critical: 95,
                            },
                        );
                    __map
                },
                instance_id,
            )?,
            presets: ::amethystate::store::field::<
                Self,
                Vec<AlertThresholds>,
                S,
            >(
                store,
                "presets",
                <[_]>::into_vec(
                    ::alloc::boxed::box_new([
                        AlertThresholds {
                            warning: 10,
                            critical: 20,
                        },
                        AlertThresholds {
                            warning: 30,
                            critical: 40,
                        },
                    ]),
                ),
                instance_id,
            )?,
        };
        store.mark_initialized(<Self as ::amethystate::StateScope>::PREFIX)?;
        Ok(result)
    }
    #[doc(hidden)]
    pub fn __schema_field_db(&self) -> ::amethystate::ReadOnly<DatabaseConfig> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    #[doc(hidden)]
    pub fn __schema_field_monitoring(
        &self,
    ) -> ::amethystate::ReadOnly<MonitoringConfig> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    #[doc(hidden)]
    pub fn __schema_field_limits(
        &self,
    ) -> ::amethystate::ReadOnly<ReactiveMap<String, AlertThresholds>> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    #[doc(hidden)]
    pub fn __schema_field_presets(
        &self,
    ) -> ::amethystate::ReadOnly<Vec<AlertThresholds>> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    pub fn db(&self) -> ::std::sync::Arc<DatabaseConfig<S>> {
        self.db.clone()
    }
    pub fn monitoring(
        &self,
    ) -> ::amethystate::Field<MonitoringConfig, S, ::amethystate::WritableMode> {
        self.monitoring.clone()
    }
    pub fn limits(
        &self,
    ) -> ::amethystate::ReactiveMap<
        String,
        AlertThresholds,
        S,
        ::amethystate::WritableMode,
    > {
        self.limits.clone()
    }
    pub fn presets(
        &self,
    ) -> ::amethystate::Field<Vec<AlertThresholds>, S, ::amethystate::WritableMode> {
        self.presets.clone()
    }
    pub fn fork(&self) -> Self {
        self.fork_with_id(::amethystate::uuid::Uuid::new_v4())
    }
    #[doc(hidden)]
    pub fn fork_with_id(&self, new_id: ::amethystate::uuid::Uuid) -> Self {
        Self {
            __amethystate_instance_id: new_id,
            db: ::std::sync::Arc::new(self.db.fork_with_id(new_id)),
            monitoring: self.monitoring.fork_with_id(new_id),
            limits: self.limits.fork_with_id(new_id),
            presets: self.presets.fork_with_id(new_id),
        }
    }
    pub fn subscribe_all<F>(&self, callback: F) -> ::amethystate::ReactiveScope
    where
        F: Fn() + Send + Sync + 'static,
    {
        let cb = ::std::sync::Arc::new(callback);
        let mut scope = ::amethystate::ReactiveScope::new();
        {
            let cb_clone = cb.clone();
            scope.watch_scope(self.db.subscribe_all(move || cb_clone()));
        }
        {
            let cb_clone = cb.clone();
            scope.watch(self.monitoring.subscribe(move |_| cb_clone()));
        }
        {
            let cb_clone = cb.clone();
            scope.watch(self.limits.subscribe_any(move |_| cb_clone()));
        }
        {
            let cb_clone = cb.clone();
            scope.watch(self.presets.subscribe(move |_| cb_clone()));
        }
        scope
    }
    pub fn subscribe_all_external<F>(&self, callback: F) -> ::amethystate::ReactiveScope
    where
        F: Fn() + Send + Sync + 'static,
    {
        let cb = ::std::sync::Arc::new(callback);
        let mut scope = ::amethystate::ReactiveScope::new();
        {
            let cb_clone = cb.clone();
            scope.watch_scope(self.db.subscribe_all_external(move || cb_clone()));
        }
        {
            let cb_clone = cb.clone();
            scope
                .watch(
                    self
                        .monitoring
                        .subscription_with()
                        .external()
                        .register(move |_| cb_clone()),
                );
        }
        {
            let cb_clone = cb.clone();
            scope
                .watch(
                    self
                        .limits
                        .subscription_with()
                        .external()
                        .register(move |_| cb_clone()),
                );
        }
        {
            let cb_clone = cb.clone();
            scope
                .watch(
                    self
                        .presets
                        .subscription_with()
                        .external()
                        .register(move |_| cb_clone()),
                );
        }
        scope
    }
}
impl SystemSettings<::amethystate::DefaultStore> {
    pub fn new() -> ::amethystate::StorageResult<Self> {
        let store = ::amethystate::global_store();
        Self::new_with(&store)
    }
}
impl<S: ::amethystate::Store> ::amethystate::AmeStateNode<S> for SystemSettings<S> {
    fn new_node(store: &S, _path: &str) -> ::amethystate::StorageResult<Self> {
        Self::new_with(store)
    }
    fn new_node_with_id(
        store: &S,
        _path: &str,
        instance_id: ::amethystate::uuid::Uuid,
    ) -> ::amethystate::StorageResult<Self> {
        Self::new_with_id(store, instance_id)
    }
}
#[serde(crate = "::amethystate::serde")]
#[doc(hidden)]
#[allow(non_camel_case_types)]
pub struct SystemSettings_Data {
    pub db: <DatabaseConfig<
        ::amethystate::DefaultStore,
    > as ::amethystate::AmeState>::Data,
    pub limits: ::std::collections::HashMap<String, AlertThresholds>,
    pub monitoring: MonitoringConfig,
    pub presets: Vec<AlertThresholds>,
}
#[doc(hidden)]
#[allow(
    non_upper_case_globals,
    unused_attributes,
    unused_qualifications,
    clippy::absolute_paths,
)]
const _: () = {
    use ::amethystate::serde as _serde;
    #[automatically_derived]
    impl _serde::Serialize for SystemSettings_Data {
        fn serialize<__S>(
            &self,
            __serializer: __S,
        ) -> _serde::__private228::Result<__S::Ok, __S::Error>
        where
            __S: _serde::Serializer,
        {
            let mut __serde_state = _serde::Serializer::serialize_struct(
                __serializer,
                "SystemSettings_Data",
                false as usize + 1 + 1 + 1 + 1,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "db",
                &self.db,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "limits",
                &self.limits,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "monitoring",
                &self.monitoring,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "presets",
                &self.presets,
            )?;
            _serde::ser::SerializeStruct::end(__serde_state)
        }
    }
};
#[doc(hidden)]
#[allow(
    non_upper_case_globals,
    unused_attributes,
    unused_qualifications,
    clippy::absolute_paths,
)]
const _: () = {
    use ::amethystate::serde as _serde;
    #[automatically_derived]
    impl<'de> _serde::Deserialize<'de> for SystemSettings_Data {
        fn deserialize<__D>(
            __deserializer: __D,
        ) -> _serde::__private228::Result<Self, __D::Error>
        where
            __D: _serde::Deserializer<'de>,
        {
            #[allow(non_camel_case_types)]
            #[doc(hidden)]
            enum __Field {
                __field0,
                __field1,
                __field2,
                __field3,
                __ignore,
            }
            #[doc(hidden)]
            struct __FieldVisitor;
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __FieldVisitor {
                type Value = __Field;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "field identifier",
                    )
                }
                fn visit_u64<__E>(
                    self,
                    __value: u64,
                ) -> _serde::__private228::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        0u64 => _serde::__private228::Ok(__Field::__field0),
                        1u64 => _serde::__private228::Ok(__Field::__field1),
                        2u64 => _serde::__private228::Ok(__Field::__field2),
                        3u64 => _serde::__private228::Ok(__Field::__field3),
                        _ => _serde::__private228::Ok(__Field::__ignore),
                    }
                }
                fn visit_str<__E>(
                    self,
                    __value: &str,
                ) -> _serde::__private228::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        "db" => _serde::__private228::Ok(__Field::__field0),
                        "limits" => _serde::__private228::Ok(__Field::__field1),
                        "monitoring" => _serde::__private228::Ok(__Field::__field2),
                        "presets" => _serde::__private228::Ok(__Field::__field3),
                        _ => _serde::__private228::Ok(__Field::__ignore),
                    }
                }
                fn visit_bytes<__E>(
                    self,
                    __value: &[u8],
                ) -> _serde::__private228::Result<Self::Value, __E>
                where
                    __E: _serde::de::Error,
                {
                    match __value {
                        b"db" => _serde::__private228::Ok(__Field::__field0),
                        b"limits" => _serde::__private228::Ok(__Field::__field1),
                        b"monitoring" => _serde::__private228::Ok(__Field::__field2),
                        b"presets" => _serde::__private228::Ok(__Field::__field3),
                        _ => _serde::__private228::Ok(__Field::__ignore),
                    }
                }
            }
            #[automatically_derived]
            impl<'de> _serde::Deserialize<'de> for __Field {
                #[inline]
                fn deserialize<__D>(
                    __deserializer: __D,
                ) -> _serde::__private228::Result<Self, __D::Error>
                where
                    __D: _serde::Deserializer<'de>,
                {
                    _serde::Deserializer::deserialize_identifier(
                        __deserializer,
                        __FieldVisitor,
                    )
                }
            }
            #[doc(hidden)]
            struct __Visitor<'de> {
                marker: _serde::__private228::PhantomData<SystemSettings_Data>,
                lifetime: _serde::__private228::PhantomData<&'de ()>,
            }
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __Visitor<'de> {
                type Value = SystemSettings_Data;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "struct SystemSettings_Data",
                    )
                }
                #[inline]
                fn visit_seq<__A>(
                    self,
                    mut __seq: __A,
                ) -> _serde::__private228::Result<Self::Value, __A::Error>
                where
                    __A: _serde::de::SeqAccess<'de>,
                {
                    let __field0 = match _serde::de::SeqAccess::next_element::<
                        <DatabaseConfig<
                            ::amethystate::DefaultStore,
                        > as ::amethystate::AmeState>::Data,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    0usize,
                                    &"struct SystemSettings_Data with 4 elements",
                                ),
                            );
                        }
                    };
                    let __field1 = match _serde::de::SeqAccess::next_element::<
                        ::std::collections::HashMap<String, AlertThresholds>,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    1usize,
                                    &"struct SystemSettings_Data with 4 elements",
                                ),
                            );
                        }
                    };
                    let __field2 = match _serde::de::SeqAccess::next_element::<
                        MonitoringConfig,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    2usize,
                                    &"struct SystemSettings_Data with 4 elements",
                                ),
                            );
                        }
                    };
                    let __field3 = match _serde::de::SeqAccess::next_element::<
                        Vec<AlertThresholds>,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    3usize,
                                    &"struct SystemSettings_Data with 4 elements",
                                ),
                            );
                        }
                    };
                    _serde::__private228::Ok(SystemSettings_Data {
                        db: __field0,
                        limits: __field1,
                        monitoring: __field2,
                        presets: __field3,
                    })
                }
                #[inline]
                fn visit_map<__A>(
                    self,
                    mut __map: __A,
                ) -> _serde::__private228::Result<Self::Value, __A::Error>
                where
                    __A: _serde::de::MapAccess<'de>,
                {
                    let mut __field0: _serde::__private228::Option<
                        <DatabaseConfig<
                            ::amethystate::DefaultStore,
                        > as ::amethystate::AmeState>::Data,
                    > = _serde::__private228::None;
                    let mut __field1: _serde::__private228::Option<
                        ::std::collections::HashMap<String, AlertThresholds>,
                    > = _serde::__private228::None;
                    let mut __field2: _serde::__private228::Option<MonitoringConfig> = _serde::__private228::None;
                    let mut __field3: _serde::__private228::Option<
                        Vec<AlertThresholds>,
                    > = _serde::__private228::None;
                    while let _serde::__private228::Some(__key) = _serde::de::MapAccess::next_key::<
                        __Field,
                    >(&mut __map)? {
                        match __key {
                            __Field::__field0 => {
                                if _serde::__private228::Option::is_some(&__field0) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field("db"),
                                    );
                                }
                                __field0 = _serde::__private228::Some(
                                    _serde::de::MapAccess::next_value::<
                                        <DatabaseConfig<
                                            ::amethystate::DefaultStore,
                                        > as ::amethystate::AmeState>::Data,
                                    >(&mut __map)?,
                                );
                            }
                            __Field::__field1 => {
                                if _serde::__private228::Option::is_some(&__field1) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field("limits"),
                                    );
                                }
                                __field1 = _serde::__private228::Some(
                                    _serde::de::MapAccess::next_value::<
                                        ::std::collections::HashMap<String, AlertThresholds>,
                                    >(&mut __map)?,
                                );
                            }
                            __Field::__field2 => {
                                if _serde::__private228::Option::is_some(&__field2) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field(
                                            "monitoring",
                                        ),
                                    );
                                }
                                __field2 = _serde::__private228::Some(
                                    _serde::de::MapAccess::next_value::<
                                        MonitoringConfig,
                                    >(&mut __map)?,
                                );
                            }
                            __Field::__field3 => {
                                if _serde::__private228::Option::is_some(&__field3) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field(
                                            "presets",
                                        ),
                                    );
                                }
                                __field3 = _serde::__private228::Some(
                                    _serde::de::MapAccess::next_value::<
                                        Vec<AlertThresholds>,
                                    >(&mut __map)?,
                                );
                            }
                            _ => {
                                let _ = _serde::de::MapAccess::next_value::<
                                    _serde::de::IgnoredAny,
                                >(&mut __map)?;
                            }
                        }
                    }
                    let __field0 = match __field0 {
                        _serde::__private228::Some(__field0) => __field0,
                        _serde::__private228::None => {
                            _serde::__private228::de::missing_field("db")?
                        }
                    };
                    let __field1 = match __field1 {
                        _serde::__private228::Some(__field1) => __field1,
                        _serde::__private228::None => {
                            _serde::__private228::de::missing_field("limits")?
                        }
                    };
                    let __field2 = match __field2 {
                        _serde::__private228::Some(__field2) => __field2,
                        _serde::__private228::None => {
                            _serde::__private228::de::missing_field("monitoring")?
                        }
                    };
                    let __field3 = match __field3 {
                        _serde::__private228::Some(__field3) => __field3,
                        _serde::__private228::None => {
                            _serde::__private228::de::missing_field("presets")?
                        }
                    };
                    _serde::__private228::Ok(SystemSettings_Data {
                        db: __field0,
                        limits: __field1,
                        monitoring: __field2,
                        presets: __field3,
                    })
                }
            }
            #[doc(hidden)]
            const FIELDS: &'static [&'static str] = &[
                "db",
                "limits",
                "monitoring",
                "presets",
            ];
            _serde::Deserializer::deserialize_struct(
                __deserializer,
                "SystemSettings_Data",
                FIELDS,
                __Visitor {
                    marker: _serde::__private228::PhantomData::<SystemSettings_Data>,
                    lifetime: _serde::__private228::PhantomData,
                },
            )
        }
    }
};
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::default::Default for SystemSettings_Data {
    #[inline]
    fn default() -> SystemSettings_Data {
        SystemSettings_Data {
            db: ::core::default::Default::default(),
            limits: ::core::default::Default::default(),
            monitoring: ::core::default::Default::default(),
            presets: ::core::default::Default::default(),
        }
    }
}
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::clone::Clone for SystemSettings_Data {
    #[inline]
    fn clone(&self) -> SystemSettings_Data {
        SystemSettings_Data {
            db: ::core::clone::Clone::clone(&self.db),
            limits: ::core::clone::Clone::clone(&self.limits),
            monitoring: ::core::clone::Clone::clone(&self.monitoring),
            presets: ::core::clone::Clone::clone(&self.presets),
        }
    }
}
impl SystemSettings_Data {}
impl ::amethystate::migration::types::AmeType for SystemSettings_Data {
    const TYPE_HASH: u32 = 0u32 ^ ::amethystate::migration::types::fnv1a("db".as_bytes())
        ^ <<DatabaseConfig<
            ::amethystate::DefaultStore,
        > as ::amethystate::AmeState>::Data as ::amethystate::migration::types::AmeType>::TYPE_HASH
        ^ ::amethystate::migration::types::fnv1a("limits".as_bytes())
        ^ <::std::collections::HashMap<
            String,
            AlertThresholds,
        > as ::amethystate::migration::types::AmeType>::TYPE_HASH
        ^ ::amethystate::migration::types::fnv1a("monitoring".as_bytes())
        ^ <MonitoringConfig as ::amethystate::migration::types::AmeType>::TYPE_HASH
        ^ ::amethystate::migration::types::fnv1a("presets".as_bytes())
        ^ <Vec<AlertThresholds> as ::amethystate::migration::types::AmeType>::TYPE_HASH;
    const TYPE_NAME: &'static str = "SystemSettings_Data";
}
impl ::amethystate::migration::fields::AmeStateFields for SystemSettings_Data {
    const FIELDS: &'static [::amethystate::migration::fields::FieldDescriptor] = &[
        ::amethystate::migration::fields::FieldDescriptor {
            name: "db",
            type_hash: 0xDEADBEEF
                ^ <<DatabaseConfig<
                    ::amethystate::DefaultStore,
                > as ::amethystate::AmeState>::Data as ::amethystate::migration::types::AmeType>::TYPE_HASH,
            type_name: "DatabaseConfig",
        },
        ::amethystate::migration::fields::FieldDescriptor {
            name: "limits",
            type_hash: <::std::collections::HashMap<
                String,
                AlertThresholds,
            > as ::amethystate::migration::types::AmeType>::TYPE_HASH,
            type_name: "ReactiveMap<String,AlertThresholds>",
        },
        ::amethystate::migration::fields::FieldDescriptor {
            name: "monitoring",
            type_hash: <MonitoringConfig as ::amethystate::migration::types::AmeType>::TYPE_HASH,
            type_name: "MonitoringConfig",
        },
        ::amethystate::migration::fields::FieldDescriptor {
            name: "presets",
            type_hash: <Vec<
                AlertThresholds,
            > as ::amethystate::migration::types::AmeType>::TYPE_HASH,
            type_name: "Vec<AlertThresholds>",
        },
    ];
    const VERSION: u32 = 0u32;
    const SCHEMA_HASH: u32 = ::amethystate::migration::types::schema_hash(Self::FIELDS);
    const PARENT_PREFIX: &'static str = "sys";
    const MIGRATION_DEPS: &'static [&'static str] = &[];
    fn load_struct(
        ctx: &mut ::amethystate::MigrationContext,
    ) -> ::amethystate::StorageResult<Self> {
        Ok(Self {
            db: {
                let mut sub_ctx = ctx.scoped("db");
                <<DatabaseConfig as ::amethystate::AmeState>::Data as ::amethystate::migration::fields::AmeStateFields>::load_struct(
                    &mut sub_ctx,
                )?
            },
            limits: ctx.scan_map::<String, AlertThresholds>("limits")?,
            monitoring: ctx
                .get::<MonitoringConfig>("monitoring")?
                .unwrap_or_else(|| MonitoringConfig {
                    enabled: true,
                    thresholds: AlertThresholds {
                        warning: 50,
                        critical: 80,
                    },
                }),
            presets: ctx
                .get::<Vec<AlertThresholds>>("presets")?
                .unwrap_or_else(|| <[_]>::into_vec(
                    ::alloc::boxed::box_new([
                        AlertThresholds {
                            warning: 10,
                            critical: 20,
                        },
                        AlertThresholds {
                            warning: 30,
                            critical: 40,
                        },
                    ]),
                )),
        })
    }
    fn save_struct(
        &self,
        ctx: &mut ::amethystate::MigrationContext,
    ) -> ::amethystate::StorageResult<()> {
        {
            let mut sub_ctx = ctx.scoped("db");
            self.db.save_struct(&mut sub_ctx)?;
        }
        for (k, v) in &self.limits {
            let full_key = ::alloc::__export::must_use({
                ::alloc::fmt::format(format_args!("{0}.{1}", "limits", k))
            });
            ctx.set(&full_key, v)?;
        }
        ctx.set("monitoring", &self.monitoring)?;
        ctx.set("presets", &self.presets)?;
        Ok(())
    }
}
impl<S: ::amethystate::Store> ::amethystate::AmeState for SystemSettings<S> {
    type Data = SystemSettings_Data;
}
#[allow(non_upper_case_globals)]
const _: () = {
    static __INVENTORY: ::inventory::Node = ::inventory::Node {
        value: &{
            ::amethystate::observability::SchemaEntry {
                prefix: Some("sys"),
                struct_name: "SystemSettings",
                version: 0u32,
                schema_hash: <SystemSettings_Data as ::amethystate::migration::types::AmeType>::TYPE_HASH,
                fields: <SystemSettings_Data as ::amethystate::migration::fields::AmeStateFields>::FIELDS,
            }
        },
        next: ::inventory::__private::UnsafeCell::new(
            ::inventory::__private::Option::None,
        ),
    };
    unsafe extern "C" fn __ctor() {
        unsafe { ::inventory::ErasedNode::submit(__INVENTORY.value, &__INVENTORY) }
    }
    #[used]
    #[link_section = ".CRT$XCU"]
    static __CTOR: unsafe extern "C" fn() = __ctor;
};
impl<S: ::amethystate::Store> ::amethystate::AmeStateSlice<S> for SystemSettings<S> {
    fn load_slice(store: &S) -> ::amethystate::StorageResult<Self> {
        Self::new_with(store)
    }
    fn subscribe_all<F>(&self, callback: F) -> ::amethystate::ReactiveScope
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.subscribe_all(callback)
    }
    fn subscribe_all_external<F>(&self, callback: F) -> ::amethystate::ReactiveScope
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.subscribe_all_external(callback)
    }
}
fn main() {}
