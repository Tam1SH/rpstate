use amethystate_macros::amethystate;
pub struct ConnectionPool {
    __amethystate_instance_id: ::std::sync::Arc<
        ::amethystate::observability::InstanceGuard,
    >,
    pub max_connections: ::amethystate::Field<u32, ::amethystate::WritableMode>,
    pub timeout_secs: ::amethystate::Field<u32, ::amethystate::WritableMode>,
}
#[automatically_derived]
impl ::core::clone::Clone for ConnectionPool {
    #[inline]
    fn clone(&self) -> ConnectionPool {
        ConnectionPool {
            __amethystate_instance_id: ::core::clone::Clone::clone(
                &self.__amethystate_instance_id,
            ),
            max_connections: ::core::clone::Clone::clone(&self.max_connections),
            timeout_secs: ::core::clone::Clone::clone(&self.timeout_secs),
        }
    }
}
impl ::std::fmt::Debug for ConnectionPool {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        struct __AmeOpaque;
        impl ::std::fmt::Debug for __AmeOpaque {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str("<opaque>")
            }
        }
        struct __AmeW<'a, T>(&'a T);
        trait __AmeViaDebug {
            fn __ame(&self) -> &dyn ::std::fmt::Debug;
        }
        impl<'a, T: ::std::fmt::Debug> __AmeViaDebug for __AmeW<'a, T> {
            fn __ame(&self) -> &dyn ::std::fmt::Debug {
                self.0
            }
        }
        trait __AmeViaFallback {
            fn __ame(&self) -> &dyn ::std::fmt::Debug;
        }
        impl<'a, T> __AmeViaFallback for &__AmeW<'a, T> {
            fn __ame(&self) -> &dyn ::std::fmt::Debug {
                &__AmeOpaque
            }
        }
        f.debug_struct("ConnectionPool")
            .field("max_connections", (&__AmeW(&self.max_connections)).__ame())
            .field("timeout_secs", (&__AmeW(&self.timeout_secs)).__ame())
            .finish()
    }
}
impl ConnectionPool {
    pub fn new(
        store: &::amethystate::Store,
        namespace: impl ::amethystate::store::IntoStorePath,
    ) -> ::amethystate::StorageResult<Self> {
        Self::new_with_id(store, namespace, ::amethystate::uuid::Uuid::new_v4())
    }
    pub fn new_with_id(
        store: &::amethystate::Store,
        namespace: impl ::amethystate::store::IntoStorePath,
        instance_id: ::amethystate::uuid::Uuid,
    ) -> ::amethystate::StorageResult<Self> {
        use ::amethystate::{StoreBackend, StoreExt};
        let namespace = ::amethystate::store::to_path(namespace)?;
        let __amethystate_guard = ::amethystate::observability::InstanceGuard::new(
            instance_id,
            ::std::any::type_name::<Self>(),
        );
        let result = Self {
            __amethystate_instance_id: __amethystate_guard,
            max_connections: ::amethystate::store::field_with_path(
                store,
                namespace
                    .join(
                        &const {
                            ::amethystate::store::StorePath::from_static(
                                &["max_connections"],
                                "max_connections",
                            )
                        },
                    ),
                10,
                instance_id,
            )?,
            timeout_secs: ::amethystate::store::field_with_path(
                store,
                namespace
                    .join(
                        &const {
                            ::amethystate::store::StorePath::from_static(
                                &["timeout_secs"],
                                "timeout_secs",
                            )
                        },
                    ),
                30,
                instance_id,
            )?,
        };
        store.mark_initialized(namespace.as_str())?;
        Ok(result)
    }
    #[doc(hidden)]
    pub fn __schema_field_max_connections(&self) -> ::amethystate::ReadOnly<u32> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    #[doc(hidden)]
    pub fn __schema_field_timeout_secs(&self) -> ::amethystate::ReadOnly<u32> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    pub fn max_connections(
        &self,
    ) -> ::amethystate::Field<u32, ::amethystate::WritableMode> {
        self.max_connections.clone()
    }
    pub fn timeout_secs(
        &self,
    ) -> ::amethystate::Field<u32, ::amethystate::WritableMode> {
        self.timeout_secs.clone()
    }
    pub fn fork(&self) -> Self {
        self.fork_with_id(::amethystate::uuid::Uuid::new_v4())
    }
    #[doc(hidden)]
    pub fn fork_with_id(&self, new_id: ::amethystate::uuid::Uuid) -> Self {
        Self {
            __amethystate_instance_id: ::amethystate::observability::InstanceGuard::new(
                new_id,
                ::std::any::type_name::<Self>(),
            ),
            max_connections: self.max_connections.fork_with_id(new_id),
            timeout_secs: self.timeout_secs.fork_with_id(new_id),
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
            scope.watch(self.max_connections.subscribe(move |_| cb_clone()));
        }
        {
            let cb_clone = cb.clone();
            scope.watch(self.timeout_secs.subscribe(move |_| cb_clone()));
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
                        .max_connections
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
                        .timeout_secs
                        .subscription_with()
                        .external()
                        .register(move |_| cb_clone()),
                );
        }
        scope
    }
}
impl ::amethystate::AmeStateNode for ConnectionPool {
    const CONSTRUCTION_TERMINATES: () = {};
    fn new_node(
        store: &::amethystate::Store,
        path: &::amethystate::store::StorePath,
    ) -> ::amethystate::StorageResult<Self> {
        Self::new(store, path)
    }
    fn new_node_with_id(
        store: &::amethystate::Store,
        path: &::amethystate::store::StorePath,
        instance_id: ::amethystate::uuid::Uuid,
    ) -> ::amethystate::StorageResult<Self> {
        Self::new_with_id(store, path, instance_id)
    }
}
const _: () = <ConnectionPool as ::amethystate::AmeStateNode>::CONSTRUCTION_TERMINATES;
#[serde(crate = "::amethystate::serde")]
#[doc(hidden)]
#[allow(non_camel_case_types)]
pub struct ConnectionPool_Data {
    pub max_connections: u32,
    pub timeout_secs: u32,
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
    impl _serde::Serialize for ConnectionPool_Data {
        fn serialize<__S>(
            &self,
            __serializer: __S,
        ) -> _serde::__private228::Result<__S::Ok, __S::Error>
        where
            __S: _serde::Serializer,
        {
            let mut __serde_state = _serde::Serializer::serialize_struct(
                __serializer,
                "ConnectionPool_Data",
                false as usize + 1 + 1,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "max_connections",
                &self.max_connections,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "timeout_secs",
                &self.timeout_secs,
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
    impl<'de> _serde::Deserialize<'de> for ConnectionPool_Data {
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
                        "max_connections" => _serde::__private228::Ok(__Field::__field0),
                        "timeout_secs" => _serde::__private228::Ok(__Field::__field1),
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
                        b"max_connections" => _serde::__private228::Ok(__Field::__field0),
                        b"timeout_secs" => _serde::__private228::Ok(__Field::__field1),
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
                marker: _serde::__private228::PhantomData<ConnectionPool_Data>,
                lifetime: _serde::__private228::PhantomData<&'de ()>,
            }
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __Visitor<'de> {
                type Value = ConnectionPool_Data;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "struct ConnectionPool_Data",
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
                        u32,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    0usize,
                                    &"struct ConnectionPool_Data with 2 elements",
                                ),
                            );
                        }
                    };
                    let __field1 = match _serde::de::SeqAccess::next_element::<
                        u32,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    1usize,
                                    &"struct ConnectionPool_Data with 2 elements",
                                ),
                            );
                        }
                    };
                    _serde::__private228::Ok(ConnectionPool_Data {
                        max_connections: __field0,
                        timeout_secs: __field1,
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
                    let mut __field0: _serde::__private228::Option<u32> = _serde::__private228::None;
                    let mut __field1: _serde::__private228::Option<u32> = _serde::__private228::None;
                    while let _serde::__private228::Some(__key) = _serde::de::MapAccess::next_key::<
                        __Field,
                    >(&mut __map)? {
                        match __key {
                            __Field::__field0 => {
                                if _serde::__private228::Option::is_some(&__field0) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field(
                                            "max_connections",
                                        ),
                                    );
                                }
                                __field0 = _serde::__private228::Some(
                                    _serde::de::MapAccess::next_value::<u32>(&mut __map)?,
                                );
                            }
                            __Field::__field1 => {
                                if _serde::__private228::Option::is_some(&__field1) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field(
                                            "timeout_secs",
                                        ),
                                    );
                                }
                                __field1 = _serde::__private228::Some(
                                    _serde::de::MapAccess::next_value::<u32>(&mut __map)?,
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
                            _serde::__private228::de::missing_field("max_connections")?
                        }
                    };
                    let __field1 = match __field1 {
                        _serde::__private228::Some(__field1) => __field1,
                        _serde::__private228::None => {
                            _serde::__private228::de::missing_field("timeout_secs")?
                        }
                    };
                    _serde::__private228::Ok(ConnectionPool_Data {
                        max_connections: __field0,
                        timeout_secs: __field1,
                    })
                }
            }
            #[doc(hidden)]
            const FIELDS: &'static [&'static str] = &["max_connections", "timeout_secs"];
            _serde::Deserializer::deserialize_struct(
                __deserializer,
                "ConnectionPool_Data",
                FIELDS,
                __Visitor {
                    marker: _serde::__private228::PhantomData::<ConnectionPool_Data>,
                    lifetime: _serde::__private228::PhantomData,
                },
            )
        }
    }
};
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::default::Default for ConnectionPool_Data {
    #[inline]
    fn default() -> ConnectionPool_Data {
        ConnectionPool_Data {
            max_connections: ::core::default::Default::default(),
            timeout_secs: ::core::default::Default::default(),
        }
    }
}
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::clone::Clone for ConnectionPool_Data {
    #[inline]
    fn clone(&self) -> ConnectionPool_Data {
        ConnectionPool_Data {
            max_connections: ::core::clone::Clone::clone(&self.max_connections),
            timeout_secs: ::core::clone::Clone::clone(&self.timeout_secs),
        }
    }
}
impl ConnectionPool_Data {
    #[doc(hidden)]
    pub fn __amethystate_load_from(
        store: &::amethystate::Store,
        prefix: &::amethystate::store::StorePath,
    ) -> ::amethystate::StorageResult<Self> {
        Ok(Self {
            max_connections: <::amethystate::Store as ::amethystate::StoreExt>::get::<
                u32,
            >(
                    store,
                    &prefix
                        .join(
                            &const {
                                ::amethystate::store::StorePath::from_static(
                                    &["max_connections"],
                                    "max_connections",
                                )
                            },
                        ),
                )?
                .unwrap_or_else(|| 10),
            timeout_secs: <::amethystate::Store as ::amethystate::StoreExt>::get::<
                u32,
            >(
                    store,
                    &prefix
                        .join(
                            &const {
                                ::amethystate::store::StorePath::from_static(
                                    &["timeout_secs"],
                                    "timeout_secs",
                                )
                            },
                        ),
                )?
                .unwrap_or_else(|| 30),
        })
    }
    #[doc(hidden)]
    pub fn __amethystate_save_to(
        &self,
        store: &::amethystate::Store,
        prefix: &::amethystate::store::StorePath,
    ) -> ::amethystate::StorageResult<()> {
        <::amethystate::Store as ::amethystate::StoreExt>::set(
            &store,
            &prefix
                .join(
                    &const {
                        ::amethystate::store::StorePath::from_static(
                            &["max_connections"],
                            "max_connections",
                        )
                    },
                ),
            &self.max_connections,
        )?;
        <::amethystate::Store as ::amethystate::StoreExt>::set(
            &store,
            &prefix
                .join(
                    &const {
                        ::amethystate::store::StorePath::from_static(
                            &["timeout_secs"],
                            "timeout_secs",
                        )
                    },
                ),
            &self.timeout_secs,
        )?;
        Ok(())
    }
}
impl ::amethystate::migration::types::AmeType for ConnectionPool_Data {
    const TYPE_HASH: u32 = 0u32
        ^ ::amethystate::migration::types::fnv1a("max_connections".as_bytes())
        ^ <u32 as ::amethystate::migration::types::AmeType>::TYPE_HASH
        ^ ::amethystate::migration::types::fnv1a("timeout_secs".as_bytes())
        ^ <u32 as ::amethystate::migration::types::AmeType>::TYPE_HASH;
    const TYPE_NAME: &'static str = "ConnectionPool_Data";
}
impl ::amethystate::migration::fields::AmeStateFields for ConnectionPool_Data {
    const FIELDS: &'static [::amethystate::migration::fields::FieldDescriptor] = &[
        ::amethystate::migration::fields::FieldDescriptor {
            name: "max_connections",
            type_hash: <u32 as ::amethystate::migration::types::AmeType>::TYPE_HASH,
            type_name: "u32",
            role: ::amethystate::migration::fields::Role::Field,
            children: &[],
        },
        ::amethystate::migration::fields::FieldDescriptor {
            name: "timeout_secs",
            type_hash: <u32 as ::amethystate::migration::types::AmeType>::TYPE_HASH,
            type_name: "u32",
            role: ::amethystate::migration::fields::Role::Field,
            children: &[],
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
            max_connections: ctx.get::<u32>("max_connections")?.unwrap_or_else(|| 10),
            timeout_secs: ctx.get::<u32>("timeout_secs")?.unwrap_or_else(|| 30),
        })
    }
    fn save_struct(
        &self,
        ctx: &mut ::amethystate::MigrationContext,
    ) -> ::amethystate::StorageResult<()> {
        ctx.set("max_connections", &self.max_connections)?;
        ctx.set("timeout_secs", &self.timeout_secs)?;
        Ok(())
    }
}
impl ::amethystate::AmeState for ConnectionPool {
    type Data = ConnectionPool_Data;
}
#[allow(non_upper_case_globals)]
const _: () = {
    static __INVENTORY: ::inventory::Node = ::inventory::Node {
        value: &{
            ::amethystate::observability::SchemaEntry {
                prefix: None,
                struct_name: "ConnectionPool",
                version: 0u32,
                schema_hash: <ConnectionPool_Data as ::amethystate::migration::types::AmeType>::TYPE_HASH,
                fields: <ConnectionPool_Data as ::amethystate::migration::fields::AmeStateFields>::FIELDS,
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
pub struct DatabaseState {
    __amethystate_instance_id: ::std::sync::Arc<
        ::amethystate::observability::InstanceGuard,
    >,
    pub pool: ::std::sync::Arc<ConnectionPool>,
}
#[automatically_derived]
impl ::core::clone::Clone for DatabaseState {
    #[inline]
    fn clone(&self) -> DatabaseState {
        DatabaseState {
            __amethystate_instance_id: ::core::clone::Clone::clone(
                &self.__amethystate_instance_id,
            ),
            pool: ::core::clone::Clone::clone(&self.pool),
        }
    }
}
impl ::std::fmt::Debug for DatabaseState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        struct __AmeOpaque;
        impl ::std::fmt::Debug for __AmeOpaque {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str("<opaque>")
            }
        }
        struct __AmeW<'a, T>(&'a T);
        trait __AmeViaDebug {
            fn __ame(&self) -> &dyn ::std::fmt::Debug;
        }
        impl<'a, T: ::std::fmt::Debug> __AmeViaDebug for __AmeW<'a, T> {
            fn __ame(&self) -> &dyn ::std::fmt::Debug {
                self.0
            }
        }
        trait __AmeViaFallback {
            fn __ame(&self) -> &dyn ::std::fmt::Debug;
        }
        impl<'a, T> __AmeViaFallback for &__AmeW<'a, T> {
            fn __ame(&self) -> &dyn ::std::fmt::Debug {
                &__AmeOpaque
            }
        }
        f.debug_struct("DatabaseState")
            .field("pool", (&__AmeW(&self.pool)).__ame())
            .finish()
    }
}
impl ::amethystate::StateScope for DatabaseState {
    const PATH: ::amethystate::store::StorePath = ::amethystate::store::StorePath::from_static(
        &["sys", "database"],
        "sys.database",
    );
    const KEY: &'static str = "sys.database";
}
impl DatabaseState {
    pub fn new_with(store: &::amethystate::Store) -> ::amethystate::StorageResult<Self> {
        Self::new_with_id(store, ::amethystate::uuid::Uuid::new_v4())
    }
    pub fn new_with_id(
        store: &::amethystate::Store,
        instance_id: ::amethystate::uuid::Uuid,
    ) -> ::amethystate::StorageResult<Self> {
        use ::amethystate::{StoreBackend, StoreExt};
        let __amethystate_guard = ::amethystate::observability::InstanceGuard::new(
            instance_id,
            ::std::any::type_name::<Self>(),
        );
        let result = Self {
            __amethystate_instance_id: __amethystate_guard,
            pool: ::std::sync::Arc::new(
                ConnectionPool::new_with_id(
                    store,
                    <Self as ::amethystate::StateScope>::PATH
                        .join(
                            &const {
                                ::amethystate::store::StorePath::from_static(
                                    &["pool"],
                                    "pool",
                                )
                            },
                        ),
                    instance_id,
                )?,
            ),
        };
        store.mark_initialized(<Self as ::amethystate::StateScope>::PATH.as_str())?;
        Ok(result)
    }
    #[doc(hidden)]
    pub fn __schema_field_pool(&self) -> ::amethystate::ReadOnly<ConnectionPool> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    pub fn pool(&self) -> ::std::sync::Arc<ConnectionPool> {
        self.pool.clone()
    }
    pub fn fork(&self) -> Self {
        self.fork_with_id(::amethystate::uuid::Uuid::new_v4())
    }
    #[doc(hidden)]
    pub fn fork_with_id(&self, new_id: ::amethystate::uuid::Uuid) -> Self {
        Self {
            __amethystate_instance_id: ::amethystate::observability::InstanceGuard::new(
                new_id,
                ::std::any::type_name::<Self>(),
            ),
            pool: ::std::sync::Arc::new(self.pool.fork_with_id(new_id)),
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
            scope.watch_scope(self.pool.subscribe_all(move || cb_clone()));
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
            scope.watch_scope(self.pool.subscribe_all_external(move || cb_clone()));
        }
        scope
    }
}
impl DatabaseState {
    pub fn new() -> ::amethystate::StorageResult<Self> {
        let store = ::amethystate::global_store();
        Self::new_with(&store)
    }
}
impl ::amethystate::AmeStateNode for DatabaseState {
    const CONSTRUCTION_TERMINATES: () = {
        let _: () = <ConnectionPool as ::amethystate::AmeStateNode>::CONSTRUCTION_TERMINATES;
    };
    fn new_node(
        store: &::amethystate::Store,
        _path: &::amethystate::store::StorePath,
    ) -> ::amethystate::StorageResult<Self> {
        Self::new_with(store)
    }
    fn new_node_with_id(
        store: &::amethystate::Store,
        _path: &::amethystate::store::StorePath,
        instance_id: ::amethystate::uuid::Uuid,
    ) -> ::amethystate::StorageResult<Self> {
        Self::new_with_id(store, instance_id)
    }
}
const _: () = <DatabaseState as ::amethystate::AmeStateNode>::CONSTRUCTION_TERMINATES;
#[serde(crate = "::amethystate::serde")]
#[doc(hidden)]
#[allow(non_camel_case_types)]
pub struct DatabaseState_Data {
    pub pool: <ConnectionPool as ::amethystate::AmeState>::Data,
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
    impl _serde::Serialize for DatabaseState_Data {
        fn serialize<__S>(
            &self,
            __serializer: __S,
        ) -> _serde::__private228::Result<__S::Ok, __S::Error>
        where
            __S: _serde::Serializer,
        {
            let mut __serde_state = _serde::Serializer::serialize_struct(
                __serializer,
                "DatabaseState_Data",
                false as usize + 1,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "pool",
                &self.pool,
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
    impl<'de> _serde::Deserialize<'de> for DatabaseState_Data {
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
                        "pool" => _serde::__private228::Ok(__Field::__field0),
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
                        b"pool" => _serde::__private228::Ok(__Field::__field0),
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
                marker: _serde::__private228::PhantomData<DatabaseState_Data>,
                lifetime: _serde::__private228::PhantomData<&'de ()>,
            }
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __Visitor<'de> {
                type Value = DatabaseState_Data;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "struct DatabaseState_Data",
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
                        <ConnectionPool as ::amethystate::AmeState>::Data,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    0usize,
                                    &"struct DatabaseState_Data with 1 element",
                                ),
                            );
                        }
                    };
                    _serde::__private228::Ok(DatabaseState_Data {
                        pool: __field0,
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
                        <ConnectionPool as ::amethystate::AmeState>::Data,
                    > = _serde::__private228::None;
                    while let _serde::__private228::Some(__key) = _serde::de::MapAccess::next_key::<
                        __Field,
                    >(&mut __map)? {
                        match __key {
                            __Field::__field0 => {
                                if _serde::__private228::Option::is_some(&__field0) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field("pool"),
                                    );
                                }
                                __field0 = _serde::__private228::Some(
                                    _serde::de::MapAccess::next_value::<
                                        <ConnectionPool as ::amethystate::AmeState>::Data,
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
                            _serde::__private228::de::missing_field("pool")?
                        }
                    };
                    _serde::__private228::Ok(DatabaseState_Data {
                        pool: __field0,
                    })
                }
            }
            #[doc(hidden)]
            const FIELDS: &'static [&'static str] = &["pool"];
            _serde::Deserializer::deserialize_struct(
                __deserializer,
                "DatabaseState_Data",
                FIELDS,
                __Visitor {
                    marker: _serde::__private228::PhantomData::<DatabaseState_Data>,
                    lifetime: _serde::__private228::PhantomData,
                },
            )
        }
    }
};
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::default::Default for DatabaseState_Data {
    #[inline]
    fn default() -> DatabaseState_Data {
        DatabaseState_Data {
            pool: ::core::default::Default::default(),
        }
    }
}
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::clone::Clone for DatabaseState_Data {
    #[inline]
    fn clone(&self) -> DatabaseState_Data {
        DatabaseState_Data {
            pool: ::core::clone::Clone::clone(&self.pool),
        }
    }
}
impl DatabaseState_Data {}
impl ::amethystate::migration::types::AmeType for DatabaseState_Data {
    const TYPE_HASH: u32 = 0u32
        ^ ::amethystate::migration::types::fnv1a("pool".as_bytes())
        ^ <<ConnectionPool as ::amethystate::AmeState>::Data as ::amethystate::migration::types::AmeType>::TYPE_HASH;
    const TYPE_NAME: &'static str = "DatabaseState_Data";
}
impl ::amethystate::migration::fields::AmeStateFields for DatabaseState_Data {
    const FIELDS: &'static [::amethystate::migration::fields::FieldDescriptor] = &[
        ::amethystate::migration::fields::FieldDescriptor {
            name: "pool",
            type_hash: 0xDEADBEEF
                ^ <<ConnectionPool as ::amethystate::AmeState>::Data as ::amethystate::migration::types::AmeType>::TYPE_HASH,
            type_name: "ConnectionPool",
            role: ::amethystate::migration::fields::Role::Node,
            children: <<ConnectionPool as ::amethystate::AmeState>::Data as ::amethystate::migration::fields::AmeStateFields>::FIELDS,
        },
    ];
    const VERSION: u32 = 0u32;
    const SCHEMA_HASH: u32 = ::amethystate::migration::types::schema_hash(Self::FIELDS);
    const PARENT_PREFIX: &'static str = "sys.database";
    const MIGRATION_DEPS: &'static [&'static str] = &[];
    fn load_struct(
        ctx: &mut ::amethystate::MigrationContext,
    ) -> ::amethystate::StorageResult<Self> {
        Ok(Self {
            pool: {
                let mut sub_ctx = ctx.scoped("pool");
                <<ConnectionPool as ::amethystate::AmeState>::Data as ::amethystate::migration::fields::AmeStateFields>::load_struct(
                    &mut sub_ctx,
                )?
            },
        })
    }
    fn save_struct(
        &self,
        ctx: &mut ::amethystate::MigrationContext,
    ) -> ::amethystate::StorageResult<()> {
        {
            let mut sub_ctx = ctx.scoped("pool");
            self.pool.save_struct(&mut sub_ctx)?;
        }
        Ok(())
    }
}
impl ::amethystate::AmeState for DatabaseState {
    type Data = DatabaseState_Data;
}
#[allow(non_upper_case_globals)]
const _: () = {
    static __INVENTORY: ::inventory::Node = ::inventory::Node {
        value: &{
            ::amethystate::observability::SchemaEntry {
                prefix: Some(const {
                    ::amethystate::store::StorePath::from_static(
                        &["sys", "database"],
                        "sys.database",
                    )
                }),
                struct_name: "DatabaseState",
                version: 0u32,
                schema_hash: <DatabaseState_Data as ::amethystate::migration::types::AmeType>::TYPE_HASH,
                fields: <DatabaseState_Data as ::amethystate::migration::fields::AmeStateFields>::FIELDS,
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
impl ::amethystate::AmeStateSlice for DatabaseState {
    fn load_slice(store: &::amethystate::Store) -> ::amethystate::StorageResult<Self> {
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
pub struct InspectorState {
    __amethystate_instance_id: ::std::sync::Arc<
        ::amethystate::observability::InstanceGuard,
    >,
    pub db_pool_view: ::std::sync::Arc<ConnectionPool>,
}
#[automatically_derived]
impl ::core::clone::Clone for InspectorState {
    #[inline]
    fn clone(&self) -> InspectorState {
        InspectorState {
            __amethystate_instance_id: ::core::clone::Clone::clone(
                &self.__amethystate_instance_id,
            ),
            db_pool_view: ::core::clone::Clone::clone(&self.db_pool_view),
        }
    }
}
impl ::std::fmt::Debug for InspectorState {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        struct __AmeOpaque;
        impl ::std::fmt::Debug for __AmeOpaque {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str("<opaque>")
            }
        }
        struct __AmeW<'a, T>(&'a T);
        trait __AmeViaDebug {
            fn __ame(&self) -> &dyn ::std::fmt::Debug;
        }
        impl<'a, T: ::std::fmt::Debug> __AmeViaDebug for __AmeW<'a, T> {
            fn __ame(&self) -> &dyn ::std::fmt::Debug {
                self.0
            }
        }
        trait __AmeViaFallback {
            fn __ame(&self) -> &dyn ::std::fmt::Debug;
        }
        impl<'a, T> __AmeViaFallback for &__AmeW<'a, T> {
            fn __ame(&self) -> &dyn ::std::fmt::Debug {
                &__AmeOpaque
            }
        }
        f.debug_struct("InspectorState")
            .field("db_pool_view", (&__AmeW(&self.db_pool_view)).__ame())
            .finish()
    }
}
impl ::amethystate::StateScope for InspectorState {
    const PATH: ::amethystate::store::StorePath = ::amethystate::store::StorePath::from_static(
        &["ui", "inspector"],
        "ui.inspector",
    );
    const KEY: &'static str = "ui.inspector";
}
impl InspectorState {
    pub fn new_with(store: &::amethystate::Store) -> ::amethystate::StorageResult<Self> {
        Self::new_with_id(store, ::amethystate::uuid::Uuid::new_v4())
    }
    pub fn new_with_id(
        store: &::amethystate::Store,
        instance_id: ::amethystate::uuid::Uuid,
    ) -> ::amethystate::StorageResult<Self> {
        use ::amethystate::{StoreBackend, StoreExt};
        let __amethystate_guard = ::amethystate::observability::InstanceGuard::new(
            instance_id,
            ::std::any::type_name::<Self>(),
        );
        let result = Self {
            __amethystate_instance_id: __amethystate_guard,
            db_pool_view: {
                const _: fn() = || {
                    fn assert_node_type<T>(_: ::amethystate::ReadOnly<T>) {}
                    let _ = || assert_node_type(
                        unsafe { (&*::core::ptr::null::<DatabaseState>()) }
                            .__schema_field_pool(),
                    );
                    let _ = unsafe { (&*::core::ptr::null::<DatabaseState>()) }
                        .__schema_field_pool();
                };
                let path = <DatabaseState as ::amethystate::StateScope>::PATH
                    .join(
                        &const {
                            ::amethystate::store::StorePath::from_static(
                                &["pool"],
                                "pool",
                            )
                        },
                    );
                ::std::sync::Arc::new(
                    <ConnectionPool as ::amethystate::AmeStateNode>::new_node_with_id(
                        store,
                        &path,
                        instance_id,
                    )?,
                )
            },
        };
        store.mark_initialized(<Self as ::amethystate::StateScope>::PATH.as_str())?;
        Ok(result)
    }
    #[doc(hidden)]
    pub fn __schema_field_db_pool_view(
        &self,
    ) -> ::amethystate::ReadOnly<ConnectionPool> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    pub fn db_pool_view(&self) -> ::std::sync::Arc<ConnectionPool> {
        self.db_pool_view.clone()
    }
    pub fn fork(&self) -> Self {
        self.fork_with_id(::amethystate::uuid::Uuid::new_v4())
    }
    #[doc(hidden)]
    pub fn fork_with_id(&self, new_id: ::amethystate::uuid::Uuid) -> Self {
        Self {
            __amethystate_instance_id: ::amethystate::observability::InstanceGuard::new(
                new_id,
                ::std::any::type_name::<Self>(),
            ),
            db_pool_view: ::std::sync::Arc::new(self.db_pool_view.fork_with_id(new_id)),
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
            scope.watch_scope(self.db_pool_view.subscribe_all(move || cb_clone()));
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
                .watch_scope(
                    self.db_pool_view.subscribe_all_external(move || cb_clone()),
                );
        }
        scope
    }
}
impl InspectorState {
    pub fn new() -> ::amethystate::StorageResult<Self> {
        let store = ::amethystate::global_store();
        Self::new_with(&store)
    }
}
impl ::amethystate::AmeStateNode for InspectorState {
    const CONSTRUCTION_TERMINATES: () = {
        let _: () = <ConnectionPool as ::amethystate::AmeStateNode>::CONSTRUCTION_TERMINATES;
    };
    fn new_node(
        store: &::amethystate::Store,
        _path: &::amethystate::store::StorePath,
    ) -> ::amethystate::StorageResult<Self> {
        Self::new_with(store)
    }
    fn new_node_with_id(
        store: &::amethystate::Store,
        _path: &::amethystate::store::StorePath,
        instance_id: ::amethystate::uuid::Uuid,
    ) -> ::amethystate::StorageResult<Self> {
        Self::new_with_id(store, instance_id)
    }
}
const _: () = <InspectorState as ::amethystate::AmeStateNode>::CONSTRUCTION_TERMINATES;
#[serde(crate = "::amethystate::serde")]
#[doc(hidden)]
#[allow(non_camel_case_types)]
pub struct InspectorState_Data {}
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
    impl _serde::Serialize for InspectorState_Data {
        fn serialize<__S>(
            &self,
            __serializer: __S,
        ) -> _serde::__private228::Result<__S::Ok, __S::Error>
        where
            __S: _serde::Serializer,
        {
            let __serde_state = _serde::Serializer::serialize_struct(
                __serializer,
                "InspectorState_Data",
                false as usize,
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
    impl<'de> _serde::Deserialize<'de> for InspectorState_Data {
        fn deserialize<__D>(
            __deserializer: __D,
        ) -> _serde::__private228::Result<Self, __D::Error>
        where
            __D: _serde::Deserializer<'de>,
        {
            #[allow(non_camel_case_types)]
            #[doc(hidden)]
            enum __Field {
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
                marker: _serde::__private228::PhantomData<InspectorState_Data>,
                lifetime: _serde::__private228::PhantomData<&'de ()>,
            }
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __Visitor<'de> {
                type Value = InspectorState_Data;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "struct InspectorState_Data",
                    )
                }
                #[inline]
                fn visit_seq<__A>(
                    self,
                    _: __A,
                ) -> _serde::__private228::Result<Self::Value, __A::Error>
                where
                    __A: _serde::de::SeqAccess<'de>,
                {
                    _serde::__private228::Ok(InspectorState_Data {})
                }
                #[inline]
                fn visit_map<__A>(
                    self,
                    mut __map: __A,
                ) -> _serde::__private228::Result<Self::Value, __A::Error>
                where
                    __A: _serde::de::MapAccess<'de>,
                {
                    while let _serde::__private228::Some(__key) = _serde::de::MapAccess::next_key::<
                        __Field,
                    >(&mut __map)? {
                        match __key {
                            _ => {
                                let _ = _serde::de::MapAccess::next_value::<
                                    _serde::de::IgnoredAny,
                                >(&mut __map)?;
                            }
                        }
                    }
                    _serde::__private228::Ok(InspectorState_Data {})
                }
            }
            #[doc(hidden)]
            const FIELDS: &'static [&'static str] = &[];
            _serde::Deserializer::deserialize_struct(
                __deserializer,
                "InspectorState_Data",
                FIELDS,
                __Visitor {
                    marker: _serde::__private228::PhantomData::<InspectorState_Data>,
                    lifetime: _serde::__private228::PhantomData,
                },
            )
        }
    }
};
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::default::Default for InspectorState_Data {
    #[inline]
    fn default() -> InspectorState_Data {
        InspectorState_Data {}
    }
}
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::clone::Clone for InspectorState_Data {
    #[inline]
    fn clone(&self) -> InspectorState_Data {
        InspectorState_Data {}
    }
}
impl InspectorState_Data {}
impl ::amethystate::migration::types::AmeType for InspectorState_Data {
    const TYPE_HASH: u32 = 0u32;
    const TYPE_NAME: &'static str = "InspectorState_Data";
}
impl ::amethystate::migration::fields::AmeStateFields for InspectorState_Data {
    const FIELDS: &'static [::amethystate::migration::fields::FieldDescriptor] = &[];
    const VERSION: u32 = 0u32;
    const SCHEMA_HASH: u32 = ::amethystate::migration::types::schema_hash(Self::FIELDS);
    const PARENT_PREFIX: &'static str = "ui.inspector";
    const MIGRATION_DEPS: &'static [&'static str] = &[
        <DatabaseState as ::amethystate::StateScope>::KEY,
    ];
    fn load_struct(
        ctx: &mut ::amethystate::MigrationContext,
    ) -> ::amethystate::StorageResult<Self> {
        Ok(Self {})
    }
    fn save_struct(
        &self,
        ctx: &mut ::amethystate::MigrationContext,
    ) -> ::amethystate::StorageResult<()> {
        Ok(())
    }
}
impl ::amethystate::AmeState for InspectorState {
    type Data = InspectorState_Data;
}
#[allow(non_upper_case_globals)]
const _: () = {
    static __INVENTORY: ::inventory::Node = ::inventory::Node {
        value: &{
            ::amethystate::observability::SchemaEntry {
                prefix: Some(const {
                    ::amethystate::store::StorePath::from_static(
                        &["ui", "inspector"],
                        "ui.inspector",
                    )
                }),
                struct_name: "InspectorState",
                version: 0u32,
                schema_hash: <InspectorState_Data as ::amethystate::migration::types::AmeType>::TYPE_HASH,
                fields: <InspectorState_Data as ::amethystate::migration::fields::AmeStateFields>::FIELDS,
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
impl ::amethystate::AmeStateSlice for InspectorState {
    fn load_slice(store: &::amethystate::Store) -> ::amethystate::StorageResult<Self> {
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
