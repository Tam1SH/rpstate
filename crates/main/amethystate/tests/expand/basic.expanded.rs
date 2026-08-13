use amethystate_macros::amethystate;
pub struct AppConfig<S: ::amethystate::Store = ::amethystate::DefaultStore> {
    __amethystate_instance_id: ::amethystate::uuid::Uuid,
    pub port: ::amethystate::Field<u16, S, ::amethystate::WritableMode>,
    pub session_id: ::amethystate::Field<String, S, ::amethystate::WritableMode>,
}
#[automatically_derived]
impl<S: ::core::clone::Clone + ::amethystate::Store> ::core::clone::Clone
for AppConfig<S> {
    #[inline]
    fn clone(&self) -> AppConfig<S> {
        AppConfig {
            __amethystate_instance_id: ::core::clone::Clone::clone(
                &self.__amethystate_instance_id,
            ),
            port: ::core::clone::Clone::clone(&self.port),
            session_id: ::core::clone::Clone::clone(&self.session_id),
        }
    }
}
impl<S: ::amethystate::Store> ::std::fmt::Debug for AppConfig<S>
where
    ::amethystate::Field<u16, S, ::amethystate::WritableMode>: ::std::fmt::Debug,
    ::amethystate::Field<String, S, ::amethystate::WritableMode>: ::std::fmt::Debug,
{
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.debug_struct("AppConfig")
            .field("port", &self.port)
            .field("session_id", &self.session_id)
            .finish()
    }
}
impl<S: ::amethystate::Store> ::amethystate::StateScope for AppConfig<S> {
    const PREFIX: &'static str = "app";
}
impl<S: ::amethystate::Store> AppConfig<S> {
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
            session_id: ::amethystate::Field::new_volatile_with_id(
                ::std::sync::Arc::from({
                    let prefix = <Self as ::amethystate::StateScope>::PREFIX;
                    if prefix == "." {
                        "session_id".to_string()
                    } else {
                        ::alloc::__export::must_use({
                            ::alloc::fmt::format(
                                format_args!("{0}.{1}", prefix, "session_id"),
                            )
                        })
                    }
                }),
                "localhost".to_string(),
                instance_id,
            ),
        };
        store.mark_initialized(<Self as ::amethystate::StateScope>::PREFIX)?;
        Ok(result)
    }
    #[doc(hidden)]
    pub fn __schema_field_port(&self) -> ::amethystate::ReadOnly<u16> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    #[doc(hidden)]
    pub fn __schema_field_session_id(&self) -> ::amethystate::ReadOnly<String> {
        ::core::panicking::panic("internal error: entered unreachable code")
    }
    pub fn port(&self) -> ::amethystate::Field<u16, S, ::amethystate::WritableMode> {
        self.port.clone()
    }
    pub fn session_id(
        &self,
    ) -> ::amethystate::Field<String, S, ::amethystate::WritableMode> {
        self.session_id.clone()
    }
    pub fn fork(&self) -> Self {
        self.fork_with_id(::amethystate::uuid::Uuid::new_v4())
    }
    #[doc(hidden)]
    pub fn fork_with_id(&self, new_id: ::amethystate::uuid::Uuid) -> Self {
        Self {
            __amethystate_instance_id: new_id,
            port: self.port.fork_with_id(new_id),
            session_id: self.session_id.fork_with_id(new_id),
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
            scope.watch(self.session_id.subscribe(move |_| cb_clone()));
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
                        .session_id
                        .subscription_with()
                        .external()
                        .register(move |_| cb_clone()),
                );
        }
        scope
    }
}
impl AppConfig<::amethystate::DefaultStore> {
    pub fn new() -> ::amethystate::StorageResult<Self> {
        let store = ::amethystate::global_store();
        Self::new_with(&store)
    }
}
impl<S: ::amethystate::Store> ::amethystate::AmeStateNode<S> for AppConfig<S> {
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
pub struct AppConfig_Data {
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
    impl _serde::Serialize for AppConfig_Data {
        fn serialize<__S>(
            &self,
            __serializer: __S,
        ) -> _serde::__private228::Result<__S::Ok, __S::Error>
        where
            __S: _serde::Serializer,
        {
            let mut __serde_state = _serde::Serializer::serialize_struct(
                __serializer,
                "AppConfig_Data",
                false as usize + 1,
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
    impl<'de> _serde::Deserialize<'de> for AppConfig_Data {
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
                        "port" => _serde::__private228::Ok(__Field::__field0),
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
                        b"port" => _serde::__private228::Ok(__Field::__field0),
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
                marker: _serde::__private228::PhantomData<AppConfig_Data>,
                lifetime: _serde::__private228::PhantomData<&'de ()>,
            }
            #[automatically_derived]
            impl<'de> _serde::de::Visitor<'de> for __Visitor<'de> {
                type Value = AppConfig_Data;
                fn expecting(
                    &self,
                    __formatter: &mut _serde::__private228::Formatter,
                ) -> _serde::__private228::fmt::Result {
                    _serde::__private228::Formatter::write_str(
                        __formatter,
                        "struct AppConfig_Data",
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
                        u16,
                    >(&mut __seq)? {
                        _serde::__private228::Some(__value) => __value,
                        _serde::__private228::None => {
                            return _serde::__private228::Err(
                                _serde::de::Error::invalid_length(
                                    0usize,
                                    &"struct AppConfig_Data with 1 element",
                                ),
                            );
                        }
                    };
                    _serde::__private228::Ok(AppConfig_Data { port: __field0 })
                }
                #[inline]
                fn visit_map<__A>(
                    self,
                    mut __map: __A,
                ) -> _serde::__private228::Result<Self::Value, __A::Error>
                where
                    __A: _serde::de::MapAccess<'de>,
                {
                    let mut __field0: _serde::__private228::Option<u16> = _serde::__private228::None;
                    while let _serde::__private228::Some(__key) = _serde::de::MapAccess::next_key::<
                        __Field,
                    >(&mut __map)? {
                        match __key {
                            __Field::__field0 => {
                                if _serde::__private228::Option::is_some(&__field0) {
                                    return _serde::__private228::Err(
                                        <__A::Error as _serde::de::Error>::duplicate_field("port"),
                                    );
                                }
                                __field0 = _serde::__private228::Some(
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
                            _serde::__private228::de::missing_field("port")?
                        }
                    };
                    _serde::__private228::Ok(AppConfig_Data { port: __field0 })
                }
            }
            #[doc(hidden)]
            const FIELDS: &'static [&'static str] = &["port"];
            _serde::Deserializer::deserialize_struct(
                __deserializer,
                "AppConfig_Data",
                FIELDS,
                __Visitor {
                    marker: _serde::__private228::PhantomData::<AppConfig_Data>,
                    lifetime: _serde::__private228::PhantomData,
                },
            )
        }
    }
};
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::default::Default for AppConfig_Data {
    #[inline]
    fn default() -> AppConfig_Data {
        AppConfig_Data {
            port: ::core::default::Default::default(),
        }
    }
}
#[automatically_derived]
#[allow(non_camel_case_types)]
impl ::core::clone::Clone for AppConfig_Data {
    #[inline]
    fn clone(&self) -> AppConfig_Data {
        AppConfig_Data {
            port: ::core::clone::Clone::clone(&self.port),
        }
    }
}
impl AppConfig_Data {}
impl ::amethystate::migration::types::AmeType for AppConfig_Data {
    const TYPE_HASH: u32 = 0u32
        ^ ::amethystate::migration::types::fnv1a("port".as_bytes())
        ^ <u16 as ::amethystate::migration::types::AmeType>::TYPE_HASH;
    const TYPE_NAME: &'static str = "AppConfig_Data";
}
impl ::amethystate::migration::fields::AmeStateFields for AppConfig_Data {
    const FIELDS: &'static [::amethystate::migration::fields::FieldDescriptor] = &[
        ::amethystate::migration::fields::FieldDescriptor {
            name: "port",
            type_hash: <u16 as ::amethystate::migration::types::AmeType>::TYPE_HASH,
            type_name: "u16",
        },
    ];
    const VERSION: u32 = 0u32;
    const SCHEMA_HASH: u32 = ::amethystate::migration::types::schema_hash(Self::FIELDS);
    const PARENT_PREFIX: &'static str = "app";
    const MIGRATION_DEPS: &'static [&'static str] = &[];
    fn load_struct(
        ctx: &mut ::amethystate::MigrationContext,
    ) -> ::amethystate::StorageResult<Self> {
        Ok(Self {
            port: ctx.get::<u16>("port")?.unwrap_or_else(|| 8080),
        })
    }
    fn save_struct(
        &self,
        ctx: &mut ::amethystate::MigrationContext,
    ) -> ::amethystate::StorageResult<()> {
        ctx.set("port", &self.port)?;
        Ok(())
    }
}
impl<S: ::amethystate::Store> ::amethystate::AmeState for AppConfig<S> {
    type Data = AppConfig_Data;
}
#[allow(non_upper_case_globals)]
const _: () = {
    static __INVENTORY: ::inventory::Node = ::inventory::Node {
        value: &{
            ::amethystate::observability::SchemaEntry {
                prefix: Some("app"),
                struct_name: "AppConfig",
                version: 0u32,
                schema_hash: <AppConfig_Data as ::amethystate::migration::types::AmeType>::TYPE_HASH,
                fields: <AppConfig_Data as ::amethystate::migration::fields::AmeStateFields>::FIELDS,
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
impl<S: ::amethystate::Store> ::amethystate::AmeStateSlice<S> for AppConfig<S> {
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
