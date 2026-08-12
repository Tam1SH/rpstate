use amethystate_macros::amethystate;
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
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::fmt::Debug for DatabaseConfig_Data {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        ::core::fmt::Formatter::debug_struct_field1_finish(
            f,
            "DatabaseConfig_Data",
            "host",
            &&self.host,
        )
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
        }
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
        };
        store.mark_initialized(<Self as ::amethystate::StateScope>::PREFIX)?;
        Ok(result)
    }
    #[doc(hidden)]
    pub fn __schema_field_db(&self) -> ::amethystate::ReadOnly<DatabaseConfig> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    pub fn db(&self) -> ::std::sync::Arc<DatabaseConfig<S>> {
        self.db.clone()
    }
    pub fn fork(&self) -> Self {
        self.fork_with_id(::amethystate::uuid::Uuid::new_v4())
    }
    #[doc(hidden)]
    pub fn fork_with_id(&self, new_id: ::amethystate::uuid::Uuid) -> Self {
        Self {
            __amethystate_instance_id: new_id,
            db: ::std::sync::Arc::new(self.db.fork_with_id(new_id)),
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
                false as usize + 1,
            )?;
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                "db",
                &self.db,
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
                        "db" => _serde::__private228::Ok(__Field::__field0),
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
                                    &"struct SystemSettings_Data with 1 element",
                                ),
                            );
                        }
                    };
                    _serde::__private228::Ok(SystemSettings_Data {
                        db: __field0,
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
                    _serde::__private228::Ok(SystemSettings_Data {
                        db: __field0,
                    })
                }
            }
            #[doc(hidden)]
            const FIELDS: &'static [&'static str] = &["db"];
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
        }
    }
}
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::fmt::Debug for SystemSettings_Data {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        ::core::fmt::Formatter::debug_struct_field1_finish(
            f,
            "SystemSettings_Data",
            "db",
            &&self.db,
        )
    }
}
impl SystemSettings_Data {}
impl ::amethystate::migration::types::AmeType for SystemSettings_Data {
    const TYPE_HASH: u32 = 0u32 ^ ::amethystate::migration::types::fnv1a("db".as_bytes())
        ^ <<DatabaseConfig<
            ::amethystate::DefaultStore,
        > as ::amethystate::AmeState>::Data as ::amethystate::migration::types::AmeType>::TYPE_HASH;
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
