use amethystate_macros::amethystate;
pub struct NetworkState<S: ::amethystate::Store = ::amethystate::DefaultStore> {
    __amethystate_instance_id: ::amethystate::uuid::Uuid,
    pub port: ::amethystate::Field<u16, S, ::amethystate::WritableMode>,
    pub host: ::amethystate::Field<String, S, ::amethystate::WritableMode>,
}
#[automatically_derived]
impl<S: ::core::clone::Clone + ::amethystate::Store> ::core::clone::Clone
for NetworkState<S> {
    #[inline]
    fn clone(&self) -> NetworkState<S> {
        NetworkState {
            __amethystate_instance_id: ::core::clone::Clone::clone(
                &self.__amethystate_instance_id,
            ),
            port: ::core::clone::Clone::clone(&self.port),
            host: ::core::clone::Clone::clone(&self.host),
        }
    }
}
impl<S: ::amethystate::Store> ::std::fmt::Debug for NetworkState<S>
where
    ::amethystate::Field<u16, S, ::amethystate::WritableMode>: ::std::fmt::Debug,
    ::amethystate::Field<String, S, ::amethystate::WritableMode>: ::std::fmt::Debug,
{
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.debug_struct("NetworkState")
            .field("port", &self.port)
            .field("host", &self.host)
            .finish()
    }
}
impl<S: ::amethystate::Store> ::amethystate::StateScope for NetworkState<S> {
    const PREFIX: &'static str = "net";
}
impl<S: ::amethystate::Store> NetworkState<S> {
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
            port: ::amethystate::store::field::<
                Self,
                u16,
                S,
            >(store, "port", 8080, instance_id)?,
            host: ::amethystate::store::field::<
                Self,
                String,
                S,
            >(store, "host", "127.0.0.1".to_string(), instance_id)?,
        };
        store.mark_initialized(<Self as ::amethystate::StateScope>::PREFIX)?;
        Ok(result)
    }
    #[doc(hidden)]
    pub fn __schema_field_port(&self) -> ::amethystate::Writable<u16> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    #[doc(hidden)]
    pub fn __schema_field_host(&self) -> ::amethystate::ReadOnly<String> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    pub fn port(&self) -> ::amethystate::Field<u16, S, ::amethystate::WritableMode> {
        self.port.clone()
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
            port: self.port.fork_with_id(new_id),
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
            scope.watch(self.port.subscribe(move |_| cb_clone()));
        }
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
                        .port
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
                        .host
                        .subscription_with()
                        .external()
                        .register(move |_| cb_clone()),
                );
        }
        scope
    }
}
impl NetworkState<::amethystate::DefaultStore> {
    pub fn new() -> ::amethystate::StorageResult<Self> {
        let store = ::amethystate::global_store();
        Self::new_with(&store)
    }
}
impl<S: ::amethystate::Store> ::amethystate::AmeStateNode<S> for NetworkState<S> {
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
pub struct NetworkState_Data {
    pub host: String,
    pub port: u16,
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
    impl _serde::Serialize for NetworkState_Data {
        fn serialize<__S>(
            &self,
            __serializer: __S,
        ) -> _serde::__private228::Result<__S::Ok, __S::Error>
        where
            __S: _serde::Serializer,
        {
            let mut __serde_state = _serde::Serializer::serialize_struct(
                __serializer,
                "NetworkState_Data",
                false as usize + 1 + 1,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "host",
                &self.host,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "port",
                &self.port,
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
    impl<'de> _serde::Deserialize<'de> for NetworkState_Data {
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
                        "host" => _serde::__private228::Ok(__Field::__field0),
                        "port" => _serde::__private228::Ok(__Field::__field1),
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
                        b"port" => _serde::__private228::Ok(__Field::__field1),
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
                marker: _serde::__private228::PhantomData<NetworkState_Data>,
                lifetime: _serde::__private228::PhantomData<&'de ()>,
            }
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __Visitor<'de> {
                type Value = NetworkState_Data;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "struct NetworkState_Data",
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
                                    &"struct NetworkState_Data with 2 elements",
                                ),
                            );
                        }
                    };
                    let __field1 = match _serde::de::SeqAccess::next_element::<
                        u16,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    1usize,
                                    &"struct NetworkState_Data with 2 elements",
                                ),
                            );
                        }
                    };
                    _serde::__private228::Ok(NetworkState_Data {
                        host: __field0,
                        port: __field1,
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
                    let mut __field1: _serde::__private228::Option<u16> = _serde::__private228::None;
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
                            __Field::__field1 => {
                                if _serde::__private228::Option::is_some(&__field1) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field("port"),
                                    );
                                }
                                __field1 = _serde::__private228::Some(
                                    _serde::de::MapAccess::next_value::<u16>(&mut __map)?,
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
                    let __field1 = match __field1 {
                        _serde::__private228::Some(__field1) => __field1,
                        _serde::__private228::None => {
                            _serde::__private228::de::missing_field("port")?
                        }
                    };
                    _serde::__private228::Ok(NetworkState_Data {
                        host: __field0,
                        port: __field1,
                    })
                }
            }
            #[doc(hidden)]
            const FIELDS: &'static [&'static str] = &["host", "port"];
            _serde::Deserializer::deserialize_struct(
                __deserializer,
                "NetworkState_Data",
                FIELDS,
                __Visitor {
                    marker: _serde::__private228::PhantomData::<NetworkState_Data>,
                    lifetime: _serde::__private228::PhantomData,
                },
            )
        }
    }
};
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::default::Default for NetworkState_Data {
    #[inline]
    fn default() -> NetworkState_Data {
        NetworkState_Data {
            host: ::core::default::Default::default(),
            port: ::core::default::Default::default(),
        }
    }
}
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::clone::Clone for NetworkState_Data {
    #[inline]
    fn clone(&self) -> NetworkState_Data {
        NetworkState_Data {
            host: ::core::clone::Clone::clone(&self.host),
            port: ::core::clone::Clone::clone(&self.port),
        }
    }
}
impl NetworkState_Data {}
impl ::amethystate::migration::types::AmeType for NetworkState_Data {
    const TYPE_HASH: u32 = 0u32
        ^ ::amethystate::migration::types::fnv1a("host".as_bytes())
        ^ <String as ::amethystate::migration::types::AmeType>::TYPE_HASH
        ^ ::amethystate::migration::types::fnv1a("port".as_bytes())
        ^ <u16 as ::amethystate::migration::types::AmeType>::TYPE_HASH;
    const TYPE_NAME: &'static str = "NetworkState_Data";
}
impl ::amethystate::migration::fields::AmeStateFields for NetworkState_Data {
    const FIELDS: &'static [::amethystate::migration::fields::FieldDescriptor] = &[
        ::amethystate::migration::fields::FieldDescriptor {
            name: "host",
            type_hash: <String as ::amethystate::migration::types::AmeType>::TYPE_HASH,
            type_name: "String",
        },
        ::amethystate::migration::fields::FieldDescriptor {
            name: "port",
            type_hash: <u16 as ::amethystate::migration::types::AmeType>::TYPE_HASH,
            type_name: "u16",
        },
    ];
    const VERSION: u32 = 0u32;
    const SCHEMA_HASH: u32 = ::amethystate::migration::types::schema_hash(Self::FIELDS);
    const PARENT_PREFIX: &'static str = "net";
    const MIGRATION_DEPS: &'static [&'static str] = &[];
    fn load_struct(
        ctx: &mut ::amethystate::MigrationContext,
    ) -> ::amethystate::StorageResult<Self> {
        Ok(Self {
            host: ctx.get::<String>("host")?.unwrap_or_else(|| "127.0.0.1".to_string()),
            port: ctx.get::<u16>("port")?.unwrap_or_else(|| 8080),
        })
    }
    fn save_struct(
        &self,
        ctx: &mut ::amethystate::MigrationContext,
    ) -> ::amethystate::StorageResult<()> {
        ctx.set("host", &self.host)?;
        ctx.set("port", &self.port)?;
        Ok(())
    }
}
impl<S: ::amethystate::Store> ::amethystate::AmeState for NetworkState<S> {
    type Data = NetworkState_Data;
}
#[allow(non_upper_case_globals)]
const _: () = {
    static __INVENTORY: ::inventory::Node = ::inventory::Node {
        value: &{
            ::amethystate::observability::SchemaEntry {
                prefix: Some("net"),
                struct_name: "NetworkState",
                version: 0u32,
                schema_hash: <NetworkState_Data as ::amethystate::migration::types::AmeType>::TYPE_HASH,
                fields: <NetworkState_Data as ::amethystate::migration::fields::AmeStateFields>::FIELDS,
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
impl<S: ::amethystate::Store> ::amethystate::AmeStateSlice<S> for NetworkState<S> {
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
pub struct UiState<S: ::amethystate::Store = ::amethystate::DefaultStore> {
    __amethystate_instance_id: ::amethystate::uuid::Uuid,
    pub proxy_port: ::amethystate::Field<u16, S, ::amethystate::ReadOnlyMode>,
    pub proxy_host: ::amethystate::Field<String, S, ::amethystate::ReadOnlyMode>,
}
#[automatically_derived]
impl<S: ::core::clone::Clone + ::amethystate::Store> ::core::clone::Clone
for UiState<S> {
    #[inline]
    fn clone(&self) -> UiState<S> {
        UiState {
            __amethystate_instance_id: ::core::clone::Clone::clone(
                &self.__amethystate_instance_id,
            ),
            proxy_port: ::core::clone::Clone::clone(&self.proxy_port),
            proxy_host: ::core::clone::Clone::clone(&self.proxy_host),
        }
    }
}
impl<S: ::amethystate::Store> ::std::fmt::Debug for UiState<S>
where
    ::amethystate::Field<u16, S, ::amethystate::ReadOnlyMode>: ::std::fmt::Debug,
    ::amethystate::Field<String, S, ::amethystate::ReadOnlyMode>: ::std::fmt::Debug,
{
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.debug_struct("UiState")
            .field("proxy_port", &self.proxy_port)
            .field("proxy_host", &self.proxy_host)
            .finish()
    }
}
impl<S: ::amethystate::Store> ::amethystate::StateScope for UiState<S> {
    const PREFIX: &'static str = "ui";
}
impl<S: ::amethystate::Store> UiState<S> {
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
            proxy_port: {
                const _: fn() = || {
                    trait TypeCheck<T> {}
                    impl<T> TypeCheck<T> for ::amethystate::ReadOnly<T> {}
                    impl<T> TypeCheck<T> for ::amethystate::Writable<T> {}
                    fn assert_field_type_matches_lookup<T, M: TypeCheck<T>>(_: M) {}
                    assert_field_type_matches_lookup::<
                        u16,
                        _,
                    >(
                        unsafe { (&*::core::ptr::null::<NetworkState>()) }
                            .__schema_field_port(),
                    );
                };
                let parent_prefix = <NetworkState as ::amethystate::StateScope>::PREFIX;
                let path = if parent_prefix == "." {
                    "port".to_string()
                } else {
                    ::alloc::__export::must_use({
                        ::alloc::fmt::format(
                            format_args!("{0}.{1}", parent_prefix, "port"),
                        )
                    })
                };
                ::amethystate::store::field_with_path::<
                    u16,
                    _,
                    ::amethystate::ReadOnlyMode,
                >(
                    store,
                    ::std::sync::Arc::from(path),
                    ::std::default::Default::default(),
                    instance_id,
                )?
            },
            proxy_host: {
                const _: fn() = || {
                    trait TypeCheck<T> {}
                    impl<T> TypeCheck<T> for ::amethystate::ReadOnly<T> {}
                    impl<T> TypeCheck<T> for ::amethystate::Writable<T> {}
                    fn assert_field_type_matches_lookup<T, M: TypeCheck<T>>(_: M) {}
                    assert_field_type_matches_lookup::<
                        String,
                        _,
                    >(
                        unsafe { (&*::core::ptr::null::<NetworkState>()) }
                            .__schema_field_host(),
                    );
                };
                let parent_prefix = <NetworkState as ::amethystate::StateScope>::PREFIX;
                let path = if parent_prefix == "." {
                    "host".to_string()
                } else {
                    ::alloc::__export::must_use({
                        ::alloc::fmt::format(
                            format_args!("{0}.{1}", parent_prefix, "host"),
                        )
                    })
                };
                ::amethystate::store::field_with_path::<
                    String,
                    _,
                    ::amethystate::ReadOnlyMode,
                >(
                    store,
                    ::std::sync::Arc::from(path),
                    ::std::default::Default::default(),
                    instance_id,
                )?
            },
        };
        store.mark_initialized(<Self as ::amethystate::StateScope>::PREFIX)?;
        Ok(result)
    }
    #[doc(hidden)]
    pub fn __schema_field_proxy_port(&self) -> ::amethystate::ReadOnly<u16> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    #[doc(hidden)]
    pub fn __schema_field_proxy_host(&self) -> ::amethystate::ReadOnly<String> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    pub fn proxy_port(
        &self,
    ) -> ::amethystate::Field<u16, S, ::amethystate::ReadOnlyMode> {
        self.proxy_port.clone()
    }
    pub fn proxy_host(
        &self,
    ) -> ::amethystate::Field<String, S, ::amethystate::ReadOnlyMode> {
        self.proxy_host.clone()
    }
    pub fn fork(&self) -> Self {
        self.fork_with_id(::amethystate::uuid::Uuid::new_v4())
    }
    #[doc(hidden)]
    pub fn fork_with_id(&self, new_id: ::amethystate::uuid::Uuid) -> Self {
        Self {
            __amethystate_instance_id: new_id,
            proxy_port: self.proxy_port.fork_with_id(new_id),
            proxy_host: self.proxy_host.fork_with_id(new_id),
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
            scope.watch(self.proxy_port.subscribe(move |_| cb_clone()));
        }
        {
            let cb_clone = cb.clone();
            scope.watch(self.proxy_host.subscribe(move |_| cb_clone()));
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
                        .proxy_port
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
                        .proxy_host
                        .subscription_with()
                        .external()
                        .register(move |_| cb_clone()),
                );
        }
        scope
    }
}
impl UiState<::amethystate::DefaultStore> {
    pub fn new() -> ::amethystate::StorageResult<Self> {
        let store = ::amethystate::global_store();
        Self::new_with(&store)
    }
}
impl<S: ::amethystate::Store> ::amethystate::AmeStateNode<S> for UiState<S> {
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
pub struct UiState_Data {}
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
    impl _serde::Serialize for UiState_Data {
        fn serialize<__S>(
            &self,
            __serializer: __S,
        ) -> _serde::__private228::Result<__S::Ok, __S::Error>
        where
            __S: _serde::Serializer,
        {
            let __serde_state = _serde::Serializer::serialize_struct(
                __serializer,
                "UiState_Data",
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
    impl<'de> _serde::Deserialize<'de> for UiState_Data {
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
                marker: _serde::__private228::PhantomData<UiState_Data>,
                lifetime: _serde::__private228::PhantomData<&'de ()>,
            }
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __Visitor<'de> {
                type Value = UiState_Data;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "struct UiState_Data",
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
                    _serde::__private228::Ok(UiState_Data {})
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
                    _serde::__private228::Ok(UiState_Data {})
                }
            }
            #[doc(hidden)]
            const FIELDS: &'static [&'static str] = &[];
            _serde::Deserializer::deserialize_struct(
                __deserializer,
                "UiState_Data",
                FIELDS,
                __Visitor {
                    marker: _serde::__private228::PhantomData::<UiState_Data>,
                    lifetime: _serde::__private228::PhantomData,
                },
            )
        }
    }
};
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::default::Default for UiState_Data {
    #[inline]
    fn default() -> UiState_Data {
        UiState_Data {}
    }
}
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::clone::Clone for UiState_Data {
    #[inline]
    fn clone(&self) -> UiState_Data {
        UiState_Data {}
    }
}
impl UiState_Data {}
impl ::amethystate::migration::types::AmeType for UiState_Data {
    const TYPE_HASH: u32 = 0u32;
    const TYPE_NAME: &'static str = "UiState_Data";
}
impl ::amethystate::migration::fields::AmeStateFields for UiState_Data {
    const FIELDS: &'static [::amethystate::migration::fields::FieldDescriptor] = &[];
    const VERSION: u32 = 0u32;
    const SCHEMA_HASH: u32 = ::amethystate::migration::types::schema_hash(Self::FIELDS);
    const PARENT_PREFIX: &'static str = "ui";
    const MIGRATION_DEPS: &'static [&'static str] = &[
        <NetworkState as ::amethystate::StateScope>::PREFIX,
        <NetworkState as ::amethystate::StateScope>::PREFIX,
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
impl<S: ::amethystate::Store> ::amethystate::AmeState for UiState<S> {
    type Data = UiState_Data;
}
#[allow(non_upper_case_globals)]
const _: () = {
    static __INVENTORY: ::inventory::Node = ::inventory::Node {
        value: &{
            ::amethystate::observability::SchemaEntry {
                prefix: Some("ui"),
                struct_name: "UiState",
                version: 0u32,
                schema_hash: <UiState_Data as ::amethystate::migration::types::AmeType>::TYPE_HASH,
                fields: <UiState_Data as ::amethystate::migration::fields::AmeStateFields>::FIELDS,
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
impl<S: ::amethystate::Store> ::amethystate::AmeStateSlice<S> for UiState<S> {
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
